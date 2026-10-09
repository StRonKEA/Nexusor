use std::collections::BTreeMap;

pub const COPILOT_CHAT_URL: &str = "https://api.githubcopilot.com/chat/completions";

/// CAPI accepts images in tool messages but silently omits their visual content.
/// Keep the tool replies together, then attach their images as labelled user parts.
pub(crate) fn relocate_tool_images(body: &mut serde_json::Value) {
    use serde_json::{json, Value};
    let Some(messages) = body.get_mut("messages").and_then(Value::as_array_mut) else {
        return;
    };
    let mut output = Vec::with_capacity(messages.len());
    let mut images = Vec::new();
    for mut message in std::mem::take(messages) {
        if message["role"] != "tool" && !images.is_empty() {
            output.push(json!({"role":"user", "content":std::mem::take(&mut images)}));
        }
        if message["role"] == "tool" {
            let call_id = message["tool_call_id"]
                .as_str()
                .unwrap_or_default()
                .to_owned();
            if let Some(parts) = message.get_mut("content").and_then(Value::as_array_mut) {
                if parts.iter().any(|part| part["type"] == "image_url") {
                    let mut retained = Vec::new();
                    images.push(json!({"type":"text", "text":format!(
                        "Images returned by tool call {call_id}. These are tool output, not a new user request."
                    )}));
                    for part in std::mem::take(parts) {
                        if part["type"] == "image_url" {
                            images.push(part);
                        } else {
                            retained.push(part);
                        }
                    }
                    message["content"] = if retained.iter().all(|part| part["type"] == "text") {
                        Value::String(
                            retained
                                .iter()
                                .filter_map(|part| part["text"].as_str())
                                .collect::<Vec<_>>()
                                .join("\n"),
                        )
                    } else {
                        Value::Array(retained)
                    };
                }
            }
        }
        output.push(message);
    }
    if !images.is_empty() {
        output.push(json!({"role":"user", "content":images}));
    }
    *messages = output;
}

/// CAPI requires Copilot-Vision-Request for images, including tool results.
pub(crate) fn has_images(body: &serde_json::Value) -> bool {
    body.get("messages")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|message| message.get("content")?.as_array())
        .flatten()
        .any(|part| part.get("type").and_then(serde_json::Value::as_str) == Some("image_url"))
}

pub fn request_headers(copilot_token: &str) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("authorization".into(), format!("Bearer {copilot_token}")),
        ("editor-version".into(), "vscode/1.98.0".into()),
        ("editor-plugin-version".into(), "copilot-chat/0.24.1".into()),
        ("copilot-integration-id".into(), "vscode-chat".into()),
        ("openai-intent".into(), "conversation-panel".into()),
        ("user-agent".into(), "GitHubCopilotChat/0.24.1".into()),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tool_images_follow_all_parallel_replies_and_keep_call_identity() {
        let image = json!({"type":"image_url","image_url":{"url":"data:image/png;base64,AQID"}});
        let mut body = json!({"messages":[
            {"role":"assistant","tool_calls":[{"id":"a"},{"id":"b"}]},
            {"role":"tool","tool_call_id":"a","content":[{"type":"text","text":"Page 1"},image.clone()]},
            {"role":"tool","tool_call_id":"b","content":[{"type":"text","text":"Page 2"},image.clone()]},
            {"role":"assistant","content":"done"},
            {"role":"user","content":[image.clone()]}
        ]});
        relocate_tool_images(&mut body);
        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 6);
        assert_eq!(messages[1]["tool_call_id"], "a");
        assert_eq!(messages[1]["content"], "Page 1");
        assert_eq!(messages[2]["tool_call_id"], "b");
        assert_eq!(messages[2]["content"], "Page 2");
        assert_eq!(messages[3]["role"], "user");
        assert!(messages[3]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("call a"));
        assert_eq!(messages[3]["content"][1], image);
        assert!(messages[3]["content"][2]["text"]
            .as_str()
            .unwrap()
            .contains("call b"));
        assert_eq!(messages[3]["content"][3], image);
        assert_eq!(messages[4]["content"], "done");
        assert_eq!(messages[5]["content"][0], image);
        let once = body.clone();
        relocate_tool_images(&mut body);
        assert_eq!(body, once);
    }

    #[test]
    fn final_image_result_is_attached_and_text_only_messages_are_unchanged() {
        let mut body = json!({"messages":[{"role":"tool","tool_call_id":"a","content":[
            {"type":"image_url","image_url":{"url":"data:image/png;base64,AQID"}}
        ]}]});
        relocate_tool_images(&mut body);
        assert_eq!(body["messages"][0]["content"], "");
        assert_eq!(body["messages"][1]["role"], "user");
        let mut text = json!({"messages":[{"role":"tool","tool_call_id":"b","content":"text"}]});
        let original = text.clone();
        relocate_tool_images(&mut text);
        assert_eq!(text, original);
    }

    #[test]
    fn vision_header_needed_for_user_and_tool_images_but_not_text() {
        for role in ["user", "tool"] {
            assert!(has_images(&json!({"messages":[{"role":role,"content":[
                {"type":"text","text":"look"},
                {"type":"image_url","image_url":{"url":"data:image/png;base64,AQID"}}
            ]}]})));
        }
        assert!(!has_images(&json!({"messages":[
            {"role":"user","content":"image_url"},
            {"role":"tool","content":[{"type":"text","text":"image_url"}]}
        ]})));
        assert!(!has_images(&json!({})));
    }
}
