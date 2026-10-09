use serde_json::Value;

pub async fn fetch_user_email(client: &reqwest::Client, access_token: &str) -> Option<String> {
    let response = client
        .get("https://www.googleapis.com/oauth2/v1/userinfo?alt=json")
        .header("authorization", format!("Bearer {access_token}"))
        .header("accept", "application/json")
        .send()
        .await
        .ok()?;

    if !response.status().is_success() {
        return None;
    }

    let text = response.text().await.ok()?;
    let json: Value = serde_json::from_str(&text).ok()?;
    json.get("email").and_then(Value::as_str).map(String::from)
}
