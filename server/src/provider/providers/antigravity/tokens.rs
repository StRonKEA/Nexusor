use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde_json::Value;

use crate::Result;

use super::oauth;
use super::resources::AntigravityAccountData;

fn unix_now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

/// Refresh ~60s before stored/JWT expiry. Unknown expiry → refresh.
pub fn access_token_needs_refresh(account: &AntigravityAccountData) -> bool {
    let skew_ms = 60_000;
    if let Some(expires_at) = account.expires_at_ms {
        return expires_at <= unix_now_ms() + skew_ms;
    }
    let token = account.access_token.as_str();
    let Some(payload) = token.split('.').nth(1) else {
        // Opaque Google tokens (ya29.*) have no exp claim — force refresh.
        return true;
    };
    let Ok(bytes) = URL_SAFE_NO_PAD.decode(payload) else {
        return true;
    };
    let Ok(json) = serde_json::from_slice::<Value>(&bytes) else {
        return true;
    };
    let Some(exp) = json.get("exp").and_then(Value::as_i64) else {
        return true;
    };
    exp * 1000 <= unix_now_ms() + skew_ms
}

/// Refresh the Google access token when expired. Returns `true` when tokens changed.
pub async fn ensure_fresh_account(
    client: &reqwest::Client,
    account: &mut AntigravityAccountData,
) -> Result<bool> {
    if !access_token_needs_refresh(account) {
        return Ok(false);
    }
    let Some(refresh) = account
        .refresh_token
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Err(crate::Error::Provider(
            "Antigravity access token expired and no refresh token is stored; sign in again".into(),
        ));
    };

    let (access, new_refresh, expires_in) = oauth::refresh_token(client, refresh).await?;
    account.access_token = access;
    if let Some(refresh) = new_refresh.filter(|value| !value.trim().is_empty()) {
        account.refresh_token = Some(refresh);
    }
    if let Some(seconds) = expires_in.filter(|value| *value > 0) {
        account.expires_at_ms = Some(unix_now_ms() + seconds * 1000);
    } else {
        // Google access tokens are typically ~1h.
        account.expires_at_ms = Some(unix_now_ms() + 3_500_000);
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account_with_expiry(expires_at_ms: Option<i64>) -> AntigravityAccountData {
        AntigravityAccountData {
            access_token: "ya29.opaque".into(),
            refresh_token: Some("refresh".into()),
            project_id: None,
            display_name: String::new(),
            expires_at_ms,
        }
    }

    #[test]
    fn opaque_token_without_expiry_needs_refresh() {
        assert!(access_token_needs_refresh(&account_with_expiry(None)));
    }

    #[test]
    fn future_expiry_skips_refresh() {
        assert!(!access_token_needs_refresh(&account_with_expiry(Some(
            unix_now_ms() + 3_600_000
        ))));
    }

    #[test]
    fn past_expiry_needs_refresh() {
        assert!(access_token_needs_refresh(&account_with_expiry(Some(
            unix_now_ms() - 1_000
        ))));
    }
}
