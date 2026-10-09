use std::collections::BTreeMap;

/// OAuth (Grok CLI) tokens are rejected by api.x.ai with 403; the CLI chat proxy is
/// the endpoint that accepts them. Mirrors CLIProxyAPI `CLIChatProxyBaseURL`.
pub const COMPLETIONS_URL: &str = "https://cli-chat-proxy.grok.com/v1/chat/completions";

/// Grok CLI client version the chat proxy expects; older values answer 426.
pub const CLIENT_VERSION: &str = "0.2.120";

pub fn request_headers(access_token: &str) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("authorization".into(), format!("Bearer {access_token}")),
        ("accept".into(), "application/json".into()),
        // Without the Grok CLI identity headers the chat proxy rejects every
        // request with 426 Upgrade Required.
        ("x-xai-token-auth".into(), "xai-grok-cli".into()),
        ("x-grok-client-version".into(), CLIENT_VERSION.into()),
        ("x-grok-client-identifier".into(), "grok-shell".into()),
        (
            "x-authenticateresponse".into(),
            "authenticate-response".into(),
        ),
        (
            "user-agent".into(),
            format!("xai-grok-workspace/{CLIENT_VERSION}"),
        ),
    ])
}
