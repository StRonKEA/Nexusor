//! OpenAI-compatible API routes (/v1/models and /v1/chat/completions).

use std::convert::Infallible;

use async_stream::stream;
use axum::{
    body::Body,
    extract::State,
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

use crate::{
    api::common::{budget_to_reasoning_effort, parse_openai_tools},
    cursor::transport::TransportRegistry,
    model::{
        ContentPart, ModelInvocation, ModelRequest, ModelSpec, ProjectedContent,
        ProjectedMessage, PromptSpec, Role, ToolCallContent, ToolResultContent,
    },
    provider::{FinishReason, ModelEvent},
};

pub fn router(registry: TransportRegistry) -> Router {
    Router::new()
        .route("/v1/models", get(list_models))
        .route("/v1/chat/completions", post(chat_completions))
        .route("/v1/completions", post(text_completions))
        .with_state(registry)
}

#[derive(Debug, Serialize)]
struct OpenAIModelItem {
    id: String,
    object: &'static str,
    created: u64,
    owned_by: &'static str,
}

#[derive(Debug, Serialize)]
struct OpenAIModelsResponse {
    object: &'static str,
    data: Vec<OpenAIModelItem>,
}

async fn list_models(
    State(registry): State<TransportRegistry>,
) -> Result<Json<OpenAIModelsResponse>, (StatusCode, String)> {
    let store = registry.store();
    let mut model_ids = Vec::new();

    // 1. Get models configured in Store
    if let Ok(configs) = store.models().await {
        for config in configs {
            model_ids.push(config.model_id);
        }
    }

    // 2. Get models from Plugins (Antigravity, Codex, Grok, etc.)
    if let Some(plugins) = registry.plugins() {
        let plugin_models = plugins.configured_models().await;
        for m in plugin_models {
            model_ids.push(m.model_id);
        }
    }

    // 3. Add Combos and Auto Routers
    if let Ok(combos) = store.router_combos().await {
        for c in combos {
            if c.enabled {
                model_ids.push(format!("combo:{}", c.combo_id));
            }
        }
    }
    model_ids.push("auto-smart".to_string());
    model_ids.push("auto".to_string());

    model_ids.sort();
    model_ids.dedup();

    let data = model_ids
        .into_iter()
        .map(|id| OpenAIModelItem {
            id,
            object: "model",
            created: 1700000000,
            owned_by: "nexusor",
        })
        .collect();

    Ok(Json(OpenAIModelsResponse {
        object: "list",
        data,
    }))
}

#[derive(Debug, Deserialize)]
struct ChatCompletionRequest {
    model: String,
    messages: Vec<Value>,
    #[serde(default)]
    tools: Option<Value>,
    #[serde(default)]
    reasoning_effort: Option<String>,
    #[serde(default)]
    thinking_budget: Option<i64>,
    #[serde(default)]
    stream: bool,
    #[serde(default)]
    _temperature: Option<f32>,
    #[serde(default)]
    max_tokens: Option<u64>,
}

async fn chat_completions(
    State(registry): State<TransportRegistry>,
    Json(payload): Json<ChatCompletionRequest>,
) -> Response {
    let call_id = format!("nexusor-call-{}", uuid::Uuid::new_v4());
    let run_id = format!("nexusor-run-{}", uuid::Uuid::new_v4());
    let conversation_id = format!("nexusor-conv-{}", uuid::Uuid::new_v4());

    let mut history = Vec::new();
    let mut instructions = String::new();

    for msg in payload.messages {
        let role_str = msg.get("role").and_then(|v| v.as_str()).unwrap_or("user");
        let content_val = msg.get("content");

        match role_str {
            "system" => {
                if let Some(c) = content_val {
                    if let Some(text) = c.as_str() {
                        if !instructions.is_empty() {
                            instructions.push_str("\n\n");
                        }
                        instructions.push_str(text);
                    }
                }
            }
            "assistant" => {
                let mut text = String::new();
                if let Some(s) = content_val.and_then(|v| v.as_str()) {
                    text = s.to_string();
                }

                let mut calls = Vec::new();
                if let Some(Value::Array(tool_calls)) = msg.get("tool_calls") {
                    for (idx, tc) in tool_calls.iter().enumerate() {
                        let id = tc.get("id").and_then(|v| v.as_str()).unwrap_or_default().to_string();
                        let name = tc.pointer("/function/name").and_then(|v| v.as_str()).unwrap_or_default().to_string();
                        let args_str = tc.pointer("/function/arguments").and_then(|v| v.as_str()).unwrap_or("{}");
                        let arguments = serde_json::from_str(args_str).unwrap_or_else(|_| serde_json::json!({}));
                        calls.push(ToolCallContent {
                            index: idx,
                            call_id: id,
                            name,
                            arguments,
                        });
                    }
                }

                history.push(ProjectedMessage {
                    message_id: uuid::Uuid::new_v4().to_string(),
                    role: Role::Assistant,
                    content: ProjectedContent::Assistant {
                        text,
                        thinking: String::new(),
                        replay_state: None,
                        calls,
                    },
                });
            }
            "tool" => {
                let call_id = msg.get("tool_call_id").and_then(|v| v.as_str()).unwrap_or_default().to_string();
                let content_text = content_val.and_then(|v| v.as_str()).unwrap_or_default().to_string();
                history.push(ProjectedMessage {
                    message_id: uuid::Uuid::new_v4().to_string(),
                    role: Role::Tool,
                    content: ProjectedContent::ToolResult(ToolResultContent {
                        call_id,
                        name: String::new(),
                        content: content_text,
                        is_error: false,
                        image: None,
                        images: Vec::new(),
                        provider_parts: Vec::new(),
                    }),
                });
            }
            _ => {
                let text = match content_val {
                    Some(Value::String(s)) => s.clone(),
                    Some(Value::Array(arr)) => {
                        let mut parts = Vec::new();
                        for item in arr {
                            if let Some(t) = item.get("text").and_then(|v| v.as_str()) {
                                parts.push(t.to_string());
                            }
                        }
                        parts.join("\n")
                    }
                    _ => String::new(),
                };

                history.push(ProjectedMessage {
                    message_id: uuid::Uuid::new_v4().to_string(),
                    role: Role::User,
                    content: ProjectedContent::Parts(vec![ContentPart::Text { text }]),
                });
            }
        }
    }

    let tools = parse_openai_tools(payload.tools.as_ref());

    let mut model_spec = ModelSpec::new(&payload.model);
    model_spec.max_output_tokens = payload.max_tokens.or(Some(4096));

    // Thinking/Reasoning effort mapping
    if let Some(effort) = payload.reasoning_effort {
        model_spec.reasoning.effort = Some(effort);
        model_spec.reasoning.enabled = true;
    } else if let Some(budget) = payload.thinking_budget {
        if let Some(effort) = budget_to_reasoning_effort(budget) {
            model_spec.reasoning.effort = Some(effort);
            model_spec.reasoning.enabled = true;
        }
    }

    let invocation = ModelInvocation {
        call_id: call_id.clone(),
        run_id,
        conversation_id,
        provider_call_index: 1,
        request: ModelRequest {
            prompt: PromptSpec {
                instructions,
                tools,
            },
            model: model_spec,
            history,
        },
        slot_account_ids: None,
        slot_strategy: None,
    };

    let cancellation = CancellationToken::new();
    let provider = registry.provider();
    let mut event_stream = provider.stream(invocation, cancellation);

    if payload.stream {
        let model = payload.model;
        let s = stream! {
            while let Some(item) = event_stream.next().await {
                match item {
                    Ok(ModelEvent::TextDelta(text)) => {
                        let chunk = json!({
                            "id": &call_id,
                            "object": "chat.completion.chunk",
                            "created": 1700000000,
                            "model": &model,
                            "choices": [{
                                "index": 0,
                                "delta": {
                                    "content": text
                                },
                                "finish_reason": null
                            }]
                        });
                        yield Ok::<_, Infallible>(format!("data: {}\n\n", serde_json::to_string(&chunk).unwrap()));
                    }
                    Ok(ModelEvent::ToolCallStart { index, call_id: tc_id, name }) => {
                        let chunk = json!({
                            "id": &call_id,
                            "object": "chat.completion.chunk",
                            "created": 1700000000,
                            "model": &model,
                            "choices": [{
                                "index": 0,
                                "delta": {
                                    "tool_calls": [{
                                        "index": index,
                                        "id": tc_id,
                                        "type": "function",
                                        "function": {
                                            "name": name,
                                            "arguments": ""
                                        }
                                    }]
                                },
                                "finish_reason": null
                            }]
                        });
                        yield Ok::<_, Infallible>(format!("data: {}\n\n", serde_json::to_string(&chunk).unwrap()));
                    }
                    Ok(ModelEvent::ToolCallArgumentsDelta { index, delta }) => {
                        let chunk = json!({
                            "id": &call_id,
                            "object": "chat.completion.chunk",
                            "created": 1700000000,
                            "model": &model,
                            "choices": [{
                                "index": 0,
                                "delta": {
                                    "tool_calls": [{
                                        "index": index,
                                        "function": {
                                            "arguments": delta
                                        }
                                    }]
                                },
                                "finish_reason": null
                            }]
                        });
                        yield Ok::<_, Infallible>(format!("data: {}\n\n", serde_json::to_string(&chunk).unwrap()));
                    }
                    Ok(ModelEvent::Done(finish)) => {
                        let finish_reason = match finish {
                            FinishReason::ToolUse => "tool_calls",
                            FinishReason::Length => "length",
                            FinishReason::Stop => "stop",
                        };
                        let chunk = json!({
                            "id": &call_id,
                            "object": "chat.completion.chunk",
                            "created": 1700000000,
                            "model": &model,
                            "choices": [{
                                "index": 0,
                                "delta": {},
                                "finish_reason": finish_reason
                            }]
                        });
                        yield Ok::<_, Infallible>(format!("data: {}\n\n", serde_json::to_string(&chunk).unwrap()));
                        yield Ok::<_, Infallible>("data: [DONE]\n\n".to_string());
                        break;
                    }
                    Err(err) => {
                        let err_chunk = json!({
                            "error": {
                                "message": err.to_string(),
                                "type": "nexusor_provider_error"
                            }
                        });
                        yield Ok::<_, Infallible>(format!("data: {}\n\n", serde_json::to_string(&err_chunk).unwrap()));
                        yield Ok::<_, Infallible>("data: [DONE]\n\n".to_string());
                        break;
                    }
                    _ => {}
                }
            }
        };

        let mut headers = HeaderMap::new();
        headers.insert(header::CONTENT_TYPE, "text/event-stream".parse().unwrap());
        headers.insert(header::CACHE_CONTROL, "no-cache".parse().unwrap());
        headers.insert(header::CONNECTION, "keep-alive".parse().unwrap());

        (headers, Body::from_stream(s)).into_response()
    } else {
        let mut full_text = String::new();
        let mut tool_calls = Vec::new();
        let mut finish_reason = "stop".to_string();

        while let Some(item) = event_stream.next().await {
            match item {
                Ok(ModelEvent::TextDelta(text)) => {
                    full_text.push_str(&text);
                }
                Ok(ModelEvent::ToolCallStart { index, call_id: tc_id, name }) => {
                    tool_calls.push(json!({
                        "id": tc_id,
                        "type": "function",
                        "function": {
                            "name": name,
                            "arguments": ""
                        }
                    }));
                    let _ = index;
                }
                Ok(ModelEvent::ToolCallArgumentsDelta { index, delta }) => {
                    if let Some(call) = tool_calls.get_mut(index) {
                        if let Some(args) = call.pointer_mut("/function/arguments") {
                            if let Some(s) = args.as_str() {
                                *args = Value::String(format!("{s}{delta}"));
                            }
                        }
                    }
                }
                Ok(ModelEvent::Done(finish)) => {
                    finish_reason = match finish {
                        FinishReason::ToolUse => "tool_calls",
                        FinishReason::Length => "length",
                        FinishReason::Stop => "stop",
                    }.to_string();
                    break;
                }
                Err(err) => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({
                            "error": {
                                "message": err.to_string(),
                                "type": "nexusor_provider_error"
                            }
                        })),
                    ).into_response();
                }
                _ => {}
            }
        }

        let mut message = json!({
            "role": "assistant"
        });
        if !full_text.is_empty() {
            message["content"] = Value::String(full_text);
        } else {
            message["content"] = Value::Null;
        }
        if !tool_calls.is_empty() {
            message["tool_calls"] = Value::Array(tool_calls);
        }

        let resp = json!({
            "id": call_id,
            "object": "chat.completion",
            "created": 1700000000,
            "model": payload.model,
            "choices": [{
                "index": 0,
                "message": message,
                "finish_reason": finish_reason
            }]
        });

        Json(resp).into_response()
    }
}

#[derive(Debug, Deserialize)]
struct TextCompletionRequest {
    model: String,
    prompt: Value,
    #[serde(default)]
    stream: bool,
    #[serde(default)]
    max_tokens: Option<u64>,
}

async fn text_completions(
    State(registry): State<TransportRegistry>,
    Json(payload): Json<TextCompletionRequest>,
) -> Response {
    let prompt_text = match payload.prompt {
        Value::String(s) => s,
        Value::Array(arr) => arr
            .into_iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    };

    let chat_req = ChatCompletionRequest {
        model: payload.model,
        messages: vec![json!({
            "role": "user",
            "content": prompt_text
        })],
        tools: None,
        reasoning_effort: None,
        thinking_budget: None,
        stream: payload.stream,
        _temperature: None,
        max_tokens: payload.max_tokens,
    };

    chat_completions(State(registry), Json(chat_req)).await
}

