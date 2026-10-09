use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{Error, Result};

pub const CLIENT_ID: &str = "Iv1.b507a08c87ecfe98";
const DEVICE_CODE_URL: &str = "https://github.com/login/device/code";
const ACCESS_TOKEN_URL: &str = "https://github.com/login/oauth/access_token";
const COPILOT_TOKEN_URL: &str = "https://api.github.com/copilot_internal/v2/token";
const USER_URL: &str = "https://api.github.com/user";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CopilotDeviceSession {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub interval: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CopilotDeviceBeginResult {
    pub session: Value,
    pub instructions: String,
    pub url: String,
    pub user_code: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CopilotPollStatus {
    Pending,
    SlowDown,
    Denied,
    Failed(String),
    Success {
        github_token: String,
        copilot_token: String,
        copilot_expires_at_ms: i64,
        login: String,
    },
}

pub async fn begin_device_flow(client: &reqwest::Client) -> Result<CopilotDeviceBeginResult> {
    let response = client
        .post(DEVICE_CODE_URL)
        .header("accept", "application/json")
        .form(&[("client_id", CLIENT_ID), ("scope", "read:user")])
        .send()
        .await?;

    let text = response.text().await.unwrap_or_default();
    let body: Value = serde_json::from_str(&text)
        .map_err(|e| Error::Provider(format!("invalid device code JSON: {e}")))?;

    let user_code = body
        .get("user_code")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Provider("missing user_code in device code response".into()))?
        .to_string();
    let device_code = body
        .get("device_code")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Provider("missing device_code in device code response".into()))?
        .to_string();
    let verification_uri = body
        .get("verification_uri")
        .and_then(Value::as_str)
        .unwrap_or("https://github.com/login/device")
        .to_string();
    let interval = body.get("interval").and_then(Value::as_u64).unwrap_or(5);

    let verify_url = format!("{verification_uri}?user_code={user_code}");
    let session = serde_json::json!({
        "device_code": device_code,
        "user_code": user_code,
        "verification_uri": verification_uri,
        "interval": interval,
    });

    Ok(CopilotDeviceBeginResult {
        session,
        instructions: format!(
            "GitHub doğrulama kodunuz: {user_code}\nTarayıcıda açılan sayfada bu kodu onaylayın."
        ),
        url: verify_url,
        user_code,
    })
}

pub async fn poll_device_flow(
    client: &reqwest::Client,
    session: &CopilotDeviceSession,
) -> Result<CopilotPollStatus> {
    let response = client
        .post(ACCESS_TOKEN_URL)
        .header("accept", "application/json")
        .form(&[
            ("client_id", CLIENT_ID),
            ("device_code", session.device_code.as_str()),
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
        ])
        .send()
        .await?;

    let text = response.text().await.unwrap_or_default();
    let body: Value = serde_json::from_str(&text).unwrap_or(Value::Null);

    if let Some(token) = body.get("access_token").and_then(Value::as_str) {
        let login = fetch_github_user(client, token)
            .await
            .unwrap_or_else(|_| "GitHub User".into());
        let (copilot_token, expires_at_ms) = exchange_copilot_token(client, token).await?;

        return Ok(CopilotPollStatus::Success {
            github_token: token.to_string(),
            copilot_token,
            copilot_expires_at_ms: expires_at_ms,
            login,
        });
    }

    let error = body.get("error").and_then(Value::as_str).unwrap_or("");
    match error {
        "authorization_pending" => Ok(CopilotPollStatus::Pending),
        "slow_down" => Ok(CopilotPollStatus::SlowDown),
        "access_denied" => Ok(CopilotPollStatus::Denied),
        "expired_token" => Ok(CopilotPollStatus::Failed(
            "Doğrulama kodunun süresi doldu".into(),
        )),
        _ => {
            let desc = body
                .get("error_description")
                .and_then(Value::as_str)
                .unwrap_or(error);
            Ok(CopilotPollStatus::Failed(format!(
                "GitHub OAuth error: {desc}"
            )))
        }
    }
}

pub async fn exchange_copilot_token(
    client: &reqwest::Client,
    github_token: &str,
) -> Result<(String, i64)> {
    let response = client
        .get(COPILOT_TOKEN_URL)
        .header("authorization", format!("token {github_token}"))
        .header("accept", "application/json")
        .header("editor-version", "vscode/1.98.0")
        .header("user-agent", "GithubCopilot/1.250.0")
        .send()
        .await?;

    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(Error::Provider(format!(
            "GitHub Copilot token exchange failed ({status}): Bu hesabın aktif bir GitHub Copilot üyeliği olduğundan emin olun."
        )));
    }

    let body: Value = serde_json::from_str(&text)
        .map_err(|e| Error::Provider(format!("invalid Copilot token JSON: {e}")))?;

    let token = body
        .get("token")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Provider("missing token in Copilot response".into()))?
        .to_string();

    let expires_at_ms = body
        .get("expires_at")
        .and_then(Value::as_i64)
        .map(|s| s * 1000)
        .unwrap_or_else(|| chrono::Utc::now().timestamp_millis() + 1800 * 1000);

    Ok((token, expires_at_ms))
}

async fn fetch_github_user(client: &reqwest::Client, github_token: &str) -> Result<String> {
    let response = client
        .get(USER_URL)
        .header("authorization", format!("token {github_token}"))
        .header("accept", "application/json")
        .header("user-agent", "Nexusor")
        .send()
        .await?;

    let body: Value = response.json().await.unwrap_or(Value::Null);
    let login = body
        .get("login")
        .and_then(Value::as_str)
        .unwrap_or("GitHub User");
    Ok(login.to_string())
}
