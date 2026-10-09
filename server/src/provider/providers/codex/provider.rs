use std::collections::BTreeMap;

pub const RESPONSES_URL: &str = "https://chatgpt.com/backend-api/codex/responses";

pub fn request_headers(access_token: &str, cache_key: Option<&str>) -> BTreeMap<String, String> {
    let mut headers = BTreeMap::from([
        ("authorization".into(), format!("Bearer {access_token}")),
        ("originator".into(), "codex_cli_rs".into()),
    ]);
    if let Some(account_id) = super::models::chat_gpt_account_id(access_token) {
        headers.insert("ChatGPT-Account-Id".into(), account_id);
    }
    if let Some(cache_key) = cache_key.filter(|value| !value.is_empty()) {
        for name in ["session-id", "thread-id", "x-client-request-id"] {
            headers.insert(name.into(), cache_key.into());
        }
    }
    headers
}
