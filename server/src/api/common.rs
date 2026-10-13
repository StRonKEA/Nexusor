//! Shared helpers for OpenAI and Claude compatibility mapping.

use crate::model::{
    ContentPart, ProjectedContent, ProjectedMessage, Role, ToolCallContent, ToolDefinition,
    ToolResultContent,
};
use serde_json::Value;

/// Maps a thinking budget in tokens to discrete reasoning effort level.
/// Matches CLIProxyAPI's `internal/thinking/convert.go` threshold mapping.
pub fn budget_to_reasoning_effort(budget: i64) -> Option<String> {
    if budget <= 0 {
        return None;
    }
    let effort = match budget {
        1..=1024 => "low",
        1025..=8192 => "medium",
        _ => "high",
    };
    Some(effort.to_string())
}

/// Parses an OpenAI tool definition list.
pub fn parse_openai_tools(tools_val: Option<&Value>) -> Vec<ToolDefinition> {
    let mut tools = Vec::new();
    let Some(Value::Array(items)) = tools_val else {
        return tools;
    };

    for item in items {
        if item.get("type").and_then(|v| v.as_str()) == Some("function") {
            if let Some(func) = item.get("function") {
                let name = func
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                let description = func
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                let parameters = func
                    .get("parameters")
                    .cloned()
                    .unwrap_or_else(|| serde_json::json!({"type": "object", "properties": {}}));
                if !name.is_empty() {
                    tools.push(ToolDefinition {
                        name,
                        description,
                        parameters,
                    });
                }
            }
        }
    }
    tools
}

/// Parses Claude tools declaration list.
pub fn parse_claude_tools(tools_val: Option<&Value>) -> Vec<ToolDefinition> {
    let mut tools = Vec::new();
    let Some(Value::Array(items)) = tools_val else {
        return tools;
    };

    for item in items {
        let name = item
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let description = item
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let parameters = item
            .get("input_schema")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({"type": "object", "properties": {}}));
        if !name.is_empty() {
            tools.push(ToolDefinition {
                name,
                description,
                parameters,
            });
        }
    }
    tools
}

/// Parses Claude messages array preserving tool_use and tool_result blocks.
pub fn parse_claude_messages(messages_val: &[Value]) -> Vec<ProjectedMessage> {
    let mut history = Vec::new();

    for msg in messages_val {
        let role_str = msg.get("role").and_then(|v| v.as_str()).unwrap_or("user");
        let content_val = msg.get("content");

        match role_str {
            "assistant" => {
                let mut text = String::new();
                let mut calls = Vec::new();

                if let Some(Value::String(s)) = content_val {
                    text = s.clone();
                } else if let Some(Value::Array(items)) = content_val {
                    for (idx, item) in items.iter().enumerate() {
                        let block_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
                        if block_type == "text" {
                            if let Some(t) = item.get("text").and_then(|v| v.as_str()) {
                                text.push_str(t);
                            }
                        } else if block_type == "tool_use" {
                            let call_id = item
                                .get("id")
                                .and_then(|v| v.as_str())
                                .unwrap_or_default()
                                .to_string();
                            let name = item
                                .get("name")
                                .and_then(|v| v.as_str())
                                .unwrap_or_default()
                                .to_string();
                            let arguments = item
                                .get("input")
                                .cloned()
                                .unwrap_or_else(|| serde_json::json!({}));
                            calls.push(ToolCallContent {
                                index: idx,
                                call_id,
                                name,
                                arguments,
                            });
                        }
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
            _ => {
                // user role (can contain normal text, images, or tool_results)
                if let Some(Value::String(s)) = content_val {
                    history.push(ProjectedMessage {
                        message_id: uuid::Uuid::new_v4().to_string(),
                        role: Role::User,
                        content: ProjectedContent::Parts(vec![ContentPart::Text { text: s.clone() }]),
                    });
                } else if let Some(Value::Array(items)) = content_val {
                    let mut parts = Vec::new();

                    for item in items {
                        let block_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
                        if block_type == "text" {
                            if let Some(t) = item.get("text").and_then(|v| v.as_str()) {
                                parts.push(ContentPart::Text {
                                    text: t.to_string(),
                                });
                            }
                        } else if block_type == "image" {
                            if let Some(source) = item.get("source") {
                                if source.get("type").and_then(|v| v.as_str()) == Some("base64") {
                                    let mime = source.get("media_type").and_then(|v| v.as_str()).unwrap_or("image/png");
                                    let b64 = source.get("data").and_then(|v| v.as_str()).unwrap_or_default();
                                    use base64::{engine::general_purpose::STANDARD, Engine};
                                    if let Ok(data) = STANDARD.decode(b64.trim()) {
                                        parts.push(ContentPart::Image {
                                            mime_type: mime.to_string(),
                                            data,
                                        });
                                    }
                                }
                            }
                        } else if block_type == "tool_result" {
                            let call_id = item
                                .get("tool_use_id")
                                .and_then(|v| v.as_str())
                                .unwrap_or_default()
                                .to_string();
                            let is_error = item
                                .get("is_error")
                                .and_then(|v| v.as_bool())
                                .unwrap_or(false);

                            let content_text = match item.get("content") {
                                Some(Value::String(s)) => s.clone(),
                                Some(Value::Array(arr)) => {
                                    let mut texts = Vec::new();
                                    for sub in arr {
                                        if let Some(t) = sub.get("text").and_then(|v| v.as_str()) {
                                            texts.push(t);
                                        }
                                    }
                                    texts.join("\n")
                                }
                                _ => String::new(),
                            };

                            history.push(ProjectedMessage {
                                message_id: uuid::Uuid::new_v4().to_string(),
                                role: Role::Tool,
                                content: ProjectedContent::ToolResult(ToolResultContent {
                                    call_id,
                                    name: String::new(),
                                    content: content_text,
                                    is_error,
                                    image: None,
                                    images: Vec::new(),
                                    provider_parts: Vec::new(),
                                }),
                            });
                        }
                    }

                    if !parts.is_empty() {
                        history.push(ProjectedMessage {
                            message_id: uuid::Uuid::new_v4().to_string(),
                            role: Role::User,
                            content: ProjectedContent::Parts(parts),
                        });
                    }
                }
            }
        }
    }

    history
}
