use serde::{Deserialize, Serialize};

use crate::plugin::ResourceRecord;

use super::super::account_private_data::normalize_account_private_data;

pub const RESOURCE_TYPE: &str = "google-account";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AntigravityAccountData {
    #[serde(rename = "accessToken")]
    pub access_token: String,
    #[serde(rename = "refreshToken", default)]
    pub refresh_token: Option<String>,
    #[serde(rename = "projectId", default)]
    pub project_id: Option<String>,
    #[serde(rename = "displayName", default)]
    pub display_name: String,
    /// Access-token expiry as unix epoch milliseconds (from OAuth `expires_in`).
    #[serde(rename = "expiresAtMs", default)]
    pub expires_at_ms: Option<i64>,
}

impl AntigravityAccountData {
    pub fn from_record(record: &ResourceRecord) -> Option<Self> {
        serde_json::from_value(normalize_account_private_data(record.private_data.clone())).ok()
    }
}

pub fn account_identity(email: Option<&str>, access_token: &str) -> (String, String) {
    if let Some(email) = email.filter(|e| !e.is_empty()) {
        (format!("antigravity:{email}"), email.to_owned())
    } else {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(access_token.as_bytes());
        let hex = format!("{:x}", hasher.finalize());
        let id = &hex[..12];
        (format!("antigravity:{id}"), id.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::{ResourceRecord, ResourceState};

    #[test]
    fn from_record_accepts_duplicate_display_name_keys() {
        let record = ResourceRecord {
            id: "1".into(),
            key: "k".into(),
            private_data: serde_json::json!({
                "accessToken": "tok",
                "display_name": "old",
                "displayName": "Google (a@b.c)",
                "projectId": "proj",
            }),
            state: ResourceState::Ready,
            created_at_ms: 0,
            updated_at_ms: 0,
        };
        let account = AntigravityAccountData::from_record(&record).expect("parse");
        assert_eq!(account.display_name, "Google (a@b.c)");
        assert_eq!(account.project_id.as_deref(), Some("proj"));
    }

    #[test]
    fn account_identity_uses_email_as_display_name() {
        let (key, display) = account_identity(Some("user@example.com"), "token");
        assert_eq!(key, "antigravity:user@example.com");
        assert_eq!(display, "user@example.com");
    }
}
