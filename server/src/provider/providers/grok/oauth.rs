use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{Error, Result};

pub const CLIENT_ID: &str = "b1a00492-073a-47ea-816f-4c329264a828";
pub const DEVICE_CODE_URL: &str = "https://auth.x.ai/oauth2/device/code";
pub const TOKEN_URL: &str = "https://auth.x.ai/oauth2/token";
pub const SCOPE: &str = "openid profile email offline_access grok-cli:access api:access";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GrokDeviceSession {
    pub device_code: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GrokDeviceBeginResult {
    pub session: Value,
    pub user_code: String,
    pub verification_url: String,
    pub verification_url_complete: Option<String>,
    pub expires_at_ms: i64,
    pub poll_interval_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GrokPollStatus {
    Pending,
    SlowDown,
    Denied(Option<String>),
    Failed(String),
    Success {
        access_token: String,
        refresh_token: Option<String>,
        email: Option<String>,
    },
}

pub async fn begin_device_flow(client: &reqwest::Client) -> Result<GrokDeviceBeginResult> {
    let params = [("client_id", CLIENT_ID), ("scope", SCOPE)];

    let response = client
        .post(DEVICE_CODE_URL)
        .header("accept", "application/json")
        .form(&params)
        .send()
        .await?;

    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(Error::Provider(format!(
            "Failed to request xAI device code (HTTP {status}): {text}"
        )));
    }

    let body: Value = serde_json::from_str(&text)
        .map_err(|e| Error::Provider(format!("invalid xAI device code JSON: {e}")))?;

    let device_code = body
        .get("device_code")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Provider("missing device_code in xAI response".into()))?
        .to_string();

    let user_code = body
        .get("user_code")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Provider("missing user_code in xAI response".into()))?
        .to_string();

    let verification_url = body
        .get("verification_uri")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Provider("missing verification_uri in xAI response".into()))?
        .to_string();

    let verification_url_complete = body
        .get("verification_uri_complete")
        .and_then(Value::as_str)
        .map(String::from);

    let expires_in = body
        .get("expires_in")
        .and_then(Value::as_i64)
        .unwrap_or(900);
    let interval = body.get("interval").and_then(Value::as_u64).unwrap_or(5);

    let session = GrokDeviceSession { device_code };

    Ok(GrokDeviceBeginResult {
        session: serde_json::to_value(&session)?,
        user_code,
        verification_url,
        verification_url_complete,
        expires_at_ms: chrono::Utc::now().timestamp_millis() + expires_in * 1000,
        poll_interval_ms: interval * 1000,
    })
}

pub async fn poll_device_flow(
    client: &reqwest::Client,
    session: &GrokDeviceSession,
) -> Result<GrokPollStatus> {
    let params = [
        ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
        ("client_id", CLIENT_ID),
        ("device_code", session.device_code.as_str()),
    ];

    let response = client
        .post(TOKEN_URL)
        .header("accept", "application/json")
        .form(&params)
        .send()
        .await?;

    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    let body: Value = serde_json::from_str(&text).unwrap_or(Value::Null);

    if status.is_success() {
        if let Some(token) = body.get("access_token").and_then(Value::as_str) {
            let refresh = body
                .get("refresh_token")
                .and_then(Value::as_str)
                .map(String::from);
            let email = body
                .get("id_token")
                .and_then(Value::as_str)
                .and_then(super::models::jwt_email)
                .or_else(|| super::models::jwt_email(token));
            return Ok(GrokPollStatus::Success {
                access_token: token.to_string(),
                refresh_token: refresh,
                email,
            });
        }
    }

    let code = body
        .get("error")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let message = body
        .get("error_description")
        .and_then(Value::as_str)
        .map(String::from);

    match code {
        "authorization_pending" => Ok(GrokPollStatus::Pending),
        "slow_down" => Ok(GrokPollStatus::SlowDown),
        "expired_token" => Ok(GrokPollStatus::Failed(
            message.unwrap_or_else(|| "Device code expired".into()),
        )),
        "access_denied" => Ok(GrokPollStatus::Denied(message)),
        _ => {
            if status.as_u16() == 400 && code.is_empty() {
                Ok(GrokPollStatus::Pending)
            } else {
                Ok(GrokPollStatus::Failed(message.unwrap_or_else(|| {
                    format!("OAuth failed (HTTP {status})")
                })))
            }
        }
    }
}

pub async fn refresh_access_token(
    client: &reqwest::Client,
    refresh_token: &str,
) -> Result<(String, Option<String>)> {
    let params = [
        ("grant_type", "refresh_token"),
        ("client_id", CLIENT_ID),
        ("refresh_token", refresh_token),
    ];

    let response = client
        .post(TOKEN_URL)
        .header("accept", "application/json")
        .form(&params)
        .send()
        .await?;

    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(Error::Provider(format!(
            "Failed to refresh Grok token (HTTP {status}): {text}"
        )));
    }

    let body: Value = serde_json::from_str(&text)
        .map_err(|e| Error::Provider(format!("invalid Grok refresh token JSON: {e}")))?;

    let new_access = body
        .get("access_token")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Provider("missing access_token in Grok refresh response".into()))?
        .to_string();

    let new_refresh = body
        .get("refresh_token")
        .and_then(Value::as_str)
        .map(String::from);

    Ok((new_access, new_refresh))
}
