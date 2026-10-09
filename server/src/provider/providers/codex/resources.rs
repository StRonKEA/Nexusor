use serde::{Deserialize, Serialize};

use crate::plugin::ResourceRecord;

use super::super::account_private_data::normalize_account_private_data;
use super::models::{chat_gpt_account_id, jwt_email};
use super::usage::AccountUsage;

pub const RESOURCE_TYPE: &str = "chatgpt-account";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AccountData {
    #[serde(rename = "accessToken")]
    pub access_token: String,
    #[serde(rename = "refreshToken", default)]
    pub refresh_token: Option<String>,
    #[serde(rename = "accountId", default)]
    pub account_id: Option<String>,
    #[serde(rename = "displayName", default)]
    pub display_name: String,
    #[serde(default)]
    pub quota: Option<AccountUsage>,
}

impl AccountData {
    pub fn from_record(record: &ResourceRecord) -> Option<Self> {
        serde_json::from_value(normalize_account_private_data(record.private_data.clone())).ok()
    }
}

pub fn account_identity(
    access_token: &str,
    account_id: Option<&str>,
    email: Option<&str>,
) -> (String, String) {
    let extracted_acct = chat_gpt_account_id(access_token);
    let identity = account_id
        .map(String::from)
        .or(extracted_acct)
        .unwrap_or_else(|| {
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            hasher.update(access_token.as_bytes());
            let hex = format!("{:x}", hasher.finalize());
            hex[..16].to_string()
        });

    let label = email
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .or_else(|| jwt_email(access_token))
        .unwrap_or_else(|| {
            format!(
                "ChatGPT ({})",
                &identity[..std::cmp::min(12, identity.len())]
            )
        });

    (format!("codex:{identity}"), label)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::{ResourceRecord, ResourceState};
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    use serde_json::json;

    fn fake_jwt(claims: serde_json::Value) -> String {
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"none"}"#);
        let payload = URL_SAFE_NO_PAD.encode(claims.to_string());
        format!("{header}.{payload}.sig")
    }

    #[test]
    fn from_record_dedupes_display_name_keys() {
        let record = ResourceRecord {
            id: "1".into(),
            key: "k".into(),
            private_data: serde_json::json!({
                "accessToken": "tok",
                "display_name": "snake",
                "displayName": "camel",
            }),
            state: ResourceState::Ready,
            created_at_ms: 0,
            updated_at_ms: 0,
        };
        let account = AccountData::from_record(&record).expect("parse");
        assert_eq!(account.access_token, "tok");
        assert_eq!(account.display_name, "camel");
    }

    #[test]
    fn account_identity_prefers_email() {
        let token = fake_jwt(json!({
            "email": "user@example.com",
            "https://api.openai.com/auth": { "chatgpt_account_id": "acct-1234567890" }
        }));
        let (key, display) = account_identity(&token, None, None);
        assert_eq!(key, "codex:acct-1234567890");
        assert_eq!(display, "user@example.com");
    }
}
