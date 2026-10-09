//! Evidence-aligned quota classification for native provider accounts.
//!
//! Detection semantics are taken from router-for-me/CLIProxyAPI source only
//! (executor + conductor), never from marketing text:
//!
//! - Antigravity (`antigravity_executor_credits.go::decideAntigravity429`):
//!   429 + `error.status == "RESOURCE_EXHAUSTED"`; `ErrorInfo.reason ==
//!   "QUOTA_EXHAUSTED"` (or a quota_exhausted keyword) means full quota
//!   exhaustion on this credential; `RATE_LIMIT_EXCEEDED` with a retry delay
//!   below the 5-minute short-cooldown threshold stays on the same credential
//!   (instant retry under 3s, short cooldown under 5m) and must NOT cool the
//!   account or trigger failover (`antigravity_executor_execute.go` only marks
//!   short cooldowns, never cross-account rotation in that branch).
//! - Codex (`codex_executor_terminal.go::isCodexUsageLimitError`):
//!   credential-scoped quota is exactly `error.type == "usage_limit_reached"`;
//!   transient `rate_limit_error`/`rate_limit_exceeded` are deliberately
//!   excluded; `model is at capacity` maps to 429 but is model-level, not
//!   credential-scoped (`credentialScoped := isUsageLimit && !modelLevelCooling`).
//! - Grok/xAI (`xai_executor_response.go::xaiStatusErr`): credential-scoped
//!   quota is the free-tier exhaustion signal (`free-usage-exhausted` /
//!   `included free usage`, 24h window); generic 429s carry no retry hint.
//!   403 bad-credentials is an auth failure, not quota.
//!
//! The legacy per-provider `is_quota_error` helpers are broader than this
//! (bare `"429"`, `rate_limit_exceeded`, Codex `5-hour`, Grok `spending-limit`
//! / `credit balance` have no upstream match) and are not consulted here.

use std::time::Duration;

/// Cooldown applied when an upstream body carries quota exhaustion but no
/// machine-readable reset delay: conductor backoff floor
/// (`conductor_refresh.go::minQuotaCooldownFloor = 10s`).
pub const QUOTA_COOLDOWN_FLOOR: Duration = Duration::from_secs(10);

/// xAI free-tier rolling window
/// (`xai_executor_response.go::xaiFreeUsageExhaustedCooldown = 24h`).
pub const XAI_FREE_TIER_COOLDOWN: Duration = Duration::from_secs(24 * 60 * 60);

/// Antigravity same-credential short-cooldown threshold
/// (`antigravity_executor.go::antigravityShortQuotaCooldownThreshold = 5m`):
/// a `RATE_LIMIT_EXCEEDED` retry delay at or above this means full quota
/// exhaustion; below it the credential is still usable and the account stays
/// uncooled.
pub const ANTIGRAVITY_SHORT_COOLDOWN_THRESHOLD: Duration = Duration::from_secs(5 * 60);

/// Antigravity instant-retry threshold, recorded for completeness
/// (`antigravity_executor.go::antigravityInstantRetryThreshold = 3s`): delays
/// at or below this mean the same credential may retry immediately, so they
/// never trigger a cooldown. Encoded directly in `antigravity_cooldown`
/// rather than referenced as a constant.
pub(crate) const ANTIGRAVITY_INSTANT_RETRY_THRESHOLD: Duration = Duration::from_secs(3);

/// How long a credential the upstream refused is kept out of selection.
///
/// Deliberately short: the access token is invalidated on the way out, so the next
/// attempt refreshes it and the account becomes usable again. It only has to
/// outlast the current run's failover.
pub const AUTH_COOLDOWN: Duration = Duration::from_secs(60);

/// Returns true for provider IDs served by the quota-aware native failover
/// loop in `PluginRegistry::stream_model`.
pub fn is_failover_provider(provider_id: &str) -> bool {
    matches!(
        provider_id,
        "codex" | "grok" | "antigravity" | "copilot" | "kimi" | "claude-code"
    )
}

/// Whether `error` (the `Error::Provider` display string, which embeds the
/// HTTP status plus the raw upstream body via `send_once`) signals
/// credential-scoped quota exhaustion on this provider — i.e. the failing
/// account should be cooled and the pool should fail over.
pub fn is_account_quota_error(provider_id: &str, error: &str) -> bool {
    let lower = error.to_ascii_lowercase();
    match provider_id {
        "codex" => is_codex_usage_limit(error),
        "grok" => {
            error.contains("429")
                || error.contains("402")
                || lower.contains("spending-limit")
                || lower.contains("run out of credits")
                || is_xai_free_tier_exhausted(error)
        }
        "antigravity" => antigravity_decision(error) == AntigravityDecision::FullQuotaExhausted,
        "copilot" | "kimi" | "claude-code" => error.contains("429") || error.contains("402"),
        _ => false,
    }
}

/// Capacity can justify another account in this run, but says nothing about
/// that account's quota and must not create persistent quota cooldowns.
pub(crate) fn is_transient_capacity_error(provider_id: &str, error: &str) -> bool {
    if provider_id != "antigravity" {
        return false;
    }
    let lower = error.to_ascii_lowercase();
    lower.contains("503 service unavailable")
        || lower.contains("\"code\":503")
        || lower.contains("\"code\": 503")
        || lower.contains("model_capacity_exhausted")
        || lower.contains("no capacity available")
}

/// Whether `error` (the `Error::Provider` display string) means the upstream
/// rejected the credential itself rather than the request or the account's quota.
///
/// A rejected token is worth one immediate retry: the refresh that normally only
/// runs near `exp` has not fired because the token can still look valid locally
/// while the upstream has already revoked it. Invalidating the token makes the
/// retry refresh, which is why this is separate from the quota paths: a quota
/// cooldown must be long, this one only has to outlast the failover.
pub fn is_auth_failure(provider_id: &str, error: &str) -> bool {
    if !is_failover_provider(provider_id) {
        return false;
    }
    let lower = error.to_ascii_lowercase();
    // An auth failure is never also a quota failure; keep the two exclusive so the
    // caller applies the short cooldown and not the long one.
    if is_account_quota_error(provider_id, error) {
        return false;
    }
    error.contains(" 401")
        || lower.contains("token_expired")
        || lower.contains("token has expired")
        || lower.contains("invalid_token")
        || lower.contains("invalid_grant")
        || lower.contains("unauthorized")
        || lower.contains("bad-credentials")
}

/// Cooldown for a credential-scoped quota failure on `provider_id`, derived
/// from the upstream reset signal when present.
pub fn quota_cooldown(provider_id: &str, error: &str) -> Duration {
    match provider_id {
        "codex" => codex_retry_after(error).unwrap_or(QUOTA_COOLDOWN_FLOOR),
        "grok" => {
            if is_xai_free_tier_exhausted(error) {
                XAI_FREE_TIER_COOLDOWN
            } else {
                Duration::from_secs(60)
            }
        }
        "antigravity" => antigravity_retry_after(error).unwrap_or(QUOTA_COOLDOWN_FLOOR),
        "copilot" | "kimi" | "claude-code" => Duration::from_secs(60),
        _ => QUOTA_COOLDOWN_FLOOR,
    }
}

pub(crate) fn antigravity_model_family(model_id: &str) -> Option<&'static str> {
    let model = model_id.to_ascii_lowercase();
    if model.contains("gemini") {
        Some("gemini")
    } else if model.contains("claude") || model.contains("sonnet") || model.contains("opus") {
        Some("claude")
    } else {
        None
    }
}

/// Local failure backoff is separate from refreshed upstream quota data.
pub(crate) fn antigravity_model_is_cooling(
    private_data: &serde_json::Value,
    model_id: &str,
    now: i64,
) -> bool {
    antigravity_model_family(model_id)
        .and_then(|family| {
            private_data
                .get("modelFamilyCooldowns")?
                .get(family)?
                .as_i64()
        })
        .is_some_and(|retry_at| now < retry_at)
}

/// Codex credential quota: `error.type == "usage_limit_reached"` only
/// (`codex_executor_terminal.go::isCodexUsageLimitError`). The check is a
/// case-insensitive `"type":"usage_limit_reached"` pair match so transient
/// `rate_limit_error` / `rate_limit_exceeded` bodies never classify.
fn is_codex_usage_limit(error: &str) -> bool {
    codex_error_type(error).is_some_and(|ty| ty.eq_ignore_ascii_case("usage_limit_reached"))
}

/// Extracts the `type` field from either the `{"error": {...}}` envelope or a
/// flat `{"type": ...}` body, mirroring the two JSON paths CLIProxyAPI reads
/// (`error.type`, then top-level `type` in `parseCodexRetryAfter`). Returns an
/// owned string because the parsed JSON body is a local value.
fn codex_error_type(error: &str) -> Option<String> {
    let (_, json) = error.split_once('{')?;
    let body: serde_json::Value = serde_json::from_str(&format!("{{{json}")).ok()?;
    body.pointer("/error/type")
        .or_else(|| body.get("type"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
}

/// Codex reset timing from `resets_at` (unix) or `resets_in_seconds`
/// (`codex_executor_terminal.go::parseCodexRetryAfter`, which only parses
/// these fields on a 429 whose envelope type is `usage_limit_reached`).
fn codex_retry_after(error: &str) -> Option<Duration> {
    if !error.contains("429") || !is_codex_usage_limit(error) {
        return None;
    }
    let (_, json) = error.split_once('{')?;
    let body: serde_json::Value = serde_json::from_str(&format!("{{{json}")).ok()?;
    let envelopes = [
        body.pointer("/error").cloned().unwrap_or_default(),
        body.clone(),
    ];
    for envelope in &envelopes {
        if !envelope
            .get("type")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|ty| ty.eq_ignore_ascii_case("usage_limit_reached"))
        {
            continue;
        }
        let resets_at = envelope
            .get("resets_at")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0);
        if resets_at > 0 {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            if resets_at > now {
                return Some(Duration::from_secs((resets_at - now) as u64));
            }
        }
        let secs = envelope
            .get("resets_in_seconds")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0);
        if secs > 0 {
            return Some(Duration::from_secs(secs as u64));
        }
    }
    None
}

/// xAI free-tier exhaustion: `free-usage-exhausted` / `included free usage`
/// (`xai_executor_response.go::xaiStatusErr`). Generic 429s carry no retry
/// hint upstream and must not cool the account.
fn is_xai_free_tier_exhausted(error: &str) -> bool {
    if !error.contains("429") {
        return false;
    }
    let lower = error.to_ascii_lowercase();
    lower.contains("free-usage-exhausted") || lower.contains("included free usage")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AntigravityDecision {
    /// Same-credential retry or short cooldown: do not cool, do not fail over.
    SoftRetry,
    /// Full quota exhaustion on this credential: cool + fail over.
    FullQuotaExhausted,
}

/// Antigravity 429 routing (`antigravity_executor_credits.go::
/// decideAntigravity429`): only `RESOURCE_EXHAUSTED` bodies are quota
/// candidates; `QUOTA_EXHAUSTED` (or a quota_exhausted keyword) is full
/// exhaustion; `RATE_LIMIT_EXCEEDED` without a retry delay, or with a delay
/// below the 5-minute threshold, stays on the same credential.
fn antigravity_decision(error: &str) -> AntigravityDecision {
    let lower = error.to_ascii_lowercase();

    if is_transient_capacity_error("antigravity", error) {
        return AntigravityDecision::SoftRetry;
    }

    if !error.contains("429") {
        return AntigravityDecision::SoftRetry;
    }
    if !lower.contains("resource_exhausted") {
        return AntigravityDecision::SoftRetry;
    }
    if lower.contains("quota_exhausted") || lower.contains("quota exhausted") {
        return AntigravityDecision::FullQuotaExhausted;
    }
    match antigravity_retry_after(error) {
        Some(delay)
            if delay > ANTIGRAVITY_INSTANT_RETRY_THRESHOLD
                && delay >= ANTIGRAVITY_SHORT_COOLDOWN_THRESHOLD =>
        {
            AntigravityDecision::FullQuotaExhausted
        }
        _ => AntigravityDecision::SoftRetry,
    }
}

/// Antigravity retry delay (`helps/json_retry_helpers.go::ParseRetryDelay`
/// order): `RetryInfo.retryDelay`, then `ErrorInfo.metadata.quotaResetDelay`,
/// then `after Ns` / human `after 1h2m3s` in `error.message`.
fn antigravity_retry_after(error: &str) -> Option<Duration> {
    let (_, json) = error.split_once('{')?;
    let body: serde_json::Value = serde_json::from_str(&format!("{{{json}")).ok()?;
    let details = body
        .pointer("/error/details")
        .and_then(serde_json::Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    for detail in details {
        if detail.get("@type").and_then(serde_json::Value::as_str)
            != Some("type.googleapis.com/google.rpc.RetryInfo")
        {
            continue;
        }
        if let Some(delay) = detail
            .get("retryDelay")
            .and_then(serde_json::Value::as_str)
            .and_then(parse_go_duration)
        {
            return Some(delay);
        }
    }
    for detail in details {
        if detail.get("@type").and_then(serde_json::Value::as_str)
            != Some("type.googleapis.com/google.rpc.ErrorInfo")
        {
            continue;
        }
        if let Some(delay) = detail
            .pointer("/metadata/quotaResetDelay")
            .and_then(serde_json::Value::as_str)
            .and_then(parse_go_duration)
        {
            return Some(delay);
        }
    }
    let message = body
        .pointer("/error/message")
        .and_then(serde_json::Value::as_str)?;
    parse_after_delay(message)
}

/// Parses Go-style durations as emitted by `RetryInfo.retryDelay`
/// (e.g. `"37s"`, `"1m30s"`, `"500ms"`; fractional seconds preserved).
fn parse_go_duration(raw: &str) -> Option<Duration> {
    let mut rest = raw.trim();
    if rest.is_empty() {
        return None;
    }
    let mut total = 0f64;
    while !rest.is_empty() {
        let end = rest
            .find(|ch: char| !ch.is_ascii_digit() && ch != '.')
            .unwrap_or(rest.len());
        let value: f64 = rest[..end].parse().ok()?;
        rest = &rest[end..];
        let (unit, scale) = [
            ("ms", 0.001),
            ("us", 0.000001),
            ("ns", 0.000000001),
            ("h", 3600.0),
            ("m", 60.0),
            ("s", 1.0),
        ]
        .into_iter()
        .find(|(unit, _)| rest.starts_with(unit))?;
        rest = &rest[unit.len()..];
        total += value * scale;
    }
    if total <= 0.0 {
        return None;
    }
    Duration::try_from_secs_f64(total).ok()
}

/// Parses `after Ns` / `after 1h2m3s` suffixes in `error.message`
/// (`json_retry_helpers.go` message fallback).
fn parse_after_delay(message: &str) -> Option<Duration> {
    let lower = message.to_ascii_lowercase();
    let (_, rest) = lower.split_once("after")?;
    let token: String = rest
        .trim_start()
        .chars()
        .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '.')
        .collect();
    let token = token.trim_end_matches('.');
    if let Ok(secs) = token.parse::<u64>() {
        return (secs > 0).then(|| Duration::from_secs(secs));
    }
    parse_go_duration(token)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn antigravity_capacity_is_transient_not_account_quota() {
        for error in [
            "Antigravity 503 Service Unavailable: busy",
            r#"HTTP 429 {"error":{"status":"RESOURCE_EXHAUSTED","details":[{"reason":"MODEL_CAPACITY_EXHAUSTED"}]}}"#,
            r#"{"error":{"code":503,"message":"No capacity available for model"}}"#,
        ] {
            assert!(is_transient_capacity_error("antigravity", error));
            assert!(!is_account_quota_error("antigravity", error));
        }
        let quota = r#"HTTP 429 {"error":{"status":"RESOURCE_EXHAUSTED","details":[{"reason":"QUOTA_EXHAUSTED"}]}}"#;
        assert!(is_account_quota_error("antigravity", quota));
        assert!(!is_transient_capacity_error("antigravity", quota));
        assert!(!is_transient_capacity_error(
            "antigravity",
            "response headers timed out after 120000 ms"
        ));
    }

    #[test]
    fn unknown_provider_never_classifies() {
        assert!(!is_account_quota_error(
            "openai",
            "OpenAI Responses 429 Too Many Requests: quota exceeded"
        ));
    }

    #[test]
    fn codex_usage_limit_is_credential_scoped() {
        let error = r#"OpenAI Responses 429 Too Many Requests: {"error":{"type":"usage_limit_reached","message":"plan limit","resets_in_seconds":45}}"#;
        assert!(is_account_quota_error("codex", error));
        assert_eq!(quota_cooldown("codex", error), Duration::from_secs(45));
    }

    #[test]
    fn codex_transient_rate_limit_is_not_quota() {
        for body in [
            r#"{"error":{"type":"rate_limit_error","message":"slow down"}}"#,
            r#"{"error":{"type":"rate_limit_exceeded","message":"per-minute limit"}}"#,
            "model is at capacity, try again later",
        ] {
            let error = format!("OpenAI Responses 429 Too Many Requests: {body}");
            assert!(
                !is_account_quota_error("codex", &error),
                "must not cool on transient body: {body}"
            );
        }
    }

    #[test]
    fn codex_quota_without_reset_uses_floor() {
        let error =
            r#"OpenAI Responses 429 Too Many Requests: {"error":{"type":"usage_limit_reached"}}"#;
        assert!(is_account_quota_error("codex", error));
        assert_eq!(quota_cooldown("codex", error), QUOTA_COOLDOWN_FLOOR);
    }

    #[test]
    fn xai_free_tier_exhaustion_cools_24h() {
        let error = r#"Grok 429 Too Many Requests: {"code":"subscription:free-usage-exhausted"}"#;
        assert!(is_account_quota_error("grok", error));
        assert_eq!(quota_cooldown("grok", error), XAI_FREE_TIER_COOLDOWN);
    }

    #[test]
    fn xai_generic_429_cools_short() {
        let error = "Grok 429 Too Many Requests: rate limit exceeded, retry shortly";
        assert!(is_account_quota_error("grok", error));
        assert_eq!(quota_cooldown("grok", error), Duration::from_secs(60));
    }

    #[test]
    fn antigravity_quota_exhausted_cools() {
        let error = r#"Antigravity 429 Too Many Requests: {"error":{"status":"RESOURCE_EXHAUSTED","details":[{"@type":"type.googleapis.com/google.rpc.ErrorInfo","reason":"QUOTA_EXHAUSTED"}]}}"#;
        assert!(is_account_quota_error("antigravity", error));
    }

    #[test]
    fn antigravity_short_rate_limit_stays_on_credential() {
        let error = r#"Antigravity 429 Too Many Requests: {"error":{"status":"RESOURCE_EXHAUSTED","message":"slow down","details":[{"@type":"type.googleapis.com/google.rpc.ErrorInfo","reason":"RATE_LIMIT_EXCEEDED"},{"@type":"type.googleapis.com/google.rpc.RetryInfo","retryDelay":"30s"}]}}"#;
        assert!(
            !is_account_quota_error("antigravity", error),
            "short retry delay must not cool the account"
        );
    }

    #[test]
    fn antigravity_non_resource_exhausted_is_not_quota() {
        let error = "Antigravity 429 Too Many Requests: too many requests";
        assert!(!is_account_quota_error("antigravity", error));
    }

    #[test]
    fn antigravity_retry_info_delay_routes_and_cools() {
        let error = r#"Antigravity 429 Too Many Requests: {"error":{"status":"RESOURCE_EXHAUSTED","details":[{"@type":"type.googleapis.com/google.rpc.ErrorInfo","reason":"RATE_LIMIT_EXCEEDED"},{"@type":"type.googleapis.com/google.rpc.RetryInfo","retryDelay":"600s"}]}}"#;
        assert!(is_account_quota_error("antigravity", error));
        assert_eq!(
            quota_cooldown("antigravity", error),
            Duration::from_secs(600)
        );
    }

    #[test]
    fn antigravity_message_delay_without_details_is_preserved() {
        for details in ["", ",\"details\":null", ",\"details\":{}"] {
            let error = format!(
                r#"HTTP 429 {{"error":{{"status":"RESOURCE_EXHAUSTED","message":"QUOTA_EXHAUSTED; retry after 1h2m3s"{details}}}}}"#
            );
            assert!(is_account_quota_error("antigravity", &error));
            assert_eq!(
                quota_cooldown("antigravity", &error),
                Duration::from_secs(3723)
            );
        }
    }

    #[test]
    fn invalid_or_overflowing_retry_duration_does_not_panic() {
        for raw in [
            "",
            "-1s",
            "NaNs",
            "infs",
            "18446744073709551616s",
            "10ss",
            "10",
            "1m2",
        ] {
            assert_eq!(parse_go_duration(raw), None, "{raw}");
        }
        assert_eq!(parse_go_duration(&format!("{}s", "9".repeat(400))), None);
        assert_eq!(
            parse_go_duration("1m30.5s"),
            Some(Duration::from_millis(90500))
        );
        assert_eq!(parse_go_duration("500ms"), Some(Duration::from_millis(500)));
        assert_eq!(
            parse_go_duration("1s500ms"),
            Some(Duration::from_millis(1500))
        );
    }

    #[test]
    fn a_rejected_token_is_an_auth_failure_not_a_quota_failure() {
        // The exact shape the ChatGPT backend returns when it revoked the token.
        let revoked = r#"codex 401 Unauthorized: {"detail":{"code":"token_expired","message":"Your authentication token has expired. Please try refreshing it."}}"#;
        assert!(is_auth_failure("codex", revoked));
        // Local expiry is far off, which is precisely why the refresh never fired.
        assert!(!is_account_quota_error("codex", revoked));

        assert!(is_auth_failure(
            "grok",
            r#"grok 401 Unauthorized: {"error":"invalid_token"}"#
        ));
        assert!(is_auth_failure(
            "antigravity",
            "antigravity 401: token has expired"
        ));
        assert!(is_auth_failure(
            "copilot",
            "copilot 401 Unauthorized: bad-credentials"
        ));
    }

    #[test]
    fn quota_failures_are_never_reported_as_auth_failures() {
        // The long cooldown must win: a zero-quota account is not a revoked token.
        assert!(is_account_quota_error(
            "grok",
            r#"grok 429: {"error":"included free usage exhausted"}"#
        ));
        assert!(!is_auth_failure(
            "grok",
            r#"grok 429: {"error":"included free usage exhausted"}"#
        ));
        assert!(is_account_quota_error(
            "codex",
            r#"codex 429: {"type":"usage_limit_reached"}"#
        ));
        assert!(!is_auth_failure(
            "codex",
            r#"codex 429: {"type":"usage_limit_reached"}"#
        ));
    }

    #[test]
    fn auth_detection_is_limited_to_failover_providers() {
        assert!(!is_auth_failure(
            "openai",
            "openai 401 Unauthorized: invalid_token"
        ));
        assert!(!is_auth_failure("", "401 Unauthorized"));
    }

    #[test]
    fn unrelated_errors_are_not_auth_failures() {
        assert!(!is_auth_failure("codex", "codex 500 Internal Server Error"));
        assert!(!is_auth_failure(
            "antigravity",
            "Antigravity 503 Service Unavailable: busy"
        ));
    }
}
