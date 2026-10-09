use serde::{Deserialize, Serialize};

use crate::plugin::ResourceRecord;

use super::super::account_private_data::normalize_account_private_data;
use super::models::jwt_email;

pub const RESOURCE_TYPE: &str = "grok-account";
pub const USERINFO_URL: &str = "https://auth.x.ai/oauth2/userinfo";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GrokAccountData {
    #[serde(rename = "accessToken")]
    pub access_token: String,
    #[serde(rename = "refreshToken", default)]
    pub refresh_token: Option<String>,
    #[serde(rename = "displayName", default)]
    pub display_name: String,
}

impl GrokAccountData {
    pub fn from_record(record: &ResourceRecord) -> Option<Self> {
        serde_json::from_value(normalize_account_private_data(record.private_data.clone())).ok()
    }
}

pub fn account_identity(email: Option<&str>, access_token: &str) -> (String, String) {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(access_token.as_bytes());
    let hex = format!("{:x}", hasher.finalize());
    let id = &hex[..12];

    let label = email
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .or_else(|| jwt_email(access_token))
        .unwrap_or_else(|| format!("Grok ({id})"));

    (format!("grok:{id}"), label)
}

pub async fn fetch_user_email(client: &reqwest::Client, access_token: &str) -> Option<String> {
    let response = client
        .get(USERINFO_URL)
        .header("accept", "application/json")
        .bearer_auth(access_token)
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let body: serde_json::Value = response.json().await.ok()?;
    body.get("email")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .or_else(|| {
            body.get("preferred_username")
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::{ResourceRecord, ResourceState};
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    use serde_json::json;

    #[test]
    fn from_record_dedupes_display_name_keys() {
        let record = ResourceRecord {
            id: "1".into(),
            key: "k".into(),
            private_data: serde_json::json!({
                "access_token": "tok",
                "display_name": "snake",
                "displayName": "camel",
            }),
            state: ResourceState::Ready,
            created_at_ms: 0,
            updated_at_ms: 0,
        };
        let account = GrokAccountData::from_record(&record).expect("parse");
        assert_eq!(account.access_token, "tok");
        assert_eq!(account.display_name, "camel");
    }

    #[test]
    fn account_identity_prefers_email() {
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"none"}"#);
        let payload = URL_SAFE_NO_PAD.encode(json!({ "sub": "x" }).to_string());
        let token = format!("{header}.{payload}.sig");
        let (key, display) = account_identity(Some("grok@x.ai"), &token);
        assert!(key.starts_with("grok:"));
        assert_eq!(display, "grok@x.ai");
    }
}
