use serde::{Deserialize, Serialize};
use serde_json::Value;

/// OAuth tokens only work against the Grok CLI chat proxy; api.x.ai answers 403.
pub const CLI_MODELS_URL: &str = "https://cli-chat-proxy.grok.com/v1/models";
pub const LANGUAGE_MODELS_URL: &str = "https://api.x.ai/v1/language-models";
pub const MODELS_URL: &str = "https://api.x.ai/v1/models";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GrokModel {
    pub id: String,
    pub display_name: String,
    pub images: bool,
}

/// Only the model the CLI chat proxy actually serves; older `grok-2`/`grok-3`
/// aliases answer 404 for subscription (OAuth) accounts.
pub fn fallback_models() -> Vec<GrokModel> {
    vec![GrokModel {
        id: "grok-4.7".into(),
        display_name: "Grok 4.7".into(),
        images: true,
    }]
}

pub fn jwt_email(token: &str) -> Option<String> {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    let payload = token.split('.').nth(1)?;
    let payload = URL_SAFE_NO_PAD.decode(payload).ok()?;
    let payload: Value = serde_json::from_slice(&payload).ok()?;
    payload
        .get("email")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .or_else(|| {
            payload
                .get("preferred_username")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
        })
}

pub async fn fetch_models(
    client: &reqwest::Client,
    access_token: &str,
) -> Result<Vec<GrokModel>, String> {
    for url in [CLI_MODELS_URL, LANGUAGE_MODELS_URL, MODELS_URL] {
        let response = client
            .get(url)
            .header("accept", "application/json")
            .bearer_auth(access_token)
            .send()
            .await
            .map_err(|error| format!("Grok models request failed: {error}"))?;
        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|error| format!("Grok models body read failed: {error}"))?;
        if !status.is_success() {
            continue;
        }
        let body: Value = serde_json::from_str(&text)
            .map_err(|error| format!("Grok models JSON parse failed: {error}"))?;
        if let Ok(models) = parse_grok_models(&body) {
            if !models.is_empty() {
                return Ok(models);
            }
        }
    }
    Err("Grok model endpoints returned no models".into())
}

fn format_display_name(id: &str) -> String {
    id.split('-')
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                None => String::new(),
                Some(first) if first.is_ascii_digit() => part.to_string(),
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn parse_grok_models(body: &Value) -> Result<Vec<GrokModel>, String> {
    let source = body
        .get("models")
        .or_else(|| body.get("data"))
        .and_then(Value::as_array)
        .or_else(|| body.as_array())
        .ok_or_else(|| "Grok model response does not contain a model list".to_string())?;

    let mut models = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for item in source {
        let id = item
            .get("id")
            .or_else(|| item.get("name"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty());

        let Some(id) = id else { continue };
        if !seen.insert(id.to_string()) {
            continue;
        }

        let has_image = item
            .get("input_modalities")
            .or_else(|| item.get("inputModalities"))
            .and_then(Value::as_array)
            .map(|arr| {
                arr.is_empty()
                    || arr.iter().any(|m| {
                        m.as_str()
                            .map(|s| s.eq_ignore_ascii_case("image"))
                            .unwrap_or(false)
                    })
            })
            .unwrap_or(true);

        models.push(GrokModel {
            id: id.to_string(),
            display_name: format_display_name(id),
            images: has_image,
        });
    }

    Ok(models)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_models_or_uses_fallbacks() {
        let json = serde_json::json!({
            "models": [
                { "id": "grok-4-fast", "input_modalities": ["text", "image"] }
            ]
        });
        let parsed = parse_grok_models(&json).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].id, "grok-4-fast");
        assert_eq!(parsed[0].display_name, "Grok 4 Fast");
        assert!(parsed[0].images);
    }
}
