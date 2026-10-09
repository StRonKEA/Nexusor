use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{Error, Result};

const USAGE_URL: &str = "https://chatgpt.com/backend-api/wham/usage";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UsageWindow {
    #[serde(default)]
    pub used_percent: f64,
    #[serde(default)]
    pub reset_at_ms: Option<i64>,
    #[serde(default)]
    pub limit_window_seconds: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AccountUsage {
    #[serde(default, alias = "primaryWindow")]
    pub primary_window: Option<UsageWindow>,
    #[serde(default, alias = "secondaryWindow")]
    pub secondary_window: Option<UsageWindow>,
    #[serde(default)]
    pub plan_type: Option<String>,
    #[serde(default)]
    pub reset_credits: Option<i64>,
}

pub async fn query_usage(
    client: &reqwest::Client,
    access_token: &str,
    account_id: Option<&str>,
) -> Result<AccountUsage> {
    let mut req = client
        .get(USAGE_URL)
        .header("authorization", format!("Bearer {access_token}"))
        .header("accept", "application/json");

    if let Some(acct) = account_id {
        req = req.header("ChatGPT-Account-Id", acct);
    }

    let res = req.send().await?;
    let status = res.status();
    let text = res.text().await.unwrap_or_default();

    if !status.is_success() {
        return Err(Error::Provider(format!(
            "ChatGPT usage query failed ({status}): {text}"
        )));
    }

    let json: Value = serde_json::from_str(&text)
        .map_err(|e| Error::Provider(format!("invalid usage JSON: {e}")))?;

    let parse_window = |val: Option<&Value>| -> Option<UsageWindow> {
        let obj = val?.as_object()?;
        let used = obj
            .get("used_percent")
            .or_else(|| obj.get("usedPercent"))
            .and_then(Value::as_f64)?;

        let reset = obj
            .get("reset_at")
            .or_else(|| obj.get("resetAt"))
            .and_then(Value::as_i64)
            .map(|secs| {
                if secs < 100_000_000_000 {
                    secs * 1000
                } else {
                    secs
                }
            });

        let limit_seconds = obj
            .get("limit_window_seconds")
            .or_else(|| obj.get("limitWindowSeconds"))
            .and_then(Value::as_i64);

        Some(UsageWindow {
            used_percent: used,
            reset_at_ms: reset,
            limit_window_seconds: limit_seconds,
        })
    };

    let rate_limit = json.get("rate_limit").or_else(|| json.get("rateLimit"));

    let primary_val = rate_limit
        .and_then(|r| r.get("primary_window").or_else(|| r.get("primaryWindow")))
        .or_else(|| json.get("primary_window"))
        .or_else(|| json.get("primaryWindow"));

    let secondary_val = rate_limit
        .and_then(|r| {
            r.get("secondary_window")
                .or_else(|| r.get("secondaryWindow"))
        })
        .or_else(|| json.get("secondary_window"))
        .or_else(|| json.get("secondaryWindow"));

    let plan_type = json
        .get("plan_type")
        .or_else(|| json.get("planType"))
        .and_then(Value::as_str)
        .map(|s| s.to_string());

    let reset_credits = json
        .pointer("/rate_limit_reset_credits/available_count")
        .or_else(|| json.pointer("/rateLimitResetCredits/availableCount"))
        .and_then(Value::as_i64);

    Ok(AccountUsage {
        primary_window: parse_window(primary_val),
        secondary_window: parse_window(secondary_val),
        plan_type,
        reset_credits,
    })
}

const CREDITS_URL: &str = "https://chatgpt.com/backend-api/wham/rate-limit-reset-credits";
const CONSUME_CREDIT_URL: &str =
    "https://chatgpt.com/backend-api/wham/rate-limit-reset-credits/consume";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResetCreditInfo {
    pub id: String,
    pub grant_time: Option<String>,
    pub expiry_time: Option<String>,
}

pub async fn list_reset_credits(
    client: &reqwest::Client,
    access_token: &str,
    account_id: Option<&str>,
) -> Result<Vec<ResetCreditInfo>> {
    let mut req = client
        .get(CREDITS_URL)
        .header("authorization", format!("Bearer {access_token}"))
        .header("accept", "application/json")
        .header("user-agent", "CodexCLI/0.153.0");

    if let Some(acct) = account_id {
        req = req.header("ChatGPT-Account-Id", acct);
    }

    let res = req.send().await?;
    let status = res.status();
    let text = res.text().await.unwrap_or_default();

    if !status.is_success() {
        return Err(Error::Provider(format!(
            "ChatGPT reset credits query failed ({status}): {text}"
        )));
    }

    let json: Value = serde_json::from_str(&text)
        .map_err(|e| Error::Provider(format!("invalid reset credits JSON: {e}")))?;

    let mut credits = Vec::new();
    if let Some(arr) = json.get("credits").and_then(Value::as_array) {
        for item in arr {
            if let Some(id) = item.get("id").and_then(Value::as_str) {
                credits.push(ResetCreditInfo {
                    id: id.to_string(),
                    grant_time: item
                        .get("grant_time")
                        .and_then(Value::as_str)
                        .map(String::from),
                    expiry_time: item
                        .get("expiry_time")
                        .and_then(Value::as_str)
                        .map(String::from),
                });
            }
        }
    }

    Ok(credits)
}

pub async fn consume_reset_credit(
    client: &reqwest::Client,
    access_token: &str,
    account_id: Option<&str>,
) -> Result<String> {
    let credits = list_reset_credits(client, access_token, account_id).await?;
    let Some(first) = credits.first() else {
        return Err(Error::Provider(
            "Bu ChatGPT hesabında kullanılabilir sıfırlama jetonu (Reset Credit) bulunmuyor."
                .into(),
        ));
    };

    let redeem_request_id = uuid::Uuid::new_v4().to_string();
    let body = serde_json::json!({
        "credit_id": first.id,
        "redeem_request_id": redeem_request_id,
    });

    let mut req = client
        .post(CONSUME_CREDIT_URL)
        .header("authorization", format!("Bearer {access_token}"))
        .header("accept", "application/json")
        .header("content-type", "application/json")
        .header("openai-beta", "codex-1")
        .header("originator", "Codex Desktop")
        .header("user-agent", "CodexCLI/0.153.0")
        .json(&body);

    if let Some(acct) = account_id {
        req = req.header("ChatGPT-Account-Id", acct);
    }

    let res = req.send().await?;
    let status = res.status();
    let text = res.text().await.unwrap_or_default();

    if !status.is_success() {
        return Err(Error::Provider(format!(
            "Sıfırlama jetonu kullanılamadı ({status}): {text}"
        )));
    }

    Ok(first.id.clone())
}
