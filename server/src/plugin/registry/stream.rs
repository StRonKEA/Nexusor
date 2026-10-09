use super::*;

impl PluginRegistry {
    pub fn stream_model(
        &self,
        invocation: ModelInvocation,
        cancellation: CancellationToken,
        recorder: CallRecorder,
    ) -> ProviderStream {
        let registry = self.clone();
        Box::pin(async_stream::try_stream! {
            let model_id = invocation.request.model.model_id.clone();
            let (plugin_id, provider_id, upstream_id) = parse_model_id(&model_id)
                .map(|(plugin, provider, model)| (plugin.to_owned(), provider.to_owned(), model.to_owned()))
                .ok_or_else(|| Error::Provider(format!("invalid plugin model ID: {model_id}")))?;

            let resource_type = match (plugin_id.as_str(), provider_id.as_str()) {
                ("dev.nexusor.examples.codex-auth" | "dev.cursorbyok.examples.codex-auth", "codex") => crate::provider::providers::codex::RESOURCE_TYPE,
                ("dev.nexusor.examples.grok-auth" | "dev.cursorbyok.examples.grok-auth", "grok") => crate::provider::providers::grok::RESOURCE_TYPE,
                ("dev.nexusor.plugins.antigravity-auth" | "dev.cursorbyok.plugins.antigravity-auth", "antigravity") => crate::provider::providers::antigravity::RESOURCE_TYPE,
                ("dev.nexusor.plugins.github-copilot", "copilot") => crate::provider::providers::copilot::RESOURCE_TYPE,
                ("dev.nexusor.plugins.kimi-auth", "kimi") => crate::provider::providers::kimi::RESOURCE_TYPE,
                ("dev.nexusor.plugins.claude-code", "claude-code") => crate::provider::providers::claude_code::RESOURCE_TYPE,
                ("dev.nexusor.plugins.nvidia-nim", "nvidia-nim") => "nvidia-account",
                ("dev.nexusor.plugins.opencode", "opencode") => "opencode-account",
                ("dev.nexusor.plugins.groq-lpu", "groq") => "groq-lpu-account",
                _ => Err(Error::Provider(format!("unknown native plugin provider: {plugin_id}/{provider_id}")))?,
            };
            let strategy = match invocation.slot_strategy {
                Some(strategy) => strategy,
                None => registry.pool_strategy(&plugin_id).await?,
            };
            let mut excluded = Vec::<String>::new();
            let mut last_quota_error: Option<String> = None;

            loop {
                let allowed_ids = invocation.slot_account_ids.as_deref();
                let stream_res = crate::provider::providers::stream_native_provider(
                    crate::provider::providers::NativeStreamRequest {
                        plugin_id: &plugin_id,
                        provider_id: &provider_id,
                        upstream_model_id: &upstream_id,
                        invocation: invocation.clone(),
                        cancellation: cancellation.clone(),
                        registry: &registry,
                        recorder: Some(recorder.clone()),
                        excluded_ids: &excluded,
                        allowed_account_ids: allowed_ids,
                    },
                ).await;

                // `unreachable!()` closes the diverging `let-else` arm; it is never reached.
                let (resource_id, mut native_stream) = match stream_res {
                    Ok(Some(parts)) => parts,
                    Ok(None) => {
                        Err(Error::Provider(format!("unknown native plugin provider: {plugin_id}/{provider_id}")))?;
                        unreachable!();
                    }
                    Err(err) => {
                        if let Some(prev_err) = last_quota_error {
                            Err(Error::Provider(prev_err))?;
                        }
                        Err(err)?;
                        unreachable!();
                    }
                };

                tracing::info!(
                    call_id = %invocation.call_id,
                    provider = %provider_id,
                    model = %upstream_id,
                    account_id = %resource_id,
                    attempt = excluded.len() + 1,
                    "native provider account selected"
                );

                use futures_util::StreamExt;
                let mut pending: Vec<Result<crate::provider::ModelEvent>> = Vec::new();
                let mut retry = false;
                while let Some(item) = native_stream.next().await {
                    match item {
                        Ok(event) => {
                            let response_started = crate::provider::is_valid_response_event(&event);
                            pending.push(Ok(event));
                            if response_started {
                                if !excluded.is_empty() {
                                    tracing::info!(
                                        call_id = %invocation.call_id,
                                        provider = %provider_id,
                                        model = %upstream_id,
                                        account_id = %resource_id,
                                        previous_account_ids = ?excluded,
                                        "native provider account failover produced a valid response"
                                    );
                                }
                                for event in pending.drain(..) {
                                    yield event?;
                                }
                                while let Some(event) = native_stream.next().await {
                                    yield event?;
                                }
                                return;
                            }
                        }
                        Err(error) => {
                            let error_text = error.to_string();
                            let account_quota = crate::provider::providers::quota::is_account_quota_error(&provider_id, &error_text);
                            let transient_capacity = crate::provider::providers::quota::is_transient_capacity_error(&provider_id, &error_text);
                            // The upstream can refuse a token that still looks valid
                            // locally, so the refresh that normally waits for `exp`
                            // never fires. Invalidating it lets the next attempt
                            // refresh instead of failing until the token really expires.
                            let auth_failure = crate::provider::providers::quota::is_auth_failure(&provider_id, &error_text);
                            if strategy != PoolStrategy::Single
                                && crate::provider::providers::quota::is_failover_provider(&provider_id)
                                && (account_quota || transient_capacity || auth_failure)
                            {
                                if account_quota {
                                    let cooldown = crate::provider::providers::quota::quota_cooldown(&provider_id, &error_text);
                                    registry.cool_model_resource(
                                        &plugin_id,
                                        resource_type,
                                        &resource_id,
                                        &upstream_id,
                                        cooldown,
                                    ).await?;
                                }
                                if auth_failure {
                                    registry
                                        .invalidate_access_token(
                                            &plugin_id,
                                            resource_type,
                                            &resource_id,
                                        )
                                        .await?;
                                    registry
                                        .cool_auth_resource(
                                            &plugin_id,
                                            resource_type,
                                            &resource_id,
                                            crate::provider::providers::quota::AUTH_COOLDOWN,
                                            "upstream rejected the credential; token invalidated",
                                        )
                                        .await?;
                                }
                                tracing::warn!(
                                    call_id = %invocation.call_id,
                                    provider = %provider_id,
                                    model = %upstream_id,
                                    account_id = %resource_id,
                                    reason = if account_quota {
                                        "account_quota"
                                    } else if auth_failure {
                                        "auth_failure"
                                    } else {
                                        "transient_capacity"
                                    },
                                    persistent_cooldown = account_quota,
                                    token_invalidated = auth_failure,
                                    "native provider account failed before valid response; selecting another account"
                                );
                                excluded.push(resource_id);
                                recorder.next_account_attempt(&error, excluded.len() + 1).await?;
                                last_quota_error = Some(error_text);
                                retry = true;
                            } else {
                                for event in pending.drain(..) {
                                    yield event?;
                                }
                                Err(error)?;
                            }
                            break;
                        }
                    }
                }
                if retry {
                    if cancellation.is_cancelled() {
                        Err(Error::Cancelled)?;
                    }
                    continue;
                }
                for event in pending {
                    yield event?;
                }
                if let Some(error_text) = last_quota_error {
                    Err(Error::Provider(error_text))?;
                }
                return;
            }
        })
    }
}
