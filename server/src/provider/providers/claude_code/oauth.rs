use crate::{Error, Result};
use serde_json::Value;
use std::path::PathBuf;

pub const CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
pub const AUTHORIZE_URL: &str = "https://claude.ai/oauth/authorize";
pub const TOKEN_URL: &str = "https://api.anthropic.com/v1/oauth/token";
pub const SCOPES: &str = "org:create_api_key user:profile user:inference";

pub fn build_authorization_url(redirect_uri: &str, state: &str, code_challenge: &str) -> String {
    let mut url = reqwest::Url::parse(AUTHORIZE_URL).unwrap();
    url.query_pairs_mut()
        .append_pair("code", "true")
        .append_pair("client_id", CLIENT_ID)
        .append_pair("response_type", "code")
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("scope", SCOPES)
        .append_pair("code_challenge", code_challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", state);
    url.to_string()
}

pub async fn exchange_code(
    client: &reqwest::Client,
    code: &str,
    code_verifier: &str,
    redirect_uri: &str,
    state: &str,
) -> Result<(String, String, i64, Option<String>)> {
    let response = client
        .post(TOKEN_URL)
        .header(reqwest::header::ACCEPT, "application/json")
        .json(&serde_json::json!({
            "grant_type": "authorization_code",
            "client_id": CLIENT_ID,
            "code": code,
            "state": state,
            "redirect_uri": redirect_uri,
            "code_verifier": code_verifier,
        }))
        .send()
        .await?;

    if !response.status().is_success() {
        let text = response.text().await.unwrap_or_default();
        return Err(Error::Provider(format!(
            "Claude Code token exchange failed: {text}"
        )));
    }

    let body: Value = response.json().await?;
    let access_token = body
        .get("access_token")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Provider("missing access_token in Claude Code response".into()))?
        .to_string();
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
    let email = body
        .get("account")
        .and_then(|a| a.get("email_address"))
        .and_then(Value::as_str)
        .map(ToString::to_string);

    Ok((access_token, refresh_token, expires_at_ms, email))
}

pub async fn refresh_access_token(
    client: &reqwest::Client,
    refresh_token: &str,
) -> Result<(String, String, i64)> {
    let response = client
        .post(TOKEN_URL)
        .header(reqwest::header::ACCEPT, "application/json")
        .json(&serde_json::json!({
            "grant_type": "refresh_token",
            "client_id": CLIENT_ID,
            "refresh_token": refresh_token,
        }))
        .send()
        .await?;

    if !response.status().is_success() {
        let text = response.text().await.unwrap_or_default();
        return Err(Error::Provider(format!(
            "Claude Code refresh failed: {text}"
        )));
    }

    let body: Value = response.json().await?;
    let access_token = body
        .get("access_token")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            Error::Provider("missing access_token in Claude Code refresh response".into())
        })?
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

pub fn detect_local_credentials() -> Option<(String, String, i64)> {
    let home = dirs::home_dir()?;
    let path: PathBuf = home.join(".claude").join(".credentials.json");
    if !path.exists() {
        return None;
    }
    let data = std::fs::read_to_string(path).ok()?;
    let val: Value = serde_json::from_str(&data).ok()?;
    let oauth = val.get("claudeAiOauth")?;
    let access = oauth
        .get("accessToken")
        .and_then(Value::as_str)?
        .to_string();
    let refresh = oauth
        .get("refreshToken")
        .and_then(Value::as_str)?
        .to_string();
    let expires = oauth.get("expiresAt").and_then(Value::as_i64).unwrap_or(0);
    Some((access, refresh, expires))
}
