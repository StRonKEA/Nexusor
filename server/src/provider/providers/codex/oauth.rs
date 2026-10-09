use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{Error, Result};

pub const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
pub const DEVICE_CODE_URL: &str = "https://auth.openai.com/api/accounts/deviceauth/usercode";
pub const DEVICE_TOKEN_URL: &str = "https://auth.openai.com/api/accounts/deviceauth/token";
pub const OAUTH_TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
pub const REDIRECT_URI: &str = "https://auth.openai.com/deviceauth/callback";
pub const VERIFICATION_URI: &str = "https://auth.openai.com/codex/device";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeviceSession {
    pub device_auth_id: String,
    pub user_code: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeviceBeginResult {
    pub session: Value,
    pub user_code: String,
    pub verification_url: String,
    pub expires_at_ms: i64,
    pub poll_interval_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DevicePollStatus {
    Pending,
    SlowDown,
    Denied(Option<String>),
    Failed(String),
    Success {
        access_token: String,
        refresh_token: Option<String>,
        account_id: Option<String>,
        email: Option<String>,
    },
}

pub async fn begin_device_flow(client: &reqwest::Client) -> Result<DeviceBeginResult> {
    let response = client
        .post(DEVICE_CODE_URL)
        .header("accept", "application/json")
        .header("content-type", "application/json")
        .json(&serde_json::json!({ "client_id": CLIENT_ID }))
        .send()
        .await?;

    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(Error::Provider(format!(
            "Failed to request OpenAI Codex device code (HTTP {status}): {text}"
        )));
    }

    let body: Value = serde_json::from_str(&text)
        .map_err(|e| Error::Provider(format!("invalid device auth JSON: {e}")))?;

    let device_auth_id = body
        .get("device_auth_id")
        .or_else(|| body.get("device_code"))
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Provider("missing device_auth_id in response".into()))?
        .to_string();

    let user_code = body
        .get("user_code")
        .or_else(|| body.get("usercode"))
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Provider("missing user_code in response".into()))?
        .to_string();

    let expires_in = body
        .get("expires_in")
        .and_then(Value::as_i64)
        .unwrap_or(900);
    let interval = body.get("interval").and_then(Value::as_u64).unwrap_or(5);

    let session = DeviceSession {
        device_auth_id,
        user_code: user_code.clone(),
    };

    Ok(DeviceBeginResult {
        session: serde_json::to_value(&session)?,
        user_code,
        verification_url: VERIFICATION_URI.into(),
        expires_at_ms: chrono::Utc::now().timestamp_millis() + expires_in * 1000,
        poll_interval_ms: interval * 1000,
    })
}

pub async fn poll_device_flow(
    client: &reqwest::Client,
    session: &DeviceSession,
) -> Result<DevicePollStatus> {
    let response = client
        .post(DEVICE_TOKEN_URL)
        .header("accept", "application/json")
        .header("content-type", "application/json")
        .json(&serde_json::json!({
            "device_auth_id": session.device_auth_id,
            "user_code": session.user_code,
        }))
        .send()
        .await?;

    let status = response.status();
    if status.as_u16() == 403 || status.as_u16() == 404 {
        return Ok(DevicePollStatus::Pending);
    }

    let text = response.text().await.unwrap_or_default();
    let body: Value = serde_json::from_str(&text).unwrap_or(Value::Null);

    let error_code = body
        .get("error")
        .and_then(|e| {
            if let Some(s) = e.as_str() {
                Some(s.to_string())
            } else {
                e.get("code")
                    .or_else(|| e.get("type"))
                    .and_then(Value::as_str)
                    .map(String::from)
            }
        })
        .unwrap_or_default();

    let error_msg = body
        .get("error_description")
        .or_else(|| body.get("message"))
        .or_else(|| body.pointer("/error/message"))
        .and_then(Value::as_str)
        .map(String::from);

    if matches!(
        error_code.as_str(),
        "authorization_pending"
            | "pending"
            | "waiting"
            | "in_progress"
            | "device_authorization_pending"
    ) {
        return Ok(DevicePollStatus::Pending);
    }
    if error_code == "slow_down" {
        return Ok(DevicePollStatus::SlowDown);
    }
    if error_code == "expired_token" || error_code == "expired" {
        return Ok(DevicePollStatus::Failed(
            error_msg.unwrap_or_else(|| "Device authorization code expired".into()),
        ));
    }
    if error_code == "access_denied" || error_code == "denied" {
        return Ok(DevicePollStatus::Denied(error_msg));
    }

    if let Some(token) = body.get("access_token").and_then(Value::as_str) {
        let refresh_token = body
            .get("refresh_token")
            .and_then(Value::as_str)
            .map(String::from);
        let account_id = super::models::chat_gpt_account_id(token);
        let email = body
            .get("id_token")
            .and_then(Value::as_str)
            .and_then(super::models::jwt_email)
            .or_else(|| super::models::jwt_email(token));
        return Ok(DevicePollStatus::Success {
            access_token: token.to_string(),
            refresh_token,
            account_id,
            email,
        });
    }

    if let (Some(code), Some(verifier)) = (
        body.get("authorization_code").and_then(Value::as_str),
        body.get("code_verifier").and_then(Value::as_str),
    ) {
        return exchange_auth_code(client, code, verifier).await;
    }

    if status.is_client_error() && error_code.is_empty() {
        return Ok(DevicePollStatus::Pending);
    }

    Ok(DevicePollStatus::Failed(error_msg.unwrap_or_else(|| {
        format!("OAuth failed (HTTP {status})")
    })))
}

pub async fn exchange_auth_code(
    client: &reqwest::Client,
    authorization_code: &str,
    code_verifier: &str,
) -> Result<DevicePollStatus> {
    let params = [
        ("grant_type", "authorization_code"),
        ("code", authorization_code),
        ("redirect_uri", REDIRECT_URI),
        ("client_id", CLIENT_ID),
        ("code_verifier", code_verifier),
    ];

    let response = client
        .post(OAUTH_TOKEN_URL)
        .header("accept", "application/json")
        .form(&params)
        .send()
        .await?;

    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Ok(DevicePollStatus::Failed(format!(
            "Failed to exchange Codex authorization code (HTTP {status}): {text}"
        )));
    }

    let body: Value = serde_json::from_str(&text)
        .map_err(|e| Error::Provider(format!("invalid token response JSON: {e}")))?;

    let access_token = body
        .get("access_token")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Provider("missing access_token".into()))?;

    let refresh_token = body
        .get("refresh_token")
        .and_then(Value::as_str)
        .map(String::from);
    let account_id = super::models::chat_gpt_account_id(access_token);
    let email = body
        .get("id_token")
        .and_then(Value::as_str)
        .and_then(super::models::jwt_email)
        .or_else(|| super::models::jwt_email(access_token));

    Ok(DevicePollStatus::Success {
        access_token: access_token.to_string(),
        refresh_token,
        account_id,
        email,
    })
}

pub async fn refresh_access_token(
    client: &reqwest::Client,
    refresh_token: &str,
) -> Result<(String, Option<String>)> {
    let params = [
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token),
        ("client_id", CLIENT_ID),
    ];

    let response = client
        .post(OAUTH_TOKEN_URL)
        .header("accept", "application/json")
        .form(&params)
        .send()
        .await?;

    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(Error::Provider(format!(
            "Failed to refresh OpenAI Codex token (HTTP {status}): {text}"
        )));
    }

    let body: Value = serde_json::from_str(&text)
        .map_err(|e| Error::Provider(format!("invalid refresh token JSON: {e}")))?;

    let new_access = body
        .get("access_token")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Provider("missing access_token in refresh response".into()))?
        .to_string();

    let new_refresh = body
        .get("refresh_token")
        .and_then(Value::as_str)
        .map(String::from);

    Ok((new_access, new_refresh))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_device_session_structure() {
        let json_val = serde_json::json!({
            "device_auth_id": "auth-123",
            "user_code": "ABCD-1234"
        });
        let session: DeviceSession = serde_json::from_value(json_val).unwrap();
        assert_eq!(session.device_auth_id, "auth-123");
        assert_eq!(session.user_code, "ABCD-1234");
    }

    #[tokio::test]
    async fn refresh_access_token_handles_network_or_invalid_endpoint() {
        let client = reqwest::Client::new();
        let result = refresh_access_token(&client, "dummy_token").await;
        // In isolated offline unit test it produces Http or Provider error, never panics.
        assert!(result.is_err());
    }
}
