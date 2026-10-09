//! Cloud Code Antigravity streaming constants and helpers.
//!
//! Endpoints and OAuth client IDs match CLIProxyAPI antigravity_executor
//! and this crate's usage.rs ANTIGRAVITY_ENDPOINTS (not cloudaicompanion).

use std::collections::BTreeMap;

pub const STREAM_PATH: &str = "/v1internal:streamGenerateContent?alt=sse";

/// Generation traffic defaults to the daily host: prod rejects consumer accounts
/// with 429 RESOURCE_EXHAUSTED even when quota remains. Mirrors CLIProxyAPI
/// `resolveAntigravityRequestBaseURL`, which also defaults to daily.
/// Quota and project lookups keep using [`ANTIGRAVITY_ENDPOINTS`] (prod first).
pub const STREAM_ENDPOINTS: &[&str] = &[
    "https://daily-cloudcode-pa.googleapis.com",
    "https://cloudcode-pa.googleapis.com",
    "https://daily-cloudcode-pa.sandbox.googleapis.com",
];

/// Primary stream URL (daily). Prefer [`stream_urls`] for fallback.
pub fn primary_stream_url() -> String {
    format!("{}{STREAM_PATH}", STREAM_ENDPOINTS[0])
}

pub fn stream_urls() -> Vec<String> {
    STREAM_ENDPOINTS
        .iter()
        .map(|base| format!("{base}{STREAM_PATH}"))
        .collect()
}

pub fn request_headers(access_token: &str) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("authorization".into(), format!("Bearer {access_token}")),
        ("content-type".into(), "application/json".into()),
        ("accept".into(), "text/event-stream".into()),
        (
            "user-agent".into(),
            "Antigravity/4.3.0 (Macintosh; Intel Mac OS X 10_15_7) Chrome/132.0.6834.160 Electron/39.2.3"
                .into(),
        ),
        ("x-client-name".into(), "antigravity".into()),
        ("x-client-version".into(), "4.3.0".into()),
    ])
}

pub async fn send_wakeup_ping(
    client: &reqwest::Client,
    access_token: &str,
    project_id: &str,
) -> crate::Result<()> {
    let body = serde_json::json!({
        "project": project_id,
        "model": "gemini-2.5-flash",
        "contents": [
            {
                "role": "user",
                "parts": [{ "text": "ping" }]
            }
        ]
    });
    for url in stream_urls() {
        let mut req = client.post(&url).json(&body);
        for (k, v) in request_headers(access_token) {
            req = req.header(k, v);
        }
        if let Ok(res) = req.send().await {
            if res.status().is_success() {
                return Ok(());
            }
        }
    }
    Ok(())
}
