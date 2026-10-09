pub const KIMI_CHAT_URL: &str = "https://api.kimi.com/coding/v1/chat/completions";

pub fn request_headers(access_token: &str) -> Vec<(String, String)> {
    vec![
        ("Authorization".into(), format!("Bearer {access_token}")),
        (
            "User-Agent".into(),
            format!("KimiCLI/{}", super::oauth::KIMI_CLI_VERSION),
        ),
        ("X-Msh-Platform".into(), "kimi_code_cli".into()),
        (
            "X-Msh-Version".into(),
            super::oauth::KIMI_CLI_VERSION.into(),
        ),
    ]
}
