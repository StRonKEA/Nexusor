use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{Error, Result};

const GROK_CLI_BILLING_URL: &str = "https://cli-chat-proxy.grok.com/v1/billing?format=credits";
const XAI_API_BILLING_URL: &str = "https://api.x.ai/v1/billing?format=credits";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GrokUsage {
    pub remaining_percent: f64,
    #[serde(default)]
    pub reset_at_ms: Option<i64>,
}

pub async fn query_usage(client: &reqwest::Client, access_token: &str) -> Result<GrokUsage> {
    // 1. Try Grok CLI chat proxy with user-id (matches opencodex)
    let user_id = extract_jwt_sub(access_token);

    let mut req = client
        .get(GROK_CLI_BILLING_URL)
        .header("authorization", format!("Bearer {access_token}"))
        .header("accept", "application/json")
        .header("token-auth", "xai-grok-cli")
        .header("authenticate-response", "authenticate-response")
        .header("x-client-version", "1.0.0");

    if let Some(uid) = &user_id {
        req = req.header("x-userid", uid);
    }

    if let Ok(res) = req.send().await {
        if res.status().is_success() {
            if let Ok(text) = res.text().await {
                if let Ok(json) = serde_json::from_str::<Value>(&text) {
                    if let Some(usage) = parse_grok_credits_response(&json) {
                        return Ok(usage);
                    }
                }
            }
        }
    }

    // 2. Fallback to api.x.ai/v1/billing
    let res = client
        .get(XAI_API_BILLING_URL)
        .header("authorization", format!("Bearer {access_token}"))
        .header("accept", "application/json")
        .send()
        .await?;

    let status = res.status();
    let text = res.text().await.unwrap_or_default();

    if !status.is_success() {
        return Err(Error::Provider(format!(
            "xAI billing query failed ({status}): {text}"
        )));
    }

    let json: Value = serde_json::from_str(&text)
        .map_err(|e| Error::Provider(format!("invalid billing JSON: {e}")))?;

    if let Some(usage) = parse_grok_credits_response(&json) {
        return Ok(usage);
    }

    let remaining = json
        .get("credits")
        .and_then(|c| {
            c.get("remaining_percentage")
                .or_else(|| c.get("remainingPercentage"))
        })
        .and_then(Value::as_f64)
        .unwrap_or(100.0);

    Ok(GrokUsage {
        remaining_percent: remaining,
        reset_at_ms: None,
    })
}

fn parse_grok_credits_response(json: &Value) -> Option<GrokUsage> {
    let config = json.get("config")?.as_object()?;

    let remaining_percent = if let Some(used_pct) = config
        .get("creditUsagePercent")
        .or_else(|| config.get("credit_usage_percent"))
        .and_then(Value::as_f64)
    {
        (100.0 - used_pct).clamp(0.0, 100.0)
    } else {
        let prepaid = json
            .pointer("/config/prepaidBalance/val")
            .or_else(|| json.pointer("/config/prepaid_balance/val"))
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        let on_demand_cap = json
            .pointer("/config/onDemandCap/val")
            .or_else(|| json.pointer("/config/on_demand_cap/val"))
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        if prepaid <= 0.0 && on_demand_cap <= 0.0 {
            0.0
        } else {
            100.0
        }
    };

    let period = config
        .get("currentPeriod")
        .or_else(|| config.get("current_period"));
    let reset_at_ms = period.and_then(|p| p.get("end")).and_then(|v| {
        if let Some(s) = v.as_str() {
            if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
                return Some(dt.timestamp_millis());
            }
            if let Ok(num) = s.parse::<i64>() {
                return Some(if num < 100_000_000_000 {
                    num * 1000
                } else {
                    num
                });
            }
        } else if let Some(n) = v.as_i64() {
            return Some(if n < 100_000_000_000 { n * 1000 } else { n });
        }
        None
    });

    Some(GrokUsage {
        remaining_percent,
        reset_at_ms,
    })
}

fn extract_jwt_sub(token: &str) -> Option<String> {
    let mut parts = token.split('.');
    let _header = parts.next()?;
    let payload = parts.next()?;

    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    let decoded = URL_SAFE_NO_PAD.decode(payload).ok()?;
    let val: Value = serde_json::from_slice(&decoded).ok()?;
    val.get("sub")
        .and_then(Value::as_str)
        .map(|s| s.trim().to_string())
}
