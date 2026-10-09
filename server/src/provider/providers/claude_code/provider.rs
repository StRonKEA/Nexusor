pub const ANTHROPIC_MESSAGES_URL: &str = "https://api.anthropic.com/v1/messages";

pub fn request_headers(access_token: &str) -> Vec<(String, String)> {
    vec![
        ("Authorization".into(), format!("Bearer {access_token}")),
        (
            "anthropic-beta".into(),
            "claude-code-20250219,oauth-2025-04-20".into(),
        ),
        ("X-App".into(), "cli".into()),
        (
            "User-Agent".into(),
            "claude-cli/2.1.280 (external, cli)".into(),
        ),
        ("X-Stainless-Retry-Count".into(), "0".into()),
        ("X-Stainless-Runtime".into(), "node".into()),
        ("X-Stainless-Lang".into(), "js".into()),
        ("X-Stainless-Package-Version".into(), "0.74.0".into()),
    ]
}
