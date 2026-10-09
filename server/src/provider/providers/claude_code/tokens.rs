use super::resources::ClaudeCodeAccountData;
use crate::Result;

pub async fn ensure_fresh_account(
    client: &reqwest::Client,
    account: &mut ClaudeCodeAccountData,
) -> Result<bool> {
    let now = chrono::Utc::now().timestamp_millis();
    let skew_ms = 120_000; // 2 minutes skew
    let is_expired = match account.expires_at_ms {
        Some(exp) => exp <= now + skew_ms,
        None => false,
    };

    if !is_expired {
        return Ok(false);
    }

    let refresh = match &account.refresh_token {
        Some(rt) if !rt.trim().is_empty() => rt.clone(),
        _ => return Ok(false),
    };

    let (access, new_refresh, expires_at_ms) =
        super::oauth::refresh_access_token(client, &refresh).await?;

    account.access_token = access;
    account.refresh_token = Some(new_refresh);
    account.expires_at_ms = Some(expires_at_ms);

    Ok(true)
}
