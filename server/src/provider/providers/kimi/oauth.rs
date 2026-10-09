use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const CLIENT_ID: &str = "17e5f671-d194-4dfb-9706-5516cb48c098";
pub const OAUTH_HOST: &str = "https://auth.kimi.com";
pub const KIMI_CLI_VERSION: &str = "0.14.0";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KimiDeviceSession {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub verification_uri_complete: Option<String>,
    pub expires_in_sec: u64,
    pub interval_sec: u64,
}

pub struct KimiBeginResult {
    pub session: Value,
    pub user_code: String,
    pub verification_url: String,
    pub verification_url_complete: Option<String>,
    pub poll_interval_ms: u64,
    pub expires_at_ms: i64,
}

pub enum KimiPollStatus {
    Pending,
    SlowDown,
    Denied,
    Failed(String),
    Success {
        access_token: String,
        refresh_token: String,
        expires_at_ms: i64,
        user_id: Option<String>,
        email: Option<String>,
    },
}

fn kimi_headers() -> reqwest::header::HeaderMap {
    let mut headers = reqwest::header::HeaderMap::new();
    if let Ok(v) = format!("KimiCLI/{KIMI_CLI_VERSION}").parse() {
        headers.insert(reqwest::header::USER_AGENT, v);
    }
    if let Ok(v) = "kimi_code_cli".parse() {
        headers.insert("X-Msh-Platform", v);
    }
    if let Ok(v) = KIMI_CLI_VERSION.parse() {
        headers.insert("X-Msh-Version", v);
    }
    if let Ok(v) = "Nexusor".parse() {
        headers.insert("X-Msh-Device-Name", v);
    }
    if let Ok(v) = "Windows".parse() {
        headers.insert("X-Msh-Device-Model", v);
    }
    headers
}

pub async fn begin_device_flow(client: &reqwest::Client) -> Result<KimiBeginResult> {
    let url = format!("{OAUTH_HOST}/api/oauth/device_authorization");
    let response = client
        .post(&url)
        .headers(kimi_headers())
        .form(&[("client_id", CLIENT_ID)])
        .send()
        .await?;

    if !response.status().is_success() {
        let err = response.text().await.unwrap_or_default();
        return Err(Error::Provider(format!("Kimi device auth failed: {err}")));
    }

    let body: Value = response.json().await?;
    let user_code = body
        .get("user_code")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Provider("missing user_code in Kimi response".into()))?
        .to_string();
    let device_code = body
        .get("device_code")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Provider("missing device_code in Kimi response".into()))?
        .to_string();
    let verification_uri = body
        .get("verification_uri")
        .and_then(Value::as_str)
        .unwrap_or("https://auth.kimi.com")
        .to_string();
    let verification_uri_complete = body
        .get("verification_uri_complete")
        .and_then(Value::as_str)
        .map(ToString::to_string);
    let expires_in_sec = body
        .get("expires_in")
        .and_then(Value::as_u64)
        .unwrap_or(900);
    let interval_sec = body
        .get("interval")
        .and_then(Value::as_u64)
        .unwrap_or(5)
        .max(1);

    let session = KimiDeviceSession {
        device_code,
        user_code: user_code.clone(),
        verification_uri: verification_uri.clone(),
        verification_uri_complete: verification_uri_complete.clone(),
        expires_in_sec,
        interval_sec,
    };

    Ok(KimiBeginResult {
        session: serde_json::to_value(session)?,
        user_code,
        verification_url: verification_uri,
        verification_url_complete: verification_uri_complete,
        poll_interval_ms: interval_sec * 1000,
        expires_at_ms: chrono::Utc::now().timestamp_millis() + (expires_in_sec as i64 * 1000),
    })
}

pub async fn poll_device_flow(
    client: &reqwest::Client,
    session: &KimiDeviceSession,
) -> Result<KimiPollStatus> {
    let url = format!("{OAUTH_HOST}/api/oauth/token");
    let response = client
        .post(&url)
        .headers(kimi_headers())
        .form(&[
            ("client_id", CLIENT_ID),
            ("device_code", session.device_code.as_str()),
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
        ])
        .send()
        .await?;

    let body: Value = response.json().await.unwrap_or(Value::Null);
    if let Some(access_token) = body.get("access_token").and_then(Value::as_str) {
        let refresh_token = body
            .get("refresh_token")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let expires_in = body
            .get("expires_in")
            .and_then(Value::as_i64)
            .unwrap_or(3600);
        let expires_at_ms =
            chrono::Utc::now().timestamp_millis() + (expires_in * 1000) - (5 * 60 * 1000);

        let (user_id, email) = parse_jwt_claims(access_token);
        return Ok(KimiPollStatus::Success {
            access_token: access_token.to_string(),
            refresh_token,
            expires_at_ms,
            user_id,
            email,
        });
    }

    let error = body.get("error").and_then(Value::as_str).unwrap_or("");
    match error {
        "authorization_pending" => Ok(KimiPollStatus::Pending),
        "slow_down" => Ok(KimiPollStatus::SlowDown),
        "access_denied" | "expired_token" => Ok(KimiPollStatus::Denied),
        other => Ok(KimiPollStatus::Failed(other.to_string())),
    }
}

pub async fn refresh_access_token(
    client: &reqwest::Client,
    refresh_token: &str,
) -> Result<(String, String, i64)> {
    let url = format!("{OAUTH_HOST}/api/oauth/token");
    let response = client
        .post(&url)
        .headers(kimi_headers())
        .form(&[
            ("client_id", CLIENT_ID),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
        ])
        .send()
        .await?;

    if !response.status().is_success() {
        let text = response.text().await.unwrap_or_default();
        return Err(Error::Provider(format!("Kimi refresh failed: {text}")));
    }

    let body: Value = response.json().await?;
    let access_token = body
        .get("access_token")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Provider("missing access_token in Kimi refresh response".into()))?
        .to_string();
    let new_refresh = body
        .get("refresh_token")
        .and_then(Value::as_str)
        .unwrap_or(refresh_token)
        .to_string();
    let expires_in = body
        .get("expires_in")
        .and_then(Value::as_i64)
        .unwrap_or(3600);
    let expires_at_ms =
        chrono::Utc::now().timestamp_millis() + (expires_in * 1000) - (5 * 60 * 1000);

    Ok((access_token, new_refresh, expires_at_ms))
}

fn parse_jwt_claims(token: &str) -> (Option<String>, Option<String>) {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return (None, None);
    }
    use base64::Engine;
    let decoded = match base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(parts[1]) {
        Ok(d) => d,
        Err(_) => match base64::engine::general_purpose::STANDARD.decode(parts[1]) {
            Ok(d) => d,
            Err(_) => return (None, None),
        },
    };
    let value: Value = match serde_json::from_slice(&decoded) {
        Ok(v) => v,
        Err(_) => return (None, None),
    };
    let user_id = value
        .get("user_id")
        .or_else(|| value.get("sub"))
        .and_then(Value::as_str)
        .map(ToString::to_string);
    let email = value
        .get("email")
        .and_then(Value::as_str)
        .map(|e| e.to_lowercase());
    (user_id, email)
}
