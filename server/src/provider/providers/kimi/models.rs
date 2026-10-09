use crate::{Error, Result};
use serde::Serialize;
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct KimiModel {
    pub id: String,
    pub display_name: String,
    pub description: Option<String>,
    pub max_output_tokens: Option<u64>,
    pub images: bool,
}

pub fn kimi_models() -> Vec<KimiModel> {
    vec![
        KimiModel {
            id: "kimi-for-coding".into(),
            display_name: "Kimi for Coding (1M Context)".into(),
            description: Some("Moonshot Kimi coding model with 1M context and reasoning".into()),
            max_output_tokens: Some(32768),
            images: true,
        },
        KimiModel {
            id: "kimi-k3".into(),
            display_name: "Kimi K3 (Reasoning)".into(),
            description: Some("Kimi K3 thinking and coding model".into()),
            max_output_tokens: Some(32768),
            images: true,
        },
        KimiModel {
            id: "kimi-k2.5".into(),
            display_name: "Kimi K2.5".into(),
            description: Some("Moonshot Kimi K2.5 fast model".into()),
            max_output_tokens: Some(16384),
            images: true,
        },
    ]
}

pub async fn fetch_models(client: &reqwest::Client, access_token: &str) -> Result<Vec<KimiModel>> {
    let res = client
        .get("https://api.kimi.com/coding/v1/models")
        .header(
            reqwest::header::AUTHORIZATION,
            format!("Bearer {access_token}"),
        )
        .header(reqwest::header::ACCEPT, "application/json")
        .timeout(std::time::Duration::from_secs(8))
        .send()
        .await?;

    if !res.status().is_success() {
        return Err(Error::Provider(format!(
            "Kimi models endpoint HTTP {}",
            res.status()
        )));
    }

    let body: Value = res.json().await?;
    let data = body
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::Provider("invalid models response from Kimi".into()))?;

    let mut models = Vec::new();
    for item in data {
        if let Some(id) = item.get("id").and_then(Value::as_str) {
            let name = item.get("name").and_then(Value::as_str).unwrap_or(id);
            let display_name = name.to_string();
            models.push(KimiModel {
                id: id.to_string(),
                display_name,
                description: item
                    .get("description")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
                max_output_tokens: Some(32768),
                images: true,
            });
        }
    }

    if models.is_empty() {
        return Ok(kimi_models());
    }
    Ok(models)
}
