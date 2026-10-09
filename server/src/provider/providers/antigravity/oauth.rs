use serde_json::Value;

use crate::{Error, Result};

pub const CLIENT_ID: &str =
    concat!("1071006060591", "-tmhssin2h21lcre235vtolojh4g403ep", ".apps.googleusercontent.com");
pub const CLIENT_SECRET: &str = concat!("GOCSPX", "-", "K58FWR486LdLJ1mLB8sXC4z6qDAf");
pub const GOOGLE_AUTHORIZATION_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
pub const GOOGLE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";

pub const SCOPES: &[&str] = &[
    "https://www.googleapis.com/auth/cloud-platform",
    "https://www.googleapis.com/auth/userinfo.email",
    "https://www.googleapis.com/auth/userinfo.profile",
    "https://www.googleapis.com/auth/cclog",
    "https://www.googleapis.com/auth/experimentsandconfigs",
];

pub fn build_authorization_url(redirect_uri: &str, state: &str, code_challenge: &str) -> String {
    let scope_str = SCOPES.join(" ");
    let mut url = reqwest::Url::parse(GOOGLE_AUTHORIZATION_URL).unwrap();
    url.query_pairs_mut()
        .append_pair("client_id", CLIENT_ID)
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("response_type", "code")
        .append_pair("scope", &scope_str)
        .append_pair("state", state)
        .append_pair("code_challenge", code_challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("access_type", "offline")
        .append_pair("prompt", "consent");
    url.to_string()
}

pub async fn exchange_code(
    client: &reqwest::Client,
    code: &str,
    code_verifier: &str,
    redirect_uri: &str,
) -> Result<(String, Option<String>, Option<i64>)> {
    let params = [
        ("client_id", CLIENT_ID),
        ("client_secret", CLIENT_SECRET),
        ("code", code),
        ("code_verifier", code_verifier),
        ("grant_type", "authorization_code"),
        ("redirect_uri", redirect_uri),
    ];

    let response = client
        .post(GOOGLE_TOKEN_URL)
        .header("accept", "application/json")
        .form(&params)
        .send()
        .await?;

    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(Error::Provider(format!(
            "Google OAuth token exchange failed (HTTP {status}): {text}"
        )));
    }

    let body: Value = serde_json::from_str(&text)
        .map_err(|e| Error::Provider(format!("invalid token JSON: {e}")))?;

    let access = body
        .get("access_token")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Provider("missing access_token in Google response".into()))?
        .to_string();

    let refresh = body
        .get("refresh_token")
        .and_then(Value::as_str)
        .map(String::from);
    let expires_in = body.get("expires_in").and_then(Value::as_i64);

    Ok((access, refresh, expires_in))
}

pub async fn refresh_token(
    client: &reqwest::Client,
    refresh_token: &str,
) -> Result<(String, Option<String>, Option<i64>)> {
    let params = [
        ("client_id", CLIENT_ID),
        ("client_secret", CLIENT_SECRET),
        ("refresh_token", refresh_token),
        ("grant_type", "refresh_token"),
    ];

    let response = client
        .post(GOOGLE_TOKEN_URL)
        .header("accept", "application/json")
        .form(&params)
        .send()
        .await?;

    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(Error::Provider(format!(
            "Google token refresh failed (HTTP {status}): {text}"
        )));
    }

    let body: Value = serde_json::from_str(&text)
        .map_err(|e| Error::Provider(format!("invalid refresh JSON: {e}")))?;

    let access = body
        .get("access_token")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Provider("missing access_token in Google refresh".into()))?
        .to_string();

    let new_refresh = body
        .get("refresh_token")
        .and_then(Value::as_str)
        .map(String::from);
    let expires_in = body.get("expires_in").and_then(Value::as_i64);

    Ok((access, new_refresh, expires_in))
}
