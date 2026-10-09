use serde::{Deserialize, Serialize};

use super::super::account_private_data::normalize_account_private_data;
use crate::plugin::ResourceRecord;

pub const RESOURCE_TYPE: &str = "github-copilot-account";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CopilotAccountData {
    #[serde(rename = "githubToken")]
    pub github_token: String,
    #[serde(rename = "copilotToken", default)]
    pub copilot_token: Option<String>,
    #[serde(rename = "copilotExpiresAtMs", default)]
    pub copilot_expires_at_ms: Option<i64>,
    #[serde(rename = "displayName", default)]
    pub display_name: String,
    #[serde(rename = "login", default)]
    pub login: Option<String>,
}

impl CopilotAccountData {
    pub fn from_record(record: &ResourceRecord) -> Option<Self> {
        serde_json::from_value(normalize_account_private_data(record.private_data.clone())).ok()
    }
}
