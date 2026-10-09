use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
pub const USER_AGENT: &str = "claude-cli/2.1.280 (external, cli)";
pub const ANTHROPIC_BETA: &str = "claude-code-20250219,oauth-2025-04-20,interleaved-thinking-2025-05-14,context-management-2025-06-27,prompt-caching-scope-2026-01-05";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClaudeCodeUsageWindow {
    pub percent: f64,
    #[serde(default)]
    pub reset_at_ms: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct ClaudeCodeQuota {
    #[serde(default)]
    pub five_hour: Option<ClaudeCodeUsageWindow>,
    #[serde(default)]
    pub weekly: Option<ClaudeCodeUsageWindow>,
    #[serde(default)]
    pub sonnet_weekly: Option<ClaudeCodeUsageWindow>,
    #[serde(default)]
    pub opus_weekly: Option<ClaudeCodeUsageWindow>,
}

pub async fn query_usage(client: &reqwest::Client, access_token: &str) -> Result<ClaudeCodeQuota> {
    let response = client
        .get(USAGE_URL)
        .header(
            reqwest::header::AUTHORIZATION,
            format!("Bearer {access_token}"),
        )
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .header("anthropic-beta", ANTHROPIC_BETA)
        .header(reqwest::header::ACCEPT, "application/json")
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await?;

    if !response.status().is_success() {
        let text = response.text().await.unwrap_or_default();
        return Err(Error::Provider(format!(
            "Claude Code usage request failed: {text}"
        )));
    }

    let payload: Value = response.json().await?;
    parse_claude_quota(&payload)
}

fn parse_claude_quota(payload: &Value) -> Result<ClaudeCodeQuota> {
    let mut quota = ClaudeCodeQuota::default();

    if let Some(w) = parse_bucket(payload.get("five_hour")) {
        quota.five_hour = Some(w);
    }
    if let Some(w) = parse_bucket(payload.get("seven_day")) {
        quota.weekly = Some(w);
    }
    if let Some(w) = parse_bucket(payload.get("seven_day_sonnet")) {
        quota.sonnet_weekly = Some(w);
    }
    if let Some(w) = parse_bucket(payload.get("seven_day_opus")) {
        quota.opus_weekly = Some(w);
    }

    Ok(quota)
}

fn parse_bucket(val: Option<&Value>) -> Option<ClaudeCodeUsageWindow> {
    let v = val?;
    let percent = v
        .get("percent")
        .or_else(|| v.get("utilization"))
        .or_else(|| v.get("used_percent"))
        .and_then(Value::as_f64)?;

    let reset_at_ms = v
        .get("resets_at")
        .or_else(|| v.get("reset_at"))
        .or_else(|| v.get("resetAt"))
        .and_then(|r| {
            if let Some(num) = r.as_i64() {
                Some(num)
            } else if let Some(iso) = r.as_str() {
                chrono::DateTime::parse_from_rfc3339(iso)
                    .ok()
                    .map(|d| d.timestamp_millis())
            } else {
                None
            }
        });

    Some(ClaudeCodeUsageWindow {
        percent: percent.clamp(0.0, 100.0),
        reset_at_ms,
    })
}
