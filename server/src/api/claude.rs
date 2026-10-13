//! Anthropic Claude compatible API route (/v1/messages).

use std::convert::Infallible;

use async_stream::stream;
use axum::{
    body::Body,
    extract::State,
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

use crate::{
    api::common::{budget_to_reasoning_effort, parse_claude_messages, parse_claude_tools},
    cursor::transport::TransportRegistry,
    model::{ModelInvocation, ModelRequest, ModelSpec, PromptSpec},
    provider::{FinishReason, ModelEvent},
};

pub fn router(registry: TransportRegistry) -> Router {
    Router::new()
        .route("/messages", post(claude_messages))
        .route("/messages/count_tokens", post(claude_count_tokens))
        .route("/v1/messages", post(claude_messages))
        .route("/v1/messages/count_tokens", post(claude_count_tokens))
        .with_state(registry)
}

#[derive(Debug, Deserialize)]
struct ClaudeMessagesRequest {
    model: String,
    messages: Vec<Value>,
    #[serde(default)]
    system: Option<Value>,
    #[serde(default)]
    tools: Option<Value>,
    #[serde(default)]
    thinking: Option<Value>,
    #[serde(default)]
    max_tokens: Option<u64>,
    #[serde(default)]
    stream: bool,
}

async fn claude_messages(
    State(registry): State<TransportRegistry>,
    Json(payload): Json<ClaudeMessagesRequest>,
) -> Response {
    let call_id = format!("msg_{}", uuid::Uuid::new_v4().simple());
    let run_id = format!("nexusor-claude-run-{}", uuid::Uuid::new_v4());
    let conversation_id = format!("nexusor-claude-conv-{}", uuid::Uuid::new_v4());

    let mut instructions = String::new();
    if let Some(sys) = payload.system {
        if let Some(s) = sys.as_str() {
            instructions = s.to_string();
        } else if let Some(arr) = sys.as_array() {
            let mut parts = Vec::new();
            for item in arr {
                if let Some(t) = item.get("text").and_then(|v| v.as_str()) {
                    parts.push(t.to_string());
                }
            }
            instructions = parts.join("\n\n");
        }
    }

    let tools = parse_claude_tools(payload.tools.as_ref());
    let history = parse_claude_messages(&payload.messages);

    // Normalize model ID and extract reasoning effort suffix if present (e.g., -high, -thinking)
    let (base_model, extracted_effort) = crate::api::common::normalize_model_id_and_effort(&payload.model);
    let resolved_model = crate::api::common::resolve_model_fallback(&registry, &base_model).await;

    let mut model_spec = ModelSpec::new(&resolved_model);
    model_spec.max_output_tokens = payload.max_tokens.or(Some(4096));

    // Thinking budget mapping from CLIProxyAPI
    if let Some(th) = payload.thinking {
        if th.get("type").and_then(|v| v.as_str()) == Some("enabled") {
            let budget = th.get("budget_tokens").and_then(|v| v.as_i64()).unwrap_or(-1);
            if let Some(effort) = budget_to_reasoning_effort(budget) {
                model_spec.reasoning.effort = Some(effort);
                model_spec.reasoning.enabled = true;
            }
        }
    } else if let Some(effort) = extracted_effort {
        model_spec.reasoning.effort = Some(effort);
        model_spec.reasoning.enabled = true;
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
            // 1. message_start
            let msg_start = json!({
                "type": "message_start",
                "message": {
                    "id": &call_id,
                    "type": "message",
                    "role": "assistant",
                    "content": [],
                    "model": &model,
                    "stop_reason": null,
                    "stop_sequence": null,
                    "usage": {
                        "input_tokens": 0,
                        "output_tokens": 0
                    }
                }
            });
            yield Ok::<_, Infallible>(format!("event: message_start\ndata: {}\n\n", serde_json::to_string(&msg_start).unwrap()));

            let mut block_index = 0;
            let mut text_block_open = false;
            let mut output_tokens = 0;

            while let Some(item) = event_stream.next().await {
                match item {
                    Ok(ModelEvent::TextDelta(text)) => {
                        output_tokens += 1;
                        if !text_block_open {
                            text_block_open = true;
                            let block_start = json!({
                                "type": "content_block_start",
                                "index": block_index,
                                "content_block": {
                                    "type": "text",
                                    "text": ""
                                }
                            });
                            yield Ok::<_, Infallible>(format!("event: content_block_start\ndata: {}\n\n", serde_json::to_string(&block_start).unwrap()));
                        }
                        let delta = json!({
                            "type": "content_block_delta",
                            "index": block_index,
                            "delta": {
                                "type": "text_delta",
                                "text": text
                            }
                        });
                        yield Ok::<_, Infallible>(format!("event: content_block_delta\ndata: {}\n\n", serde_json::to_string(&delta).unwrap()));
                    }
                    Ok(ModelEvent::ThinkingDelta(thinking_text)) => {
                        output_tokens += 1;
                        let delta = json!({
                            "type": "content_block_delta",
                            "index": block_index,
                            "delta": {
                                "type": "thinking_delta",
                                "thinking": thinking_text
                            }
                        });
                        yield Ok::<_, Infallible>(format!("event: content_block_delta\ndata: {}\n\n", serde_json::to_string(&delta).unwrap()));
                    }
                    Ok(ModelEvent::ToolCallStart { index: _, call_id: tc_id, name }) => {
                        if text_block_open {
                            text_block_open = false;
                            let block_stop = json!({
                                "type": "content_block_stop",
                                "index": block_index
                            });
                            yield Ok::<_, Infallible>(format!("event: content_block_stop\ndata: {}\n\n", serde_json::to_string(&block_stop).unwrap()));
                            block_index += 1;
                        }

                        let tool_block_start = json!({
                            "type": "content_block_start",
                            "index": block_index,
                            "content_block": {
                                "type": "tool_use",
                                "id": tc_id,
                                "name": name,
                                "input": {}
                            }
                        });
                        yield Ok::<_, Infallible>(format!("event: content_block_start\ndata: {}\n\n", serde_json::to_string(&tool_block_start).unwrap()));
                    }
                    Ok(ModelEvent::ToolCallArgumentsDelta { index: _, delta }) => {
                        let arg_delta = json!({
                            "type": "content_block_delta",
                            "index": block_index,
                            "delta": {
                                "type": "input_json_delta",
                                "partial_json": delta
                            }
                        });
                        yield Ok::<_, Infallible>(format!("event: content_block_delta\ndata: {}\n\n", serde_json::to_string(&arg_delta).unwrap()));
                    }
                    Ok(ModelEvent::ToolCallEnd { .. }) => {
                        let block_stop = json!({
                            "type": "content_block_stop",
                            "index": block_index
                        });
                        yield Ok::<_, Infallible>(format!("event: content_block_stop\ndata: {}\n\n", serde_json::to_string(&block_stop).unwrap()));
                        block_index += 1;
                    }
                    Ok(ModelEvent::Done(finish)) => {
                        if text_block_open {
                            let block_stop = json!({
                                "type": "content_block_stop",
                                "index": block_index
                            });
                            yield Ok::<_, Infallible>(format!("event: content_block_stop\ndata: {}\n\n", serde_json::to_string(&block_stop).unwrap()));
                        }

                        let stop_reason = match finish {
                            FinishReason::ToolUse => "tool_use",
                            FinishReason::Length => "max_tokens",
                            FinishReason::Stop => "end_turn",
                        };

                        let msg_delta = json!({
                            "type": "message_delta",
                            "delta": {
                                "stop_reason": stop_reason,
                                "stop_sequence": null
                            },
                            "usage": {
                                "output_tokens": output_tokens
                            }
                        });
                        yield Ok::<_, Infallible>(format!("event: message_delta\ndata: {}\n\n", serde_json::to_string(&msg_delta).unwrap()));

                        let msg_stop = json!({
                            "type": "message_stop"
                        });
                        yield Ok::<_, Infallible>(format!("event: message_stop\ndata: {}\n\n", serde_json::to_string(&msg_stop).unwrap()));
                        break;
                    }
                    Err(err) => {
                        let err_event = json!({
                            "type": "error",
                            "error": {
                                "type": "api_error",
                                "message": err.to_string()
                            }
                        });
                        yield Ok::<_, Infallible>(format!("event: error\ndata: {}\n\n", serde_json::to_string(&err_event).unwrap()));
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
        let mut content_blocks = Vec::new();
        let mut tool_calls = Vec::new();
        let mut stop_reason = "end_turn".to_string();

        while let Some(item) = event_stream.next().await {
            match item {
                Ok(ModelEvent::TextDelta(text)) => {
                    full_text.push_str(&text);
                }
                Ok(ModelEvent::ToolCallStart { index: _, call_id: tc_id, name }) => {
                    tool_calls.push((tc_id, name, String::new()));
                }
                Ok(ModelEvent::ToolCallArgumentsDelta { index, delta }) => {
                    if let Some(call) = tool_calls.get_mut(index) {
                        call.2.push_str(&delta);
                    }
                }
                Ok(ModelEvent::Done(finish)) => {
                    stop_reason = match finish {
                        FinishReason::ToolUse => "tool_use",
                        FinishReason::Length => "max_tokens",
                        FinishReason::Stop => "end_turn",
                    }.to_string();
                    break;
                }
                Err(err) => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({
                            "type": "error",
                            "error": {
                                "type": "api_error",
                                "message": err.to_string()
                            }
                        })),
                    ).into_response();
                }
                _ => {}
            }
        }

        if !full_text.is_empty() {
            content_blocks.push(json!({
                "type": "text",
                "text": full_text
            }));
        }

        for (tc_id, name, args_str) in tool_calls {
            let input: Value = serde_json::from_str(&args_str).unwrap_or_else(|_| json!({}));
            content_blocks.push(json!({
                "type": "tool_use",
                "id": tc_id,
                "name": name,
                "input": input
            }));
        }

        let resp = json!({
            "id": call_id,
            "type": "message",
            "role": "assistant",
            "content": content_blocks,
            "model": payload.model,
            "stop_reason": stop_reason,
            "stop_sequence": null,
            "usage": {
                "input_tokens": 0,
                "output_tokens": 0
            }
        });

        Json(resp).into_response()
    }
}

async fn claude_count_tokens(
    Json(payload): Json<ClaudeMessagesRequest>,
) -> Response {
    let history = parse_claude_messages(&payload.messages);
    let mut total_chars = 0;
    if let Some(sys) = payload.system {
        if let Some(s) = sys.as_str() {
            total_chars += s.len();
        }
    }
    for msg in history {
        match msg.content {
            crate::model::ProjectedContent::Parts(parts) => {
                for p in parts {
                    if let crate::model::ContentPart::Text { text } = p {
                        total_chars += text.len();
                    }
                }
            }
            crate::model::ProjectedContent::Assistant { text, .. } => {
                total_chars += text.len();
            }
            crate::model::ProjectedContent::ToolResult(res) => {
                total_chars += res.content.len();
            }
        }
    }

    // Standard rule of thumb: ~4 characters per token
    let estimated_tokens = (total_chars / 4).max(1);

    Json(json!({
        "input_tokens": estimated_tokens
    }))
    .into_response()
}

