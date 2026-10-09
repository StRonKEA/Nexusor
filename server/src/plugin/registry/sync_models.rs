use super::*;

impl PluginRegistry {
    pub async fn sync_models(&self, plugin_id: &str, provider_id: &str) -> Result<usize> {
        let client = self.client().await?;

        let models: Vec<StoredModel> = match (plugin_id, provider_id) {
            ("dev.nexusor.plugins.claude-code", "claude-code") => {
                let accounts = self
                    .inner
                    .state
                    .resources(
                        plugin_id,
                        crate::provider::providers::claude_code::RESOURCE_TYPE,
                    )
                    .await
                    .unwrap_or_default();
                let models = if let Some(first) = accounts.first() {
                    if let Some(account) =
                        crate::provider::providers::claude_code::ClaudeCodeAccountData::from_record(
                            first,
                        )
                    {
                        crate::provider::providers::claude_code::models::fetch_models(
                            &client,
                            &account.access_token,
                        )
                        .await
                        .map_err(|e| {
                            Error::Provider(format!(
                                "Claude Code modelleri sunucudan çekilemedi: {e}"
                            ))
                        })?
                    } else {
                        crate::provider::providers::claude_code::claude_code_models()
                    }
                } else {
                    crate::provider::providers::claude_code::claude_code_models()
                };

                models
                    .into_iter()
                    .map(|m| {
                        let display_name = m.display_name;
                        StoredModel {
                            id: m.id,
                            display_name,
                            description: m.description,
                            max_output_tokens: m.max_output_tokens,
                            images: m.images,
                            enabled: true,
                            private_data: serde_json::json!({}),
                        }
                    })
                    .collect()
            }
            ("dev.nexusor.plugins.nvidia-nim", "nvidia-nim") => {
                let accounts = self
                    .inner
                    .state
                    .resources(plugin_id, "nvidia-account")
                    .await
                    .unwrap_or_default();
                let Some(first) = accounts.first() else {
                    return Err(Error::Provider(
                        "NVIDIA hesabı bulunamadı; önce API anahtarınızı bağlayın, ardından modelleri senkronize edin.".into(),
                    ));
                };
                let api_key = first
                    .private_data
                    .get("apiKey")
                    .or_else(|| first.private_data.get("api_key"))
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("");
                Self::fetch_openai_compatible_models(
                    &client,
                    "https://integrate.api.nvidia.com/v1",
                    api_key,
                )
                .await?
            }
            ("dev.nexusor.plugins.opencode", "opencode") => {
                let accounts = self
                    .inner
                    .state
                    .resources(plugin_id, "opencode-account")
                    .await
                    .unwrap_or_default();
                let Some(first) = accounts.first() else {
                    return Err(Error::Provider(
                        "OpenCode hesabı bulunamadı; önce API anahtarınızı veya uç noktanızı bağlayın, ardından modelleri senkronize edin.".into(),
                    ));
                };
                let api_key = first
                    .private_data
                    .get("apiKey")
                    .or_else(|| first.private_data.get("api_key"))
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("");
                let base_url = first
                    .private_data
                    .get("baseUrl")
                    .or_else(|| first.private_data.get("base_url"))
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("https://opencode.ai/zen/v1");
                Self::fetch_openai_compatible_models(&client, base_url, api_key).await?
            }
            ("dev.nexusor.plugins.groq-lpu", "groq") => {
                let accounts = self
                    .inner
                    .state
                    .resources(plugin_id, "groq-lpu-account")
                    .await
                    .unwrap_or_default();
                let Some(first) = accounts.first() else {
                    return Err(Error::Provider(
                        "Groq hesabı bulunamadı; önce API anahtarınızı bağlayın, ardından modelleri senkronize edin.".into(),
                    ));
                };
                let api_key = first
                    .private_data
                    .get("apiKey")
                    .or_else(|| first.private_data.get("api_key"))
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("");
                Self::fetch_openai_compatible_models(
                    &client,
                    "https://api.groq.com/openai/v1",
                    api_key,
                )
                .await?
            }
            ("dev.nexusor.plugins.kimi-auth", "kimi") => {
                let accounts = self
                    .inner
                    .state
                    .resources(plugin_id, crate::provider::providers::kimi::RESOURCE_TYPE)
                    .await
                    .unwrap_or_default();
                let models = if let Some(first) = accounts.first() {
                    if let Some(account) =
                        crate::provider::providers::kimi::KimiAccountData::from_record(first)
                    {
                        crate::provider::providers::kimi::models::fetch_models(
                            &client,
                            &account.access_token,
                        )
                        .await
                        .map_err(|e| {
                            Error::Provider(format!("Kimi modelleri sunucudan çekilemedi: {e}"))
                        })?
                    } else {
                        crate::provider::providers::kimi::kimi_models()
                    }
                } else {
                    crate::provider::providers::kimi::kimi_models()
                };

                models
                    .into_iter()
                    .map(|m| {
                        let display_name = m.display_name;
                        StoredModel {
                            id: m.id,
                            display_name,
                            description: m.description,
                            max_output_tokens: m.max_output_tokens,
                            images: m.images,
                            enabled: true,
                            private_data: serde_json::json!({}),
                        }
                    })
                    .collect()
            }
            ("dev.nexusor.plugins.github-copilot", "copilot") => {
                let accounts = self
                    .inner
                    .state
                    .resources(
                        plugin_id,
                        crate::provider::providers::copilot::RESOURCE_TYPE,
                    )
                    .await
                    .unwrap_or_default();
                let models = if let Some(first) = accounts.first() {
                    if let Some(account) =
                        crate::provider::providers::copilot::CopilotAccountData::from_record(first)
                    {
                        let token = account
                            .copilot_token
                            .as_deref()
                            .unwrap_or(&account.github_token);
                        crate::provider::providers::copilot::models::fetch_models(&client, token)
                            .await
                            .map_err(|e| {
                                Error::Provider(format!(
                                    "Copilot modelleri sunucudan çekilemedi: {e}"
                                ))
                            })?
                    } else {
                        crate::provider::providers::copilot::copilot_models()
                    }
                } else {
                    crate::provider::providers::copilot::copilot_models()
                };

                models
                    .into_iter()
                    .map(|m| {
                        let display_name = format!("{} (Copilot)", m.id);
                        StoredModel {
                            id: m.id,
                            display_name,
                            description: m.description,
                            max_output_tokens: m.max_output_tokens,
                            images: m.images,
                            enabled: true,
                            private_data: serde_json::json!({}),
                        }
                    })
                    .collect()
            }
            ("dev.nexusor.examples.codex-auth" | "dev.cursorbyok.examples.codex-auth", "codex") => {
                let accounts = self
                    .inner
                    .state
                    .resources(plugin_id, crate::provider::providers::codex::RESOURCE_TYPE)
                    .await?;
                if accounts.is_empty() {
                    return Err(Error::Provider(
                        "Codex hesabı yok; önce hesap bağla, sonra modelleri senkronize et".into(),
                    ));
                }
                let record = accounts
                    .iter()
                    .find(|record| !matches!(record.state, ResourceState::Disabled))
                    .ok_or_else(|| {
                        Error::Provider("Codex model sync requires an enabled account".into())
                    })?;
                let mut account =
                    crate::provider::providers::codex::AccountData::from_record(record)
                        .ok_or_else(|| Error::Provider("Codex hesap verisi okunamadı".into()))?;
                if crate::provider::providers::codex::ensure_fresh_account(&client, &mut account)
                    .await?
                {
                    self.persist_codex_account(
                        plugin_id,
                        crate::provider::providers::codex::RESOURCE_TYPE,
                        &record.id,
                        &account,
                    )
                    .await?;
                }
                let source = crate::provider::providers::codex::models::fetch_models(
                    &client,
                    &account.access_token,
                    account.account_id.as_deref(),
                )
                .await
                .map_err(Error::Provider)?;
                if source.is_empty() {
                    return Err(Error::Provider(
                        "Codex returned no available models; existing catalog preserved".into(),
                    ));
                }
                source
                    .into_iter()
                    .map(|m| StoredModel {
                        id: m.id,
                        display_name: m.display_name,
                        description: m.description,
                        max_output_tokens: m.max_output_tokens,
                        images: m.images,
                        enabled: true,
                        private_data: serde_json::json!({
                            "reasoningEfforts": m.reasoning_efforts,
                        }),
                    })
                    .collect()
            }
            ("dev.nexusor.examples.grok-auth" | "dev.cursorbyok.examples.grok-auth", "grok") => {
                let accounts = self
                    .inner
                    .state
                    .resources(plugin_id, crate::provider::providers::grok::RESOURCE_TYPE)
                    .await?;
                if accounts.is_empty() {
                    return Err(Error::Provider(
                        "Grok hesabı yok; önce hesap bağla, sonra modelleri senkronize et".into(),
                    ));
                }
                let account =
                    crate::provider::providers::grok::GrokAccountData::from_record(&accounts[0])
                        .ok_or_else(|| Error::Provider("Grok hesap verisi okunamadı".into()))?;
                let source = match crate::provider::providers::grok::models::fetch_models(
                    &client,
                    &account.access_token,
                )
                .await
                {
                    Ok(models) if !models.is_empty() => models,
                    // The CLI chat proxy is the only endpoint OAuth tokens may use; when it
                    // is unreachable keep the single model it is known to serve.
                    _ => crate::provider::providers::grok::models::fallback_models(),
                };
                source
                    .into_iter()
                    .map(|m| StoredModel {
                        id: m.id,
                        display_name: m.display_name,
                        description: None,
                        max_output_tokens: None,
                        images: m.images,
                        enabled: true,
                        private_data: serde_json::json!({}),
                    })
                    .collect()
            }
            (
                "dev.nexusor.plugins.antigravity-auth" | "dev.cursorbyok.plugins.antigravity-auth",
                "antigravity",
            ) => {
                let accounts = self
                    .inner
                    .state
                    .resources(
                        plugin_id,
                        crate::provider::providers::antigravity::RESOURCE_TYPE,
                    )
                    .await?;
                if accounts.is_empty() {
                    return Err(Error::Provider(
                        "Antigravity hesabı yok; önce hesap bağla, sonra modelleri senkronize et"
                            .into(),
                    ));
                }
                let account =
                    crate::provider::providers::antigravity::AntigravityAccountData::from_record(
                        &accounts[0],
                    )
                    .ok_or_else(|| Error::Provider("Antigravity hesap verisi okunamadı".into()))?;
                let project_id = account.project_id.clone().unwrap_or_default();
                let fetched = crate::provider::providers::antigravity::models::fetch_models(
                    &client,
                    &account.access_token,
                    &project_id,
                )
                .await;
                let source = match fetched {
                    Ok(models) => models,
                    Err(err) => {
                        return Err(Error::Provider(format!(
                            "Antigravity modelleri Google sunucusundan çekilemedi: {err}"
                        )));
                    }
                };
                source
                    .into_iter()
                    .map(|m| StoredModel {
                        id: m.id,
                        display_name: m.display_name,
                        description: None,
                        max_output_tokens: m.max_output_tokens,
                        images: m.images,
                        enabled: true,
                        private_data: serde_json::json!({
                            "effortTiers": m.effort_tiers,
                        }),
                    })
                    .collect()
            }
            _ => Vec::new(),
        };

        let count = models.len();
        self.inner
            .state
            .replace_models(plugin_id, provider_id, &models)
            .await?;
        Ok(count)
    }

    pub async fn set_model_enabled(
        &self,
        plugin_id: &str,
        provider_id: &str,
        model_id: &str,
        enabled: bool,
    ) -> Result<()> {
        self.inner
            .state
            .set_model_enabled(plugin_id, provider_id, model_id, enabled)
            .await
    }
}
