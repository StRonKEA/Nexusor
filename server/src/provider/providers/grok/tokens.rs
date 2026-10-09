use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde_json::Value;

use super::oauth;
use super::resources::GrokAccountData;
use crate::Result;

fn unix_now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub fn access_token_needs_refresh(token: &str) -> bool {
    let skew_ms = 120_000; // 2 minutes skew
    let Some(payload) = token.split('.').nth(1) else {
        return false;
    };
    let Ok(bytes) = URL_SAFE_NO_PAD.decode(payload) else {
        return false;
    };
    let Ok(json) = serde_json::from_slice::<Value>(&bytes) else {
        return false;
    };
    let Some(exp) = json.get("exp").and_then(Value::as_i64) else {
        return false;
    };
    exp * 1000 <= unix_now_ms() + skew_ms
}

pub async fn ensure_fresh_account(
    client: &reqwest::Client,
    account: &mut GrokAccountData,
) -> Result<bool> {
    if !access_token_needs_refresh(&account.access_token) {
        return Ok(false);
    }
    let Some(refresh) = account
        .refresh_token
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    else {
        return Ok(false);
    };

    let (access, new_refresh) = oauth::refresh_access_token(client, refresh).await?;
    account.access_token = access;
    if let Some(r) = new_refresh.filter(|v| !v.trim().is_empty()) {
        account.refresh_token = Some(r);
    }
    tracing::info!(account = %account.display_name, "successfully refreshed xAI Grok access token");
    Ok(true)
}
