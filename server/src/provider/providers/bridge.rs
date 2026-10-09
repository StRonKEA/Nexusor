use crate::{
    model::ModelInvocation,
    plugin::PluginRegistry,
    provider::{
        providers::{
            antigravity::{self, AntigravityAccountData, AntigravityCloudCodeProvider},
            claude_code,
            codex::{self, AccountData as CodexAccountData},
            copilot,
            grok::{self, GrokAccountData},
            kimi,
        },
        CallRecorder, Provider, ProviderStream,
    },
    Error, Result,
};

/// Arguments for [`stream_native_provider`], grouped so the native-provider
/// entry point stays under clippy's argument-count lint.
pub struct NativeStreamRequest<'a> {
    pub plugin_id: &'a str,
    pub provider_id: &'a str,
    pub upstream_model_id: &'a str,
    pub invocation: ModelInvocation,
    pub cancellation: tokio_util::sync::CancellationToken,
    pub registry: &'a PluginRegistry,
    pub recorder: Option<CallRecorder>,
    pub excluded_ids: &'a [String],
    /// If set, only these account/resource IDs will be used for this slot.
    pub allowed_account_ids: Option<&'a [String]>,
}

pub async fn stream_native_provider(
    request: NativeStreamRequest<'_>,
) -> Result<Option<(String, ProviderStream)>> {
    let NativeStreamRequest {
        plugin_id,
        provider_id,
        upstream_model_id,
        mut invocation,
        cancellation,
        registry,
        recorder,
        excluded_ids,
        allowed_account_ids,
    } = request;
    invocation.request.model.model_id = upstream_model_id.to_owned();

    match (plugin_id, provider_id) {
        ("dev.nexusor.examples.codex-auth" | "dev.cursorbyok.examples.codex-auth", "codex") => {
            let record = registry
                .select_resource_with_strategy(
                    plugin_id,
                    codex::RESOURCE_TYPE,
                    excluded_ids,
                    allowed_account_ids,
                    Some(&invocation.conversation_id),
                    invocation.slot_strategy,
                )
                .await?;
            let mut account = CodexAccountData::from_record(&record).ok_or_else(|| {
                Error::Provider("invalid Codex account credentials in record".into())
            })?;

            let client = registry.client().await?;

            if codex::ensure_fresh_account(&client, &mut account).await? {
                registry
                    .persist_codex_account(plugin_id, codex::RESOURCE_TYPE, &record.id, &account)
                    .await?;
            }

            let mut config = crate::config::ProviderConfig {
                // Codex kind forces `store: false`, which the ChatGPT backend requires.
                kind: crate::config::ProviderKind::Codex,
                request_url: codex::RESPONSES_URL.into(),
                api_key: account.access_token.clone(),
                custom_headers: reqwest::header::HeaderMap::new(),
                max_output_tokens: None,
                request_timeout: std::time::Duration::from_secs(3600),
                allowed_body_fields: None,
            };

            for (k, v) in codex::request_headers(&account.access_token, None) {
                if let (Ok(h_name), Ok(h_val)) = (
                    reqwest::header::HeaderName::from_bytes(k.as_bytes()),
                    reqwest::header::HeaderValue::from_str(&v),
                ) {
                    config.custom_headers.insert(h_name, h_val);
                }
            }

            let provider = crate::provider::OpenAiResponsesProvider::new(client, config)
                .with_recorder(recorder);

            Ok(Some((
                record.id.clone(),
                provider.stream(invocation, cancellation),
            )))
        }

        ("dev.nexusor.examples.grok-auth" | "dev.cursorbyok.examples.grok-auth", "grok") => {
            let record = registry
                .select_resource_with_strategy(
                    plugin_id,
                    grok::RESOURCE_TYPE,
                    excluded_ids,
                    allowed_account_ids,
                    Some(&invocation.conversation_id),
                    invocation.slot_strategy,
                )
                .await?;
            let mut account = GrokAccountData::from_record(&record).ok_or_else(|| {
                Error::Provider("invalid Grok account credentials in record".into())
            })?;

            let client = registry.client().await?;

            if grok::ensure_fresh_account(&client, &mut account).await? {
                registry
                    .persist_grok_account(plugin_id, grok::RESOURCE_TYPE, &record.id, &account)
                    .await?;
            }

            let mut config = crate::config::ProviderConfig {
                kind: crate::config::ProviderKind::OpenAiChat,
                request_url: grok::COMPLETIONS_URL.into(),
                api_key: account.access_token.clone(),
                custom_headers: reqwest::header::HeaderMap::new(),
                max_output_tokens: None,
                request_timeout: std::time::Duration::from_secs(3600),
                allowed_body_fields: None,
            };

            for (k, v) in grok::request_headers(&account.access_token) {
                if let (Ok(h_name), Ok(h_val)) = (
                    reqwest::header::HeaderName::from_bytes(k.as_bytes()),
                    reqwest::header::HeaderValue::from_str(&v),
                ) {
                    config.custom_headers.insert(h_name, h_val);
                }
            }

            let provider =
                crate::provider::OpenAiChatProvider::new(client, config).with_recorder(recorder);

            Ok(Some((
                record.id.clone(),
                provider.stream(invocation, cancellation),
            )))
        }

        ("dev.nexusor.plugins.github-copilot", "copilot") => {
            if copilot::models::rejects_reasoning_effort(upstream_model_id) {
                invocation.request.model.reasoning.enabled = false;
                invocation.request.model.reasoning.effort = None;
            }
            let record = registry
                .select_resource_with_strategy(
                    plugin_id,
                    copilot::RESOURCE_TYPE,
                    excluded_ids,
                    allowed_account_ids,
                    Some(&invocation.conversation_id),
                    invocation.slot_strategy,
                )
                .await?;
            let mut account =
                copilot::CopilotAccountData::from_record(&record).ok_or_else(|| {
                    Error::Provider("invalid GitHub Copilot account credentials in record".into())
                })?;

            let client = registry.client().await?;

            if copilot::ensure_fresh_account(&client, &mut account).await? {
                registry
                    .persist_copilot_account(
                        plugin_id,
                        copilot::RESOURCE_TYPE,
                        &record.id,
                        &account,
                    )
                    .await?;
            }

            let copilot_token = account
                .copilot_token
                .as_deref()
                .unwrap_or(&account.github_token);

            let mut config = crate::config::ProviderConfig {
                kind: crate::config::ProviderKind::OpenAiChat,
                request_url: copilot::COPILOT_CHAT_URL.into(),
                api_key: copilot_token.to_string(),
                custom_headers: reqwest::header::HeaderMap::new(),
                max_output_tokens: None,
                request_timeout: std::time::Duration::from_secs(3600),
                allowed_body_fields: None,
            };

            for (k, v) in copilot::request_headers(copilot_token) {
                if let (Ok(h_name), Ok(h_val)) = (
                    reqwest::header::HeaderName::from_bytes(k.as_bytes()),
                    reqwest::header::HeaderValue::from_str(&v),
                ) {
                    config.custom_headers.insert(h_name, h_val);
                }
            }

            let provider = crate::provider::OpenAiChatProvider::new(client, config)
                .with_copilot_vision_header()
                .with_recorder(recorder);

            Ok(Some((
                record.id.clone(),
                provider.stream(invocation, cancellation),
            )))
        }

        ("dev.nexusor.plugins.kimi-auth", "kimi") => {
            let record = registry
                .select_resource_with_strategy(
                    plugin_id,
                    kimi::RESOURCE_TYPE,
                    excluded_ids,
                    allowed_account_ids,
                    Some(&invocation.conversation_id),
                    invocation.slot_strategy,
                )
                .await?;
            let mut account = kimi::KimiAccountData::from_record(&record).ok_or_else(|| {
                Error::Provider("invalid Moonshot Kimi account credentials in record".into())
            })?;

            let client = registry.client().await?;

            if kimi::ensure_fresh_account(&client, &mut account).await? {
                registry
                    .persist_kimi_account(plugin_id, kimi::RESOURCE_TYPE, &record.id, &account)
                    .await?;
            }

            let mut config = crate::config::ProviderConfig {
                kind: crate::config::ProviderKind::OpenAiChat,
                request_url: kimi::KIMI_CHAT_URL.into(),
                api_key: account.access_token.clone(),
                custom_headers: reqwest::header::HeaderMap::new(),
                max_output_tokens: None,
                request_timeout: std::time::Duration::from_secs(3600),
                allowed_body_fields: None,
            };

            for (k, v) in kimi::request_headers(&account.access_token) {
                if let (Ok(h_name), Ok(h_val)) = (
                    reqwest::header::HeaderName::from_bytes(k.as_bytes()),
                    reqwest::header::HeaderValue::from_str(&v),
                ) {
                    config.custom_headers.insert(h_name, h_val);
                }
            }

            let provider =
                crate::provider::OpenAiChatProvider::new(client, config).with_recorder(recorder);

            Ok(Some((
                record.id.clone(),
                provider.stream(invocation, cancellation),
            )))
        }

        ("dev.nexusor.plugins.claude-code", "claude-code") => {
            let record = registry
                .select_resource_with_strategy(
                    plugin_id,
                    claude_code::RESOURCE_TYPE,
                    excluded_ids,
                    allowed_account_ids,
                    Some(&invocation.conversation_id),
                    invocation.slot_strategy,
                )
                .await?;
            let mut account =
                claude_code::ClaudeCodeAccountData::from_record(&record).ok_or_else(|| {
                    Error::Provider("invalid Claude Code account credentials in record".into())
                })?;

            let client = registry.client().await?;

            if claude_code::ensure_fresh_account(&client, &mut account).await? {
                registry
                    .persist_claude_code_account(
                        plugin_id,
                        claude_code::RESOURCE_TYPE,
                        &record.id,
                        &account,
                    )
                    .await?;
            }

            let mut config = crate::config::ProviderConfig {
                kind: crate::config::ProviderKind::Anthropic,
                request_url: claude_code::ANTHROPIC_MESSAGES_URL.into(),
                api_key: account.access_token.clone(),
                custom_headers: reqwest::header::HeaderMap::new(),
                max_output_tokens: None,
                request_timeout: std::time::Duration::from_secs(3600),
                allowed_body_fields: None,
            };

            for (k, v) in claude_code::request_headers(&account.access_token) {
                if let (Ok(h_name), Ok(h_val)) = (
                    reqwest::header::HeaderName::from_bytes(k.as_bytes()),
                    reqwest::header::HeaderValue::from_str(&v),
                ) {
                    config.custom_headers.insert(h_name, h_val);
                }
            }

            let provider =
                crate::provider::AnthropicProvider::new(client, config).with_recorder(recorder);

            Ok(Some((
                record.id.clone(),
                provider.stream(invocation, cancellation),
            )))
        }

        ("dev.nexusor.plugins.nvidia-nim", "nvidia-nim") => {
            let record = registry
                .select_resource_with_strategy(
                    plugin_id,
                    "nvidia-account",
                    excluded_ids,
                    allowed_account_ids,
                    Some(&invocation.conversation_id),
                    invocation.slot_strategy,
                )
                .await?;
            let api_key = record
                .private_data
                .get("apiKey")
                .or_else(|| record.private_data.get("api_key"))
                .or_else(|| record.private_data.get("accessToken"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string();

            let client = registry.client().await?;
            let config = crate::config::ProviderConfig {
                kind: crate::config::ProviderKind::OpenAiChat,
                request_url: "https://integrate.api.nvidia.com/v1/chat/completions".into(),
                api_key,
                custom_headers: reqwest::header::HeaderMap::new(),
                max_output_tokens: None,
                request_timeout: std::time::Duration::from_secs(3600),
                allowed_body_fields: None,
            };

            let provider =
                crate::provider::OpenAiChatProvider::new(client, config).with_recorder(recorder);

            Ok(Some((
                record.id.clone(),
                provider.stream(invocation, cancellation),
            )))
        }

        ("dev.nexusor.plugins.opencode", "opencode") => {
            let record = registry
                .select_resource_with_strategy(
                    plugin_id,
                    "opencode-account",
                    excluded_ids,
                    allowed_account_ids,
                    Some(&invocation.conversation_id),
                    invocation.slot_strategy,
                )
                .await?;
            let api_key = record
                .private_data
                .get("apiKey")
                .or_else(|| record.private_data.get("api_key"))
                .or_else(|| record.private_data.get("accessToken"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string();

            let base_url = record
                .private_data
                .get("baseUrl")
                .or_else(|| record.private_data.get("base_url"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or("https://opencode.ai/zen/v1");

            let chat_url = if base_url.contains("opencode.ai") {
                "https://opencode.ai/zen/v1/chat/completions".to_string()
            } else {
                format!("{}/chat/completions", base_url.trim_end_matches('/'))
            };

            let client = registry.client().await?;
            let config = crate::config::ProviderConfig {
                kind: crate::config::ProviderKind::OpenAiChat,
                request_url: chat_url,
                api_key,
                custom_headers: reqwest::header::HeaderMap::new(),
                max_output_tokens: None,
                request_timeout: std::time::Duration::from_secs(3600),
                allowed_body_fields: None,
            };

            let provider =
                crate::provider::OpenAiChatProvider::new(client, config).with_recorder(recorder);

            Ok(Some((
                record.id.clone(),
                provider.stream(invocation, cancellation),
            )))
        }

        ("dev.nexusor.plugins.groq-lpu", "groq") => {
            let record = registry
                .select_resource_with_strategy(
                    plugin_id,
                    "groq-lpu-account",
                    excluded_ids,
                    allowed_account_ids,
                    Some(&invocation.conversation_id),
                    invocation.slot_strategy,
                )
                .await?;
            let api_key = record
                .private_data
                .get("apiKey")
                .or_else(|| record.private_data.get("api_key"))
                .or_else(|| record.private_data.get("accessToken"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string();

            let client = registry.client().await?;
            let config = crate::config::ProviderConfig {
                kind: crate::config::ProviderKind::OpenAiChat,
                request_url: "https://api.groq.com/openai/v1/chat/completions".into(),
                api_key,
                custom_headers: reqwest::header::HeaderMap::new(),
                max_output_tokens: None,
                request_timeout: std::time::Duration::from_secs(3600),
                allowed_body_fields: None,
            };

            let provider =
                crate::provider::OpenAiChatProvider::new(client, config).with_recorder(recorder);

            Ok(Some((
                record.id.clone(),
                provider.stream(invocation, cancellation),
            )))
        }

        (
            "dev.nexusor.plugins.antigravity-auth" | "dev.cursorbyok.plugins.antigravity-auth",
            "antigravity",
        ) => {
            let mut effective_excluded = excluded_ids.to_vec();
            // Model-family quota awareness: if this request targets Gemini or Claude, exclude
            // accounts whose quota for that specific family is already known to be exhausted.
            let model_lower = upstream_model_id.to_ascii_lowercase();
            let is_gemini = model_lower.contains("gemini");
            let is_claude = model_lower.contains("claude")
                || model_lower.contains("sonnet")
                || model_lower.contains("opus");

            let now = crate::store::now_ms();
            if let Ok(records) = registry
                .resources(plugin_id, antigravity::RESOURCE_TYPE)
                .await
            {
                for r in records {
                    if super::quota::antigravity_model_is_cooling(
                        &r.private_data,
                        upstream_model_id,
                        now,
                    ) && !effective_excluded.contains(&r.id)
                    {
                        effective_excluded.push(r.id.clone());
                    }
                    if let Some(quota) = r.private_data.get("quota") {
                        if is_gemini {
                            let g_5h = quota
                                .pointer("/gemini/remaining_percent")
                                .and_then(serde_json::Value::as_f64)
                                .unwrap_or(100.0);
                            let g_5h_reset = quota
                                .pointer("/gemini/reset_at_ms")
                                .and_then(serde_json::Value::as_i64)
                                .unwrap_or(i64::MAX);
                            let g_wk = quota
                                .pointer("/gemini_weekly/remaining_percent")
                                .and_then(serde_json::Value::as_f64)
                                .unwrap_or(100.0);
                            let g_wk_reset = quota
                                .pointer("/gemini_weekly/reset_at_ms")
                                .and_then(serde_json::Value::as_i64)
                                .unwrap_or(i64::MAX);
                            let is_empty = (g_5h <= 0.0 && now < g_5h_reset)
                                || (g_wk <= 1.0 && now < g_wk_reset);
                            if is_empty && !effective_excluded.contains(&r.id) {
                                effective_excluded.push(r.id);
                            }
                        } else if is_claude {
                            let c_5h = quota
                                .pointer("/claude/remaining_percent")
                                .and_then(serde_json::Value::as_f64)
                                .unwrap_or(100.0);
                            let c_5h_reset = quota
                                .pointer("/claude/reset_at_ms")
                                .and_then(serde_json::Value::as_i64)
                                .unwrap_or(i64::MAX);
                            let c_wk = quota
                                .pointer("/claude_weekly/remaining_percent")
                                .and_then(serde_json::Value::as_f64)
                                .unwrap_or(100.0);
                            let c_wk_reset = quota
                                .pointer("/claude_weekly/reset_at_ms")
                                .and_then(serde_json::Value::as_i64)
                                .unwrap_or(i64::MAX);
                            let is_empty = (c_5h <= 0.0 && now < c_5h_reset)
                                || (c_wk <= 1.0 && now < c_wk_reset);
                            if is_empty && !effective_excluded.contains(&r.id) {
                                effective_excluded.push(r.id);
                            }
                        }
                    }
                }
            }

            let record = registry
                .select_resource_with_strategy(
                    plugin_id,
                    antigravity::RESOURCE_TYPE,
                    &effective_excluded,
                    allowed_account_ids,
                    Some(&invocation.conversation_id),
                    invocation.slot_strategy,
                )
                .await?;
            let mut account = AntigravityAccountData::from_record(&record).ok_or_else(|| {
                Error::Provider("invalid Antigravity account credentials in record".into())
            })?;

            let client = registry.client().await?;

            if antigravity::ensure_fresh_account(&client, &mut account).await? {
                registry
                    .persist_antigravity_account(
                        plugin_id,
                        antigravity::RESOURCE_TYPE,
                        &record.id,
                        &account,
                    )
                    .await?;
            }

            if account
                .project_id
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .is_none()
            {
                if let Ok(quota) = antigravity::query_usage(&client, &account.access_token).await {
                    account.project_id = Some(quota.project_id.clone());
                    registry
                        .persist_antigravity_account(
                            plugin_id,
                            antigravity::RESOURCE_TYPE,
                            &record.id,
                            &account,
                        )
                        .await?;
                }
            }

            let effort_tiers = registry
                .model_effort_tiers(plugin_id, provider_id, upstream_model_id)
                .await;
            let provider =
                AntigravityCloudCodeProvider::new(client, account, upstream_model_id.to_owned())
                    .with_effort_tiers(effort_tiers)
                    .with_recorder(recorder);
            #[cfg(test)]
            let provider = provider.with_test_stream_urls(registry.antigravity_test_urls().await);

            Ok(Some((
                record.id.clone(),
                provider.stream(invocation, cancellation),
            )))
        }

        _ => Ok(None),
    }
}
