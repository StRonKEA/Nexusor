//! Cloud Code `streamGenerateContent` adapter for Antigravity.
//!
//! Request/response shapes follow CLIProxyAPI antigravity executor + Gemini
//! translator evidence: `{project, model, request:{systemInstruction,contents,tools}}`
//! and SSE chunks under `response.candidates` or top-level `candidates`.

use std::collections::BTreeMap;

use async_stream::try_stream;
use base64::{engine::general_purpose::STANDARD, Engine};
use eventsource_stream::Eventsource;
use futures_util::StreamExt;
use serde_json::{json, Value};

use crate::{
    model::{
        ContentPart, ModelInvocation, ProjectedContent, ProjectedMessage, Role, ToolCallContent,
        Usage,
    },
    provider::{
        attempt::{send_once_with_header_timeout, Attempt},
        map_sse_error, CallRecorder, FinishReason, ModelEvent, Provider, ProviderStream,
    },
    Error, Result,
};

use super::{
    provider::{request_headers, stream_urls},
    resolve_model_and_thinking, AntigravityAccountData,
};

pub struct AntigravityCloudCodeProvider {
    client: reqwest::Client,
    account: AntigravityAccountData,
    upstream_model_id: String,
    effort_tiers: Vec<String>,
    recorder: Option<CallRecorder>,
    #[cfg(test)]
    test_stream_urls: Option<Vec<String>>,
}

impl AntigravityCloudCodeProvider {
    pub fn new(
        client: reqwest::Client,
        account: AntigravityAccountData,
        upstream_model_id: impl Into<String>,
    ) -> Self {
        Self {
            client,
            account,
            upstream_model_id: upstream_model_id.into(),
            effort_tiers: Vec::new(),
            recorder: None,
            #[cfg(test)]
            test_stream_urls: None,
        }
    }

    pub fn with_effort_tiers(mut self, effort_tiers: Vec<String>) -> Self {
        self.effort_tiers = effort_tiers;
        self
    }

    pub fn with_recorder(mut self, recorder: Option<CallRecorder>) -> Self {
        self.recorder = recorder;
        self
    }

    #[cfg(test)]
    pub(crate) fn with_test_stream_urls(mut self, urls: Option<Vec<String>>) -> Self {
        self.test_stream_urls = urls;
        self
    }
}

impl Provider for AntigravityCloudCodeProvider {
    fn stream(
        &self,
        invocation: ModelInvocation,
        cancellation: tokio_util::sync::CancellationToken,
    ) -> ProviderStream {
        let client = self.client.clone();
        let account = self.account.clone();
        let upstream_model_id = self.upstream_model_id.clone();
        let effort_tiers = self.effort_tiers.clone();
        let recorder = self.recorder.clone();
        let urls = stream_urls();
        #[cfg(test)]
        let urls = self.test_stream_urls.clone().unwrap_or(urls);
        Box::pin(try_stream! {
            let ModelInvocation { call_id, request, .. } = invocation;
            let project = account
                .project_id
                .clone()
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| Error::Provider(
                    "Antigravity account is missing projectId; refresh the Google account".into(),
                ))?;
            let (model, thinking_level) = resolve_model_and_thinking(
                &upstream_model_id,
                request.model.reasoning.effort.as_deref(),
                &effort_tiers,
            );
            let body = build_cloud_code_body(
                &project,
                &model,
                &request.prompt.instructions,
                &request.history,
                &request.prompt.tools,
                request.model.max_output_tokens,
                thinking_level.as_deref(),
            )?;
            let headers = request_headers(&account.access_token);
            if let Some(recorder) = &recorder {
                let mut header_json = serde_json::Map::new();
                for (name, value) in &headers {
                    if name.eq_ignore_ascii_case("authorization") {
                        continue;
                    }
                    header_json.insert(name.clone(), Value::String(value.clone()));
                }
                recorder
                    .request(Value::Object(header_json), &body)
                    .await?;
            }

            let mut last_error = None;
            let mut primary_account_error = None;
            let mut response = None;
            for (index, url) in urls.into_iter().enumerate() {
                let endpoint = url.clone();
                let headers = headers.clone();
                let body = body.clone();
                let client = client.clone();
                match send_once_with_header_timeout(
                    "Antigravity",
                    move || {
                        let mut builder = client.post(&url).json(&body);
                        for (name, value) in &headers {
                            if let (Ok(h), Ok(v)) = (
                                reqwest::header::HeaderName::from_bytes(name.as_bytes()),
                                reqwest::header::HeaderValue::from_str(value),
                            ) {
                                builder = builder.header(h, v);
                            }
                        }
                        builder
                    },
                    &cancellation,
                    recorder.as_ref(),
                    // This client has no request deadline. Bound each endpoint's
                    // header wait so a stalled host cannot block URL fallback.
                    // Once headers arrive, the existing stream idle budget applies.
                    std::time::Duration::from_secs(120),
                )
                .await
                {
                    Ok(Attempt::Cancelled) => return,
                    Ok(Attempt::Response(resp)) => {
                        response = Some(resp);
                        break;
                    }
                    // A 429 from one endpoint does not mean the account is out of
                    // quota: prod answers RESOURCE_EXHAUSTED for consumer accounts
                    // that the daily endpoint serves normally. Keep trying and only
                    // report the failure once every endpoint has been attempted.
                    Err(error) => {
                        tracing::warn!(%endpoint, %error, "Antigravity endpoint failed before streaming; trying next endpoint");
                        if index == 0 && account_failover_error(&error) {
                            primary_account_error = Some(error);
                        } else {
                            last_error = Some(error);
                        }
                    }
                }
            }
            let response = match response {
                Some(response) => response,
                None => Err(primary_account_error.or(last_error).unwrap_or_else(|| {
                    Error::Provider("Antigravity stream failed on all endpoints".into())
                }))?,
            };

            yield ModelEvent::Start { model_call_id: call_id };

            let chunk_recorder = recorder.clone();
            let chunks = response
                .bytes_stream()
                .map(|chunk| chunk.map_err(Error::from))
                .then(move |chunk| {
                    let recorder = chunk_recorder.clone();
                    async move {
                        let chunk = chunk?;
                        if let Some(recorder) = recorder {
                            recorder.response_chunk(&chunk).await?;
                        }
                        Ok::<_, Error>(chunk)
                    }
                });
            let source = chunks.eventsource();
            futures_util::pin_mut!(source);

            let mut text_open = false;
            let mut thinking_open = false;
            let mut tools = BTreeMap::<usize, ToolState>::new();
            let mut next_tool_index = 0usize;
            let mut final_usage = None;
            let mut finish = None;
            let mut saw_payload = false;

            loop {
                let event = tokio::select! {
                    _ = cancellation.cancelled() => return,
                    event = source.next() => event,
                };
                let Some(event) = event else { break };
                let event = event.map_err(|error| map_sse_error("Antigravity", error))?;
                let data = event.data.trim();
                if data.is_empty() || data == "[DONE]" {
                    continue;
                }
                let value: Value = match serde_json::from_str(data) {
                    Ok(value) => value,
                    Err(_) => continue,
                };
                if let Some(error) = cloud_code_error(&value) {
                    Err(error)?;
                }
                if let Some(usage) = parse_usage(&value) {
                    final_usage = Some(usage);
                }
                for part_event in parse_candidate_events(&value, &mut tools, &mut next_tool_index)? {
                    saw_payload = true;
                    match &part_event {
                        ModelEvent::TextDelta(_) if !text_open => {
                            yield ModelEvent::TextStart;
                            text_open = true;
                        }
                        ModelEvent::ThinkingDelta(_) if !thinking_open => {
                            yield ModelEvent::ThinkingStart;
                            thinking_open = true;
                        }
                        _ => {}
                    }
                    if matches!(part_event, ModelEvent::Done(_)) {
                        if let ModelEvent::Done(reason) = part_event {
                            finish = Some(reason);
                        }
                        continue;
                    }
                    yield part_event;
                }
            }

            if thinking_open {
                yield ModelEvent::ThinkingEnd;
            }
            if text_open {
                yield ModelEvent::TextEnd;
            }
            for index in tools.keys().copied().collect::<Vec<_>>() {
                yield ModelEvent::ToolCallEnd { index };
            }
            if let Some(usage) = final_usage {
                yield ModelEvent::Usage(usage);
            }
            let finish = finish
                .or_else(|| {
                    saw_payload.then_some(if tools.is_empty() {
                        FinishReason::Stop
                    } else {
                        FinishReason::ToolUse
                    })
                })
                .ok_or_else(|| {
                    Error::Provider("Antigravity stream ended without a response payload".into())
                })?;
            yield ModelEvent::Done(finish);
        })
    }
}

// Only the primary daily endpoint is authoritative here. A secondary prod 429
// may reject consumer credentials even when daily has capacity/quota.
fn account_failover_error(error: &Error) -> bool {
    let text = error.to_string();
    super::super::quota::is_account_quota_error("antigravity", &text)
        || super::super::quota::is_transient_capacity_error("antigravity", &text)
}

#[derive(Default)]
struct ToolState {
    call_id: String,
    name: String,
    started: bool,
}

/// Google Gemini API rejects JSON schema `$ref` keywords in function declarations with:
/// "Invalid JSON payload received. Unknown name '$ref' ... Cannot find field".
/// Recursively resolves local `$ref` (e.g. `#/properties/current_step`) or converts
/// unresolved references into basic string types so Gemini never fails with HTTP 400.
fn sanitize_gemini_schema(val: &Value, root: &Value) -> Value {
    match val {
        Value::Object(map) => {
            if let Some(ref_str) = map.get("$ref").and_then(Value::as_str) {
                let mut resolved = None;
                if let Some(path) = ref_str.strip_prefix("#/") {
                    let mut curr = root;
                    for part in path.split('/') {
                        if let Some(next) = curr.get(part) {
                            curr = next;
                        } else {
                            curr = &Value::Null;
                            break;
                        }
                    }
                    if curr.is_object() {
                        resolved = Some(curr.clone());
                    }
                }
                if let Some(mut target) = resolved {
                    if let Some(desc) = map.get("description") {
                        if let Some(target_map) = target.as_object_mut() {
                            target_map.insert("description".into(), desc.clone());
                        }
                    }
                    return sanitize_gemini_schema(&target, root);
                } else {
                    let mut fallback = serde_json::Map::new();
                    fallback.insert("type".into(), Value::String("string".into()));
                    if let Some(desc) = map.get("description") {
                        fallback.insert("description".into(), desc.clone());
                    }
                    return Value::Object(fallback);
                }
            }
            let mut sanitized = serde_json::Map::with_capacity(map.len());
            for (k, v) in map {
                sanitized.insert(k.clone(), sanitize_gemini_schema(v, root));
            }
            Value::Object(sanitized)
        }
        Value::Array(arr) => Value::Array(
            arr.iter()
                .map(|item| sanitize_gemini_schema(item, root))
                .collect(),
        ),
        _ => val.clone(),
    }
}

pub(crate) fn build_cloud_code_body(
    project: &str,
    model: &str,
    instructions: &str,
    history: &[ProjectedMessage],
    tools: &[crate::model::ToolDefinition],
    max_output_tokens: Option<u64>,
    thinking_level: Option<&str>,
) -> Result<Value> {
    let is_claude = model.contains("claude");
    let contents = cloud_code_contents(history, is_claude)?;
    let mut request = json!({
        "contents": contents,
    });
    if !instructions.is_empty() {
        request["systemInstruction"] = json!({
            "parts": [{"text": instructions}]
        });
    }
    if !tools.is_empty() {
        request["tools"] = json!([{
            "functionDeclarations": tools.iter().map(|tool| {
                let params = if tool.parameters.is_null() || !tool.parameters.is_object() {
                    json!({ "type": "object", "properties": {} })
                } else {
                    sanitize_gemini_schema(&tool.parameters, &tool.parameters)
                };
                json!({
                    "name": tool.name,
                    "description": tool.description,
                    "parameters": params,
                })
            }).collect::<Vec<_>>()
        }]);
    }
    let mut generation = serde_json::Map::new();
    if let Some(max) = max_output_tokens {
        generation.insert("maxOutputTokens".into(), json!(max));
    }
    // Gemini 3.x: thinkingLevel via Cursor Effort (CLIProxyAPI parity).
    if let Some(level) = thinking_level.filter(|value| !value.is_empty()) {
        if model.contains("gemini") {
            generation.insert(
                "thinkingConfig".into(),
                json!({
                    "thinkingLevel": level,
                    "includeThoughts": true,
                }),
            );
        }
    }
    if !generation.is_empty() {
        request["generationConfig"] = Value::Object(generation);
    }
    Ok(json!({
        "project": project,
        "model": model,
        "request": request,
    }))
}

fn cloud_code_contents(history: &[ProjectedMessage], is_claude: bool) -> Result<Vec<Value>> {
    let mut contents = Vec::with_capacity(history.len());
    for message in history {
        match &message.content {
            ProjectedContent::Parts(parts) => {
                let role = match message.role {
                    Role::Assistant => "model",
                    _ => "user",
                };
                contents.push(json!({
                    "role": role,
                    "parts": content_parts(parts)?,
                }));
            }
            ProjectedContent::Assistant {
                text,
                thinking,
                calls,
                ..
            } => {
                if text.is_empty() && thinking.is_empty() && calls.is_empty() {
                    continue;
                }
                let mut parts = Vec::new();
                // Anthropic Claude strictly requires an authentic cryptographic signature
                // on any thinking block passed back in conversation history. If the model
                // is Claude, or if the thought was produced by Gemini without an Anthropic signature,
                // sending {"thought": true} causes Vertex AI Anthropic 400 Bad Request:
                // "messages.X.content.0.thinking.signature: Field required".
                // When targeting Claude, omit raw previous thoughts from history.
                if !thinking.is_empty() && !is_claude {
                    parts.push(json!({"text": thinking, "thought": true}));
                }
                if !text.is_empty() {
                    parts.push(json!({"text": text}));
                }
                for call in calls {
                    parts.push(function_call_part(call)?);
                }
                if parts.is_empty() {
                    continue;
                }
                contents.push(json!({ "role": "model", "parts": parts }));
            }
            ProjectedContent::ToolResult(result) => {
                let response = if result.content.trim().is_empty() {
                    json!({ "result": "" })
                } else if let Ok(parsed) = serde_json::from_str::<Value>(&result.content) {
                    if parsed.is_object() {
                        parsed
                    } else {
                        json!({ "result": parsed })
                    }
                } else {
                    json!({ "result": result.content })
                };
                let mut parts = vec![json!({
                    "functionResponse": {
                        "id": result.call_id,
                        "name": result.name,
                        "response": response,
                    }
                })];
                for part in &result.provider_parts {
                    if matches!(part, ContentPart::Image { .. }) {
                        parts.extend(content_parts(std::slice::from_ref(part))?);
                    }
                }
                contents.push(json!({ "role": "user", "parts": parts }));
            }
        }
    }
    Ok(contents)
}

fn content_parts(parts: &[ContentPart]) -> Result<Vec<Value>> {
    parts
        .iter()
        .map(|part| match part {
            ContentPart::Text { text } => Ok(json!({ "text": text })),
            ContentPart::Image { mime_type, data } => Ok(json!({
                "inlineData": {
                    "mimeType": mime_type,
                    "data": STANDARD.encode(data),
                }
            })),
        })
        .collect()
}

fn function_call_part(call: &ToolCallContent) -> Result<Value> {
    Ok(json!({
        // Google Gemini 2.5 / 3+ thinking models strictly enforce a thought signature on
        // functionCall parts during multi-turn replay. For tools called across turns (or
        // originated from another model in a combo/fallback), Google's documented bypass
        // token skips the internal validator and prevents HTTP 400 INVALID_ARGUMENT failures.
        "thoughtSignature": "skip_thought_signature_validator",
        "functionCall": {
            "id": call.call_id,
            "name": call.name,
            "args": call.arguments,
        }
    }))
}

fn cloud_code_error(value: &Value) -> Option<Error> {
    let error = value.get("error")?;
    // Keep the envelope: quota classification needs status/reason/retryDelay,
    // including when an HTTP 200 stream carries a structured 429 error.
    Some(Error::Provider(format!(
        "Antigravity error: {}",
        json!({"error": error})
    )))
}

fn parse_usage(value: &Value) -> Option<Usage> {
    let usage = value
        .pointer("/response/usageMetadata")
        .or_else(|| value.get("usageMetadata"))?;
    Some(Usage {
        input_tokens: usage.get("promptTokenCount").and_then(Value::as_u64),
        context_input_tokens: None,
        output_tokens: usage.get("candidatesTokenCount").and_then(Value::as_u64),
        total_tokens: usage.get("totalTokenCount").and_then(Value::as_u64),
        cache_read_tokens: usage.get("cachedContentTokenCount").and_then(Value::as_u64),
        cache_write_tokens: None,
        reasoning_tokens: usage.get("thoughtsTokenCount").and_then(Value::as_u64),
    })
}

fn parse_candidate_events(
    value: &Value,
    tools: &mut BTreeMap<usize, ToolState>,
    next_tool_index: &mut usize,
) -> Result<Vec<ModelEvent>> {
    let candidates = value
        .pointer("/response/candidates")
        .or_else(|| value.get("candidates"))
        .and_then(Value::as_array);

    let mut events = Vec::new();
    let Some(candidates) = candidates else {
        return Ok(events);
    };

    for candidate in candidates {
        let parts = candidate
            .pointer("/content/parts")
            .and_then(Value::as_array);
        let mut saw_tool_in_candidate = false;
        if let Some(parts) = parts {
            for part in parts {
                if part
                    .get("thought")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                {
                    if let Some(text) = part.get("text").and_then(Value::as_str) {
                        if !text.is_empty() {
                            events.push(ModelEvent::ThinkingDelta(text.to_owned()));
                        }
                    }
                    continue;
                }
                if let Some(text) = part.get("text").and_then(Value::as_str) {
                    if !text.is_empty() {
                        events.push(ModelEvent::TextDelta(text.to_owned()));
                    }
                }
                if let Some(call) = part.get("functionCall") {
                    saw_tool_in_candidate = true;
                    let name = call
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("tool")
                        .to_owned();
                    let call_id = call
                        .get("id")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                        .unwrap_or_else(|| format!("antigravity-tool-{next_tool_index}"));
                    let args_text = if let Some(args) = call.get("args") {
                        serde_json::to_string(args)?
                    } else {
                        "{}".to_owned()
                    };
                    let index = *next_tool_index;
                    *next_tool_index += 1;
                    let tool = tools.entry(index).or_default();
                    tool.call_id = call_id.clone();
                    tool.name = name.clone();
                    if !tool.started {
                        tool.started = true;
                        events.push(ModelEvent::ToolCallStart {
                            index,
                            call_id,
                            name,
                        });
                    }
                    if !args_text.is_empty() {
                        events.push(ModelEvent::ToolCallArgumentsDelta {
                            index,
                            delta: args_text,
                        });
                    }
                }
            }
        }
        if let Some(reason) = candidate.get("finishReason").and_then(Value::as_str) {
            events.push(ModelEvent::Done(map_finish(
                reason,
                saw_tool_in_candidate || !tools.is_empty(),
            )));
        }
    }
    Ok(events)
}

fn map_finish(value: &str, has_tools: bool) -> FinishReason {
    match value {
        "MAX_TOKENS" | "LENGTH" => FinishReason::Length,
        "STOP" | "FINISH_REASON_UNSPECIFIED" | "" if has_tools => FinishReason::ToolUse,
        "STOP" | "FINISH_REASON_UNSPECIFIED" | "" => FinishReason::Stop,
        _ if has_tools => FinishReason::ToolUse,
        _ => FinishReason::Stop,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ProjectedContent, ProjectedMessage, Role, ToolDefinition};

    #[tokio::test]
    async fn endpoint_fallback_preserves_primary_account_failure_but_allows_success() {
        use crate::model::{ModelRequest, ModelSpec, PromptSpec};
        use axum::{http::StatusCode, routing::post, Router};
        let quota = r#"{"error":{"code":429,"status":"RESOURCE_EXHAUSTED","details":[{"reason":"QUOTA_EXHAUSTED"},{"@type":"type.googleapis.com/google.rpc.RetryInfo","retryDelay":"300s"}]}}"#;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(listener, Router::new()
                .route("/quota", post(move || async move { (StatusCode::TOO_MANY_REQUESTS, quota) }))
                .route("/capacity", post(|| async { (StatusCode::SERVICE_UNAVAILABLE, "no capacity available") }))
                .route("/missing", post(|| async { StatusCode::NOT_FOUND }))
                .route("/ok", post(|| async { ([ ("content-type", "text/event-stream") ],
                    "data: {\"response\":{\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"FALLBACK_OK\"}]},\"finishReason\":\"STOP\"}]}}\n\n") }))
            ).await.unwrap();
        });
        for (paths, expected) in [
            (vec!["quota", "missing", "missing"], "quota"),
            (vec!["capacity", "missing", "missing"], "capacity"),
            (vec!["missing", "quota", "missing"], "missing"),
            (vec!["quota", "ok"], "ok"),
        ] {
            let mut provider = AntigravityCloudCodeProvider::new(
                reqwest::Client::builder().no_proxy().build().unwrap(),
                AntigravityAccountData {
                    access_token: "test-only".into(),
                    refresh_token: None,
                    project_id: Some("fixture".into()),
                    display_name: String::new(),
                    expires_at_ms: None,
                },
                "claude-sonnet-4-6",
            );
            provider.test_stream_urls =
                Some(paths.iter().map(|path| format!("{base}/{path}")).collect());
            let invocation = ModelInvocation {
                call_id: "fixture-call".into(),
                run_id: "fixture-run".into(),
                conversation_id: "fixture-conversation".into(),
                provider_call_index: 0,
                slot_account_ids: None,
                slot_strategy: None,
                request: ModelRequest {
                    model: ModelSpec {
                        model_id: "claude-sonnet-4-6".into(),
                        display_name: None,
                        reasoning: Default::default(),
                        latency: Default::default(),
                        max_output_tokens: None,
                        context_window_tokens: None,
                        supports_image_generation: false,
                        extra_params: json!({}),
                    },
                    prompt: PromptSpec {
                        instructions: String::new(),
                        tools: vec![],
                    },
                    history: vec![ProjectedMessage {
                        message_id: "user".into(),
                        role: Role::User,
                        content: ProjectedContent::Parts(vec![ContentPart::Text {
                            text: "hello".into(),
                        }]),
                    }],
                },
            };
            let events = provider
                .stream(invocation, tokio_util::sync::CancellationToken::new())
                .collect::<Vec<_>>()
                .await;
            if expected == "ok" {
                assert!(events.iter().all(Result::is_ok));
                assert!(events.iter().any(|event| matches!(event, Ok(ModelEvent::TextDelta(text)) if text == "FALLBACK_OK")));
            } else {
                let error = events
                    .into_iter()
                    .find_map(Result::err)
                    .expect("expected error");
                let text = error.to_string();
                match expected {
                    "quota" => {
                        assert!(super::super::super::quota::is_account_quota_error(
                            "antigravity",
                            &text
                        ));
                        assert_eq!(
                            super::super::super::quota::quota_cooldown("antigravity", &text)
                                .as_secs(),
                            300
                        );
                    }
                    "capacity" => {
                        assert!(super::super::super::quota::is_transient_capacity_error(
                            "antigravity",
                            &text
                        ));
                        assert!(!super::super::super::quota::is_account_quota_error(
                            "antigravity",
                            &text
                        ));
                    }
                    _ => {
                        assert!(text.contains("404"));
                        assert!(!account_failover_error(&error));
                    }
                }
            }
        }
        server.abort();
    }

    #[test]
    fn streaming_error_retains_quota_reason_and_retry_delay() {
        use crate::provider::providers::quota::{
            is_account_quota_error, is_transient_capacity_error, quota_cooldown,
        };
        let value = json!({"error":{"code":429,"status":"RESOURCE_EXHAUSTED","message":"limit",
            "details":[{"reason":"QUOTA_EXHAUSTED"},{"@type":"type.googleapis.com/google.rpc.RetryInfo","retryDelay":"300s"}]}});
        let error = cloud_code_error(&value).unwrap().to_string();
        assert!(is_account_quota_error("antigravity", &error));
        assert_eq!(
            quota_cooldown("antigravity", &error),
            std::time::Duration::from_secs(300)
        );
        let capacity = cloud_code_error(
            &json!({"error":{"code":503,"status":"UNAVAILABLE","message":"busy"}}),
        )
        .unwrap()
        .to_string();
        assert!(is_transient_capacity_error("antigravity", &capacity));
        assert!(!is_account_quota_error("antigravity", &capacity));
        assert!(cloud_code_error(&json!({"candidates":[]})).is_none());
        assert!(cloud_code_error(&json!({"error":"message"}))
            .unwrap()
            .to_string()
            .contains("message"));
    }

    #[test]
    fn tool_responses_are_structs_even_when_output_is_json_scalar_or_array() {
        for (text, expected) in [
            ("2485", json!({"result":2485})),
            ("[1,2]", json!({"result":[1,2]})),
            ("null", json!({"result":null})),
            ("true", json!({"result":true})),
            ("\"ok\"", json!({"result":"ok"})),
            ("{\"balance\":2485}", json!({"balance":2485})),
            ("plain output", json!({"result":"plain output"})),
        ] {
            let history = vec![ProjectedMessage {
                message_id: "result".into(),
                role: Role::Tool,
                content: ProjectedContent::ToolResult(crate::model::ToolResultContent {
                    call_id: "shell-1".into(),
                    name: "Shell".into(),
                    content: text.into(),
                    is_error: false,
                    image: None,
                    images: vec![],
                    provider_parts: vec![],
                }),
            }];
            for is_claude in [false, true] {
                let contents = cloud_code_contents(&history, is_claude).unwrap();
                assert_eq!(
                    contents[0]["parts"][0]["functionResponse"]["response"],
                    expected
                );
            }
        }
    }

    #[test]
    fn tool_result_images_reach_cloud_code_in_order_with_call_identity() {
        let history = vec![ProjectedMessage {
            message_id: "media-result".into(),
            role: Role::Tool,
            content: ProjectedContent::ToolResult(crate::model::ToolResultContent {
                call_id: "read-media".into(),
                name: "Read".into(),
                content: "pages 1, 2".into(),
                is_error: false,
                image: None,
                images: vec![],
                provider_parts: vec![
                    ContentPart::Text {
                        text: "pages 1, 2".into(),
                    },
                    ContentPart::Image {
                        mime_type: "image/png".into(),
                        data: vec![1, 2, 3],
                    },
                    ContentPart::Image {
                        mime_type: "image/png".into(),
                        data: vec![4, 5, 6],
                    },
                ],
            }),
        }];
        for is_claude in [false, true] {
            let contents = cloud_code_contents(&history, is_claude).unwrap();
            let parts = contents[0]["parts"].as_array().unwrap();
            assert_eq!(parts.len(), 3);
            assert_eq!(parts[0]["functionResponse"]["id"], "read-media");
            assert_eq!(
                parts[0]["functionResponse"]["response"]["result"],
                "pages 1, 2"
            );
            assert_eq!(parts[1]["inlineData"]["data"], "AQID");
            assert_eq!(parts[2]["inlineData"]["data"], "BAUG");
            assert_eq!(parts[1]["inlineData"]["mimeType"], "image/png");
        }
    }

    #[test]
    fn builds_cloud_code_body_with_project_model_and_tools() {
        let history = vec![ProjectedMessage {
            message_id: "1".into(),
            role: Role::User,
            content: ProjectedContent::Parts(vec![ContentPart::Text {
                text: "hello".into(),
            }]),
        }];
        let tools = vec![ToolDefinition {
            name: "Shell".into(),
            description: "run".into(),
            parameters: json!({"type":"object"}),
        }];
        let body = build_cloud_code_body(
            "project-1",
            "gemini-2.5-flash",
            "be helpful",
            &history,
            &tools,
            Some(1024),
            Some("high"),
        )
        .unwrap();
        assert_eq!(body["project"], "project-1");
        assert_eq!(body["model"], "gemini-2.5-flash");
        assert_eq!(
            body["request"]["systemInstruction"]["parts"][0]["text"],
            "be helpful"
        );
        assert_eq!(body["request"]["contents"][0]["role"], "user");
        assert_eq!(
            body["request"]["tools"][0]["functionDeclarations"][0]["name"],
            "Shell"
        );
        assert_eq!(body["request"]["generationConfig"]["maxOutputTokens"], 1024);
        assert_eq!(
            body["request"]["generationConfig"]["thinkingConfig"]["thinkingLevel"],
            "high"
        );
    }

    #[test]
    fn builds_cloud_code_body_includes_thought_signature_on_function_calls() {
        let history = vec![
            ProjectedMessage {
                message_id: "1".into(),
                role: Role::User,
                content: ProjectedContent::Parts(vec![ContentPart::Text {
                    text: "search".into(),
                }]),
            },
            ProjectedMessage {
                message_id: "2".into(),
                role: Role::Assistant,
                content: ProjectedContent::Assistant {
                    text: "".into(),
                    thinking: "".into(),
                    replay_state: None,
                    calls: vec![ToolCallContent {
                        index: 0,
                        call_id: "call-1".into(),
                        name: "Grep".into(),
                        arguments: json!({"pattern": "foo"}),
                    }],
                },
            },
        ];
        let body = build_cloud_code_body(
            "project-1",
            "gemini-3.8-flash",
            "",
            &history,
            &[],
            None,
            None,
        )
        .unwrap();

        let model_part = &body["request"]["contents"][1]["parts"][0];
        assert_eq!(
            model_part["thoughtSignature"],
            "skip_thought_signature_validator"
        );
        assert_eq!(model_part["functionCall"]["name"], "Grep");
    }

    #[test]
    fn sanitizes_json_schema_ref_in_tool_declarations() {
        let history = vec![ProjectedMessage {
            message_id: "1".into(),
            role: Role::User,
            content: ProjectedContent::Parts(vec![ContentPart::Text {
                text: "test".into(),
            }]),
        }];
        let tools = vec![ToolDefinition {
            name: "UpdateCurrentStep".into(),
            description: "step update".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "current_step": {
                        "type": "string",
                        "description": "current phase"
                    },
                    "completed_subtitle": {
                        "$ref": "#/properties/current_step",
                        "description": "summary"
                    }
                }
            }),
        }];
        let body = build_cloud_code_body(
            "project-1",
            "gemini-3.8-flash",
            "",
            &history,
            &tools,
            None,
            None,
        )
        .unwrap();

        let tool_decl = &body["request"]["tools"][0]["functionDeclarations"][0];
        let completed_prop = &tool_decl["parameters"]["properties"]["completed_subtitle"];
        assert_eq!(completed_prop["type"], "string");
        assert_eq!(completed_prop["description"], "summary");
        assert!(completed_prop.get("$ref").is_none());
    }

    #[test]
    fn parses_sse_candidate_text_and_tool_call() {
        let value = json!({
            "response": {
                "candidates": [{
                    "content": {
                        "role": "model",
                        "parts": [
                            {"text": "hi"},
                            {"functionCall": {"id": "c1", "name": "Shell", "args": {"cmd": "ls"}}}
                        ]
                    },
                    "finishReason": "STOP"
                }],
                "usageMetadata": {
                    "promptTokenCount": 10,
                    "candidatesTokenCount": 3,
                    "totalTokenCount": 13
                }
            }
        });
        let mut tools = BTreeMap::new();
        let mut next = 0;
        let events = parse_candidate_events(&value, &mut tools, &mut next).unwrap();
        assert!(events
            .iter()
            .any(|e| matches!(e, ModelEvent::TextDelta(t) if t == "hi")));
        assert!(events.iter().any(|e| matches!(
            e,
            ModelEvent::ToolCallStart { name, .. } if name == "Shell"
        )));
        assert!(events
            .iter()
            .any(|e| matches!(e, ModelEvent::Done(FinishReason::ToolUse))));
        let usage = parse_usage(&value).unwrap();
        assert_eq!(usage.input_tokens, Some(10));
        assert_eq!(usage.output_tokens, Some(3));
    }

    #[test]
    fn claude_model_omits_unsigned_thinking_from_history() {
        let history = vec![
            ProjectedMessage {
                message_id: "m-1".into(),
                role: Role::User,
                content: ProjectedContent::Parts(vec![ContentPart::Text {
                    text: "hello".into(),
                }]),
            },
            ProjectedMessage {
                message_id: "m-2".into(),
                role: Role::Assistant,
                content: ProjectedContent::Assistant {
                    text: "Here is your plan.".into(),
                    thinking: "I am thinking deeply about this...".into(),
                    replay_state: None,
                    calls: vec![],
                },
            },
        ];

        // For Gemini: thought should be present
        let gemini_body =
            build_cloud_code_body("p-1", "gemini-3.8-flash", "", &history, &[], None, None)
                .unwrap();
        let gemini_parts = gemini_body["request"]["contents"][1]["parts"]
            .as_array()
            .unwrap();
        assert_eq!(gemini_parts.len(), 2);
        assert_eq!(gemini_parts[0]["thought"], true);

        // For Claude: unsigned thought must be omitted to prevent Anthropic signature error
        let claude_body =
            build_cloud_code_body("p-1", "claude-opus-4-6", "", &history, &[], None, None).unwrap();
        let claude_parts = claude_body["request"]["contents"][1]["parts"]
            .as_array()
            .unwrap();
        assert_eq!(claude_parts.len(), 1);
        assert_eq!(claude_parts[0]["text"], "Here is your plan.");
        assert!(claude_parts[0].get("thought").is_none());
    }
}
