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
    cursor::transport::TransportRegistry,
    model::{
        ContentPart, ModelInvocation, ModelRequest, ModelSpec, ProjectedContent,
        ProjectedMessage, PromptSpec, Role,
    },
    provider::{FinishReason, ModelEvent},
};

pub fn router(registry: TransportRegistry) -> Router {
    Router::new()
        .route("/v1/messages", post(claude_messages))
        .with_state(registry)
}

#[derive(Debug, Deserialize)]
struct ClaudeMessageItem {
    role: String,
    content: Value,
}

#[derive(Debug, Deserialize)]
struct ClaudeMessagesRequest {
    model: String,
    messages: Vec<ClaudeMessageItem>,
    #[serde(default)]
    system: Option<Value>,
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

    let mut history = Vec::new();
    for msg in payload.messages {
        let role = match msg.role.as_str() {
            "assistant" => Role::Assistant,
            _ => Role::User,
        };

        let text = match msg.content {
            Value::String(s) => s,
            Value::Array(arr) => {
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
            role,
            content: ProjectedContent::Parts(vec![ContentPart::Text { text }]),
        });
    }

    let mut model_spec = ModelSpec::new(&payload.model);
    model_spec.max_output_tokens = payload.max_tokens.or(Some(4096));

    let invocation = ModelInvocation {
        call_id: call_id.clone(),
        run_id,
        conversation_id,
        provider_call_index: 1,
        request: ModelRequest {
            prompt: PromptSpec {
                instructions,
                tools: Vec::new(),
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

            // 2. content_block_start
            let block_start = json!({
                "type": "content_block_start",
                "index": 0,
                "content_block": {
                    "type": "text",
                    "text": ""
                }
            });
            yield Ok::<_, Infallible>(format!("event: content_block_start\ndata: {}\n\n", serde_json::to_string(&block_start).unwrap()));

            let mut output_tokens = 0;
            while let Some(item) = event_stream.next().await {
                match item {
                    Ok(ModelEvent::TextDelta(text)) => {
                        output_tokens += 1;
                        let delta = json!({
                            "type": "content_block_delta",
                            "index": 0,
                            "delta": {
                                "type": "text_delta",
                                "text": text
                            }
                        });
                        yield Ok::<_, Infallible>(format!("event: content_block_delta\ndata: {}\n\n", serde_json::to_string(&delta).unwrap()));
                    }
                    Ok(ModelEvent::Done(finish)) => {
                        let stop_reason = match finish {
                            FinishReason::ToolUse => "tool_use",
                            FinishReason::Length => "max_tokens",
                            FinishReason::Stop => "end_turn",
                        };

                        // 3. content_block_stop
                        let block_stop = json!({
                            "type": "content_block_stop",
                            "index": 0
                        });
                        yield Ok::<_, Infallible>(format!("event: content_block_stop\ndata: {}\n\n", serde_json::to_string(&block_stop).unwrap()));

                        // 4. message_delta
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

                        // 5. message_stop
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
        let mut stop_reason = "end_turn".to_string();

        while let Some(item) = event_stream.next().await {
            match item {
                Ok(ModelEvent::TextDelta(text)) => {
                    full_text.push_str(&text);
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

        let resp = json!({
            "id": call_id,
            "type": "message",
            "role": "assistant",
            "content": [{
                "type": "text",
                "text": full_text
            }],
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
