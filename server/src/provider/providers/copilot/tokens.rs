use super::oauth;
use super::resources::CopilotAccountData;
use crate::Result;

fn unix_now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub fn copilot_token_needs_refresh(account: &CopilotAccountData) -> bool {
    let skew_ms = 120_000; // 2 minutes skew
    let Some(expires_at) = account.copilot_expires_at_ms else {
        return true;
    };
    expires_at <= unix_now_ms() + skew_ms
}

pub async fn ensure_fresh_account(
    client: &reqwest::Client,
    account: &mut CopilotAccountData,
) -> Result<bool> {
    if !copilot_token_needs_refresh(account) && account.copilot_token.is_some() {
        return Ok(false);
    }

    let (copilot_token, expires_at_ms) =
        oauth::exchange_copilot_token(client, &account.github_token).await?;

    account.copilot_token = Some(copilot_token);
    account.copilot_expires_at_ms = Some(expires_at_ms);

    tracing::info!(account = %account.display_name, "successfully refreshed GitHub Copilot session token");
    Ok(true)
}
