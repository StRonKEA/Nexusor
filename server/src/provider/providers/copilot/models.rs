use crate::{Error, Result};
use serde::Serialize;
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CopilotModel {
    pub id: String,
    pub display_name: String,
    pub description: Option<String>,
    pub max_output_tokens: Option<u64>,
    pub images: bool,
}

/// These models do not accept reasoning_effort retained in older Cursor chats.
pub fn rejects_reasoning_effort(model_id: &str) -> bool {
    matches!(
        model_id,
        "gpt-4.1"
            | "gpt-4o"
            | "gpt-4o-mini"
            | "gpt-4o-2024-05-13"
            | "gpt-4o-2024-08-06"
            | "gpt-4o-2024-11-20"
            | "gpt-4o-mini-2024-07-18"
    )
}

pub fn copilot_models() -> Vec<CopilotModel> {
    vec![
        CopilotModel {
            id: "claude-3.7-sonnet".into(),
            display_name: "Claude 3.7 Sonnet (Copilot)".into(),
            description: Some("Anthropic Claude 3.7 Sonnet via GitHub Copilot".into()),
            max_output_tokens: Some(8192),
            images: true,
        },
        CopilotModel {
            id: "claude-3.5-sonnet".into(),
            display_name: "Claude 3.5 Sonnet (Copilot)".into(),
            description: Some("Anthropic Claude 3.5 Sonnet via GitHub Copilot".into()),
            max_output_tokens: Some(8192),
            images: true,
        },
        CopilotModel {
            id: "gpt-4o".into(),
            display_name: "GPT-4o (Copilot)".into(),
            description: Some("OpenAI GPT-4o via GitHub Copilot".into()),
            max_output_tokens: Some(4096),
            images: true,
        },
        CopilotModel {
            id: "o3-mini".into(),
            display_name: "o3-mini (Copilot)".into(),
            description: Some("OpenAI o3-mini reasoning model via GitHub Copilot".into()),
            max_output_tokens: Some(16384),
            images: false,
        },
        CopilotModel {
            id: "o1".into(),
            display_name: "o1 (Copilot)".into(),
            description: Some("OpenAI o1 reasoning model via GitHub Copilot".into()),
            max_output_tokens: Some(16384),
            images: false,
        },
    ]
}

fn is_valid_copilot_chat_model(id: &str) -> bool {
    if id == "gpt-4.1" {
        return true;
    }
    let lower = id.to_lowercase();
    if lower.contains("embedding")
        || lower.contains("compaction")
        || lower.contains("search-")
        || lower.contains("exec-agent")
        || lower.starts_with("gpt-5")
        || lower.starts_with("gpt-6")
        || lower.starts_with("mai-code")
        || lower.starts_with("kimi-")
        || lower.contains("fable")
        || lower.contains("opus-5")
        || lower.contains("sonnet-5")
        || lower.contains("opus-4")
        || lower.contains("4.1")
        || lower.contains("haiku-4.5")
    {
        return false;
    }
    true
}

pub async fn fetch_models(
    client: &reqwest::Client,
    copilot_token: &str,
) -> Result<Vec<CopilotModel>> {
    let mut req = client
        .get("https://api.githubcopilot.com/models")
        .timeout(std::time::Duration::from_secs(8));
    for (k, v) in super::provider::request_headers(copilot_token) {
        req = req.header(k, v);
    }
    let res = req.send().await?;
    if !res.status().is_success() {
        return Err(Error::Provider(format!(
            "Copilot models endpoint HTTP {}",
            res.status()
        )));
    }
    let body: Value = res.json().await?;
    let data = body
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::Provider("invalid models response from Copilot".into()))?;

    let mut models = Vec::new();
    for item in data {
        if let Some(id) = item.get("id").and_then(Value::as_str) {
            if !is_valid_copilot_chat_model(id) {
                continue;
            }
            let name = item.get("name").and_then(Value::as_str).unwrap_or(id);
            let display_name = format!("{name} (Copilot)");
            models.push(CopilotModel {
                id: id.to_string(),
                display_name,
                description: item
                    .get("description")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
                max_output_tokens: item
                    .get("capabilities")
                    .and_then(|c| c.get("limits"))
                    .and_then(|l| l.get("max_output_tokens"))
                    .and_then(Value::as_u64)
                    .or(Some(8192)),
                images: item
                    .pointer("/capabilities/supports/vision")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            });
        }
    }
    if models.is_empty() {
        return Ok(copilot_models());
    }
    Ok(models)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verified_gpt41_chat_model_is_not_hidden_by_legacy_filter() {
        assert!(is_valid_copilot_chat_model("gpt-4.1"));
        assert!(rejects_reasoning_effort("gpt-4.1"));
        assert!(is_valid_copilot_chat_model("gpt-4o"));
        assert!(!is_valid_copilot_chat_model("text-embedding-3-small"));
    }
}
