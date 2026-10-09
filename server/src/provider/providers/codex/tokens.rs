use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde_json::Value;

use super::oauth;
use super::resources::AccountData;
use crate::Result;

fn unix_now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub fn access_token_needs_refresh(token: &str) -> bool {
    let skew_ms = 120_000; // 2 minutes skew
                           // An empty token is what an upstream rejection leaves behind once the token has
                           // been invalidated, so it has to read as "refresh me" rather than "nothing to do".
    if token.trim().is_empty() {
        return true;
    }
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
    account: &mut AccountData,
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
    tracing::info!(account = %account.display_name, "successfully refreshed OpenAI Codex access token");
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A token the upstream rejected is blanked, and a blank token has to read as
    /// "refresh me". Without this the account stays broken until the old token's
    /// expiry, which is exactly the self-healing this closes.
    #[test]
    fn an_invalidated_token_reads_as_needing_refresh() {
        assert!(access_token_needs_refresh(""));
        assert!(access_token_needs_refresh("   "));
    }

    #[test]
    fn a_valid_token_does_not_need_refresh() {
        let future = unix_now_ms() + 3_600_000;
        assert!(!access_token_needs_refresh(&token_expiring_at(future)));
    }

    #[test]
    fn a_token_at_or_past_its_skew_needs_refresh() {
        let past = unix_now_ms() - 1_000;
        assert!(access_token_needs_refresh(&token_expiring_at(past)));
        // Inside the two minute skew: still refreshed, so a token about to lapse is
        // never sent and rejected.
        let inside_skew = unix_now_ms() + 30_000;
        assert!(access_token_needs_refresh(&token_expiring_at(inside_skew)));
    }

    #[test]
    fn an_opaque_token_is_left_alone() {
        // Not a JWT: there is no expiry to reason about, so do not force a refresh.
        assert!(!access_token_needs_refresh("not-a-jwt"));
    }

    fn token_expiring_at(exp_ms: i64) -> String {
        let payload = URL_SAFE_NO_PAD.encode(
            serde_json::json!({ "exp": exp_ms / 1000 })
                .to_string()
                .as_bytes(),
        );
        format!("header.{payload}.sig")
    }
}
