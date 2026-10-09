use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const USAGE_URL: &str = "https://api.kimi.com/coding/v1/usages";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KimiUsageWindow {
    pub percent: f64,
    #[serde(default)]
    pub reset_at_ms: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct KimiQuota {
    #[serde(default)]
    pub five_hour: Option<KimiUsageWindow>,
    #[serde(default)]
    pub weekly: Option<KimiUsageWindow>,
    #[serde(default)]
    pub total: Option<KimiUsageWindow>,
}

pub async fn query_usage(client: &reqwest::Client, access_token: &str) -> Result<KimiQuota> {
    let response = client
        .get(USAGE_URL)
        .header(
            reqwest::header::AUTHORIZATION,
            format!("Bearer {access_token}"),
        )
        .header(reqwest::header::ACCEPT, "application/json")
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await?;

    if !response.status().is_success() {
        let text = response.text().await.unwrap_or_default();
        return Err(Error::Provider(format!(
            "Kimi usage request failed: {text}"
        )));
    }

    let payload: Value = response.json().await?;
    parse_kimi_quota(&payload)
}

fn parse_kimi_quota(value: &Value) -> Result<KimiQuota> {
    let body = unwrap_payload(value);
    let mut five_hour: Option<(f64, Option<i64>)> = None;
    let mut weekly: Option<(f64, Option<i64>)> = parse_row(body.get("usage"));

    if let Some(limits) = body.get("limits").and_then(Value::as_array) {
        for item in limits {
            let detail = item.get("detail").unwrap_or(item);
            let window = item.get("window").unwrap_or(&Value::Null);

            if five_hour.is_none() && is_five_hour(item, detail, window) {
                five_hour = parse_row(Some(detail));
            }
            if weekly.is_none() && is_weekly(item, detail, window) {
                weekly = parse_row(Some(detail));
            }
        }
    }

    let total = parse_row(body.get("totalQuota"));
    let mut quota = KimiQuota::default();

    if let Some((percent, reset_at_ms)) = five_hour {
        quota.five_hour = Some(KimiUsageWindow {
            percent,
            reset_at_ms,
        });
    }
    if let Some((percent, reset_at_ms)) = weekly {
        quota.weekly = Some(KimiUsageWindow {
            percent,
            reset_at_ms,
        });
    }
    if let Some((percent, reset_at_ms)) = total {
        quota.total = Some(KimiUsageWindow {
            percent,
            reset_at_ms,
        });
    }

    Ok(quota)
}

fn unwrap_payload(value: &Value) -> &Value {
    if let Some(data) = value.get("data") {
        if data.is_object() {
            return data;
        }
    }
    value
}

fn parse_row(val: Option<&Value>) -> Option<(f64, Option<i64>)> {
    let v = val?;
    let reset_at = v
        .get("resetAt")
        .or_else(|| v.get("reset_at"))
        .or_else(|| v.get("expiresAt"))
        .and_then(Value::as_i64);

    let limit = v.get("limit").and_then(Value::as_f64);
    if let Some(l) = limit {
        if l > 0.0 {
            let used = v
                .get("used")
                .and_then(Value::as_f64)
                .or_else(|| v.get("remaining").and_then(Value::as_f64).map(|r| l - r));
            if let Some(u) = used {
                let pct = ((u / l) * 100.0).clamp(0.0, 100.0);
                return Some((pct, reset_at));
            }
        }
    }

    let direct = v
        .get("utilization")
        .or_else(|| v.get("percent"))
        .or_else(|| v.get("usedPercent"))
        .or_else(|| v.get("used_percent"))
        .and_then(Value::as_f64);

    direct.map(|p| (p.clamp(0.0, 100.0), reset_at))
}

fn is_five_hour(item: &Value, detail: &Value, window: &Value) -> bool {
    let duration = window
        .get("duration")
        .or_else(|| item.get("duration"))
        .or_else(|| detail.get("duration"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let unit = window
        .get("timeUnit")
        .or_else(|| item.get("timeUnit"))
        .or_else(|| detail.get("timeUnit"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_uppercase();

    if (unit.contains("MINUTE") && duration == 300) || (unit.contains("HOUR") && duration == 5) {
        return true;
    }

    let label = limit_label(item, detail);
    label.contains("5h") || label.contains("5 hour")
}

fn is_weekly(item: &Value, detail: &Value, window: &Value) -> bool {
    let duration = window
        .get("duration")
        .or_else(|| item.get("duration"))
        .or_else(|| detail.get("duration"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let unit = window
        .get("timeUnit")
        .or_else(|| item.get("timeUnit"))
        .or_else(|| detail.get("timeUnit"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_uppercase();

    if (unit.contains("DAY") && duration == 7) || (unit.contains("HOUR") && duration == 168) {
        return true;
    }

    let label = limit_label(item, detail);
    label.contains("weekly") || label.contains("7d") || label.contains("7 day")
}

fn limit_label(item: &Value, detail: &Value) -> String {
    let mut parts = Vec::new();
    for v in [
        item.get("name"),
        item.get("title"),
        item.get("scope"),
        detail.get("name"),
        detail.get("title"),
    ] {
        if let Some(s) = v.and_then(Value::as_str) {
            parts.push(s.to_lowercase());
        }
    }
    parts.join(" ")
}
