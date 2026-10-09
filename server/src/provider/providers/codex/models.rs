use serde::Serialize;
use serde_json::Value;

pub const MODELS_URL: &str = "https://chatgpt.com/backend-api/codex/models?client_version=0.144.1";

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CodexModel {
    pub id: String,
    pub display_name: String,
    pub description: Option<String>,
    pub max_output_tokens: Option<u64>,
    pub reasoning_efforts: Vec<String>,
    pub images: bool,
}

fn decode_jwt_payload(token: &str) -> Option<Value> {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    let payload = token.split('.').nth(1)?;
    let payload = URL_SAFE_NO_PAD.decode(payload).ok()?;
    serde_json::from_slice(&payload).ok()
}

pub fn chat_gpt_account_id(token: &str) -> Option<String> {
    let payload = decode_jwt_payload(token)?;
    payload
        .get("https://api.openai.com/auth")
        .and_then(Value::as_object)
        .and_then(|auth| auth.get("chatgpt_account_id"))
        .and_then(Value::as_str)
        .or_else(|| payload.get("chatgpt_account_id").and_then(Value::as_str))
        .map(str::to_owned)
}

pub fn jwt_email(token: &str) -> Option<String> {
    let payload = decode_jwt_payload(token)?;
    text(payload.get("email"))
        .or_else(|| text(payload.get("preferred_username")))
        .or_else(|| {
            payload
                .get("https://api.openai.com/profile")
                .and_then(Value::as_object)
                .and_then(|profile| text(profile.get("email")))
        })
        .or_else(|| text(payload.get("name")).filter(|value| value.contains('@')))
}

pub async fn fetch_models(
    client: &reqwest::Client,
    access_token: &str,
    account_id: Option<&str>,
) -> Result<Vec<CodexModel>, String> {
    let mut request = client
        .get(MODELS_URL)
        .header("accept", "application/json")
        .header("authorization", format!("Bearer {access_token}"))
        .header("originator", "codex_cli_rs")
        .header("user-agent", "codex_cli_rs/0.144.1 (Windows 10; x86_64)");
    if let Some(account_id) = account_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .or_else(|| chat_gpt_account_id(access_token))
    {
        request = request.header("Chatgpt-Account-Id", account_id);
    }

    let response = request
        .send()
        .await
        .map_err(|error| format!("Codex models request failed: {error}"))?;
    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|error| format!("Codex models body read failed: {error}"))?;
    if !status.is_success() {
        return Err(format!("Codex models HTTP {status}: {text}"));
    }
    let body: Value = serde_json::from_str(&text)
        .map_err(|error| format!("Codex models JSON parse failed: {error}"))?;
    parse_official_models(&body)
}

fn object(value: &Value) -> Option<&serde_json::Map<String, Value>> {
    value.as_object()
}

fn text(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn model_id(value: &Value) -> Option<String> {
    text(Some(value)).or_else(|| {
        object(value).and_then(|model| {
            ["slug", "id", "model", "name"]
                .iter()
                .find_map(|key| text(model.get(*key)))
        })
    })
}

fn positive_integer(value: Option<&Value>) -> Option<u64> {
    value
        .and_then(|value| value.as_u64().or_else(|| value.as_str()?.parse().ok()))
        .filter(|value| *value > 0)
}

fn reasoning_efforts(model: &serde_json::Map<String, Value>) -> Vec<String> {
    let source = [
        "supported_reasoning_levels",
        "supportedReasoningLevels",
        "reasoning_levels",
        "reasoningLevels",
        "supported_reasoning_efforts",
        "supportedReasoningEfforts",
        "reasoning_efforts",
        "reasoningEfforts",
    ]
    .iter()
    .find_map(|key| model.get(*key).and_then(Value::as_array));

    let Some(source) = source else {
        return Vec::new();
    };
    let mut result = Vec::new();
    for item in source {
        let value = text(Some(item)).or_else(|| {
            object(item).and_then(|entry| {
                ["effort", "id", "value", "name"]
                    .iter()
                    .find_map(|key| text(entry.get(*key)))
            })
        });
        if let Some(value) = value {
            if !result.contains(&value) {
                result.push(value);
            }
        }
    }
    result
}

pub fn parse_official_models(body: &Value) -> Result<Vec<CodexModel>, String> {
    let root = object(body);
    let source = root
        .and_then(|root| root.get("models").or_else(|| root.get("data")))
        .and_then(Value::as_array)
        .or_else(|| body.as_array())
        .ok_or_else(|| "Codex model discovery response does not contain a model list".to_owned())?;

    let mut models = Vec::new();
    for raw in source {
        let Some(model) = object(raw) else {
            continue;
        };
        if model.get("supported_in_api").and_then(Value::as_bool) == Some(false)
            || model.get("supportedInApi").and_then(Value::as_bool) == Some(false)
            || text(model.get("visibility"))
                .is_some_and(|value| value.eq_ignore_ascii_case("hidden"))
        {
            continue;
        }
        let Some(id) = model_id(raw) else {
            continue;
        };
        if models.iter().any(|item: &CodexModel| item.id == id) {
            continue;
        }
        models.push(CodexModel {
            display_name: text(model.get("display_name"))
                .or_else(|| text(model.get("displayName")))
                .or_else(|| text(model.get("title")))
                .or_else(|| text(model.get("name")))
                .unwrap_or_else(|| id.clone()),
            description: text(model.get("description")),
            max_output_tokens: [
                "max_output_tokens",
                "maxOutputTokens",
                "max_completion_tokens",
                "maxCompletionTokens",
            ]
            .iter()
            .find_map(|key| positive_integer(model.get(*key))),
            reasoning_efforts: reasoning_efforts(model),
            images: true,
            id,
        });
    }

    if let Some(default_model) = root.and_then(|root| {
        [
            "default_model",
            "defaultModel",
            "default_model_slug",
            "defaultModelSlug",
            "primary_model",
            "primaryModel",
        ]
        .iter()
        .find_map(|key| root.get(*key).and_then(model_id))
    }) {
        models.sort_by_key(|model| model.id != default_model);
    }
    Ok(models)
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    use serde_json::json;

    #[test]
    fn jwt_email_reads_profile_claim() {
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"none"}"#);
        let payload = URL_SAFE_NO_PAD.encode(
            json!({
                "https://api.openai.com/profile": { "email": "codex@example.com" }
            })
            .to_string(),
        );
        let token = format!("{header}.{payload}.sig");
        assert_eq!(jwt_email(&token).as_deref(), Some("codex@example.com"));
    }
}
