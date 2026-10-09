use serde::{Deserialize, Serialize};

pub const RESOURCE_TYPE: &str = "claude-code-account";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeCodeAccountData {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub expires_at_ms: Option<i64>,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub quota: Option<super::usage::ClaudeCodeQuota>,
}

impl ClaudeCodeAccountData {
    pub fn from_record(record: &crate::plugin::ResourceRecord) -> Option<Self> {
        serde_json::from_value(record.private_data.clone()).ok()
    }
}
