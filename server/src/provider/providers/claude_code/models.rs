use crate::{Error, Result};
use serde::Serialize;
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ClaudeCodeModel {
    pub id: String,
    pub display_name: String,
    pub description: Option<String>,
    pub max_output_tokens: Option<u64>,
    pub images: bool,
}

pub fn claude_code_models() -> Vec<ClaudeCodeModel> {
    vec![
        ClaudeCodeModel {
            id: "claude-3-7-sonnet-20250219".into(),
            display_name: "Claude 3.7 Sonnet (Hybrid Reasoning)".into(),
            description: Some("Anthropic's flagship hybrid reasoning model".into()),
            max_output_tokens: Some(64000),
            images: true,
        },
        ClaudeCodeModel {
            id: "claude-3-5-sonnet-20241022".into(),
            display_name: "Claude 3.5 Sonnet".into(),
            description: Some("High-intelligence coding and analysis model".into()),
            max_output_tokens: Some(8192),
            images: true,
        },
        ClaudeCodeModel {
            id: "claude-3-5-haiku-20241022".into(),
            display_name: "Claude 3.5 Haiku".into(),
            description: Some("Fast, efficient lightweight model".into()),
            max_output_tokens: Some(8192),
            images: true,
        },
        ClaudeCodeModel {
            id: "claude-3-opus-20240229".into(),
            display_name: "Claude 3 Opus".into(),
            description: Some("Deep reasoning and complex task execution".into()),
            max_output_tokens: Some(4096),
            images: true,
        },
    ]
}

pub async fn fetch_models(
    client: &reqwest::Client,
    access_token: &str,
) -> Result<Vec<ClaudeCodeModel>> {
    let res = client
        .get("https://api.anthropic.com/v1/models")
        .header(
            reqwest::header::AUTHORIZATION,
            format!("Bearer {access_token}"),
        )
        .header("anthropic-version", "2023-06-01")
        .header("anthropic-beta", "claude-code-20250219,oauth-2025-04-20")
        .header(reqwest::header::ACCEPT, "application/json")
        .timeout(std::time::Duration::from_secs(8))
        .send()
        .await?;

    if !res.status().is_success() {
        return Err(Error::Provider(format!(
            "Anthropic models endpoint HTTP {}",
            res.status()
        )));
    }

    let body: Value = res.json().await?;
    let data = body
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::Provider("invalid models response from Anthropic".into()))?;

    let mut models = Vec::new();
    for item in data {
        if let Some(id) = item.get("id").and_then(Value::as_str) {
            let display_name = item
                .get("display_name")
                .and_then(Value::as_str)
                .unwrap_or(id)
                .to_string();
            models.push(ClaudeCodeModel {
                id: id.to_string(),
                display_name,
                description: None,
                max_output_tokens: Some(8192),
                images: true,
            });
        }
    }

    if models.is_empty() {
        return Ok(claude_code_models());
    }
    Ok(models)
}
