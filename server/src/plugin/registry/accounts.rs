use super::*;

impl PluginRegistry {
    pub async fn persist_antigravity_account(
        &self,
        plugin_id: &str,
        resource_type: &str,
        resource_id: &str,
        account: &crate::provider::providers::antigravity::AntigravityAccountData,
    ) -> Result<()> {
        self.inner
            .state
            .mutate_resource(plugin_id, resource_type, resource_id, |record| {
                record.private_data =
            crate::provider::providers::account_private_data::normalize_account_private_data(
                record.private_data.clone(),
            );
                if let Some(object) = record.private_data.as_object_mut() {
                    object.insert(
                        "accessToken".into(),
                        serde_json::Value::String(account.access_token.clone()),
                    );
                    if let Some(refresh) = account.refresh_token.clone() {
                        object.insert("refreshToken".into(), serde_json::Value::String(refresh));
                    }
                    if let Some(project_id) = account.project_id.clone() {
                        object.insert("projectId".into(), serde_json::Value::String(project_id));
                    }
                    if let Some(expires_at_ms) = account.expires_at_ms {
                        object.insert(
                            "expiresAtMs".into(),
                            serde_json::Value::Number(expires_at_ms.into()),
                        );
                    }
                    if !account.display_name.trim().is_empty() {
                        object.insert(
                            "displayName".into(),
                            serde_json::Value::String(account.display_name.clone()),
                        );
                    }
                }
                Ok(())
            })
            .await
    }

    pub async fn persist_codex_account(
        &self,
        plugin_id: &str,
        resource_type: &str,
        resource_id: &str,
        account: &crate::provider::providers::codex::AccountData,
    ) -> Result<()> {
        self.inner
            .state
            .mutate_resource(plugin_id, resource_type, resource_id, |record| {
                record.private_data =
            crate::provider::providers::account_private_data::normalize_account_private_data(
                record.private_data.clone(),
            );
                if let Some(object) = record.private_data.as_object_mut() {
                    object.insert(
                        "accessToken".into(),
                        serde_json::Value::String(account.access_token.clone()),
                    );
                    if let Some(refresh) = account.refresh_token.clone() {
                        object.insert("refreshToken".into(), serde_json::Value::String(refresh));
                    }
                    if !account.display_name.trim().is_empty() {
                        object.insert(
                            "displayName".into(),
                            serde_json::Value::String(account.display_name.clone()),
                        );
                    }
                }
                Ok(())
            })
            .await
    }

    /// Clears a credential the upstream refused.
    ///
    /// The refresh token is kept, so the next attempt goes through the normal refresh
    /// path and the account recovers on its own. This is what turns an upstream
    /// revocation into a self-healing retry instead of a dead account.
    pub async fn invalidate_access_token(
        &self,
        plugin_id: &str,
        resource_type: &str,
        resource_id: &str,
    ) -> Result<()> {
        self.inner
            .state
            .mutate_resource(plugin_id, resource_type, resource_id, |record| {
                if let Some(object) = record.private_data.as_object_mut() {
                    object.insert(
                        "accessToken".into(),
                        serde_json::Value::String(String::new()),
                    );
                }
                Ok(())
            })
            .await
    }

    pub async fn persist_grok_account(
        &self,
        plugin_id: &str,
        resource_type: &str,
        resource_id: &str,
        account: &crate::provider::providers::grok::GrokAccountData,
    ) -> Result<()> {
        self.inner
            .state
            .mutate_resource(plugin_id, resource_type, resource_id, |record| {
                record.private_data =
            crate::provider::providers::account_private_data::normalize_account_private_data(
                record.private_data.clone(),
            );
                if let Some(object) = record.private_data.as_object_mut() {
                    object.insert(
                        "accessToken".into(),
                        serde_json::Value::String(account.access_token.clone()),
                    );
                    if let Some(refresh) = account.refresh_token.clone() {
                        object.insert("refreshToken".into(), serde_json::Value::String(refresh));
                    }
                    if !account.display_name.trim().is_empty() {
                        object.insert(
                            "displayName".into(),
                            serde_json::Value::String(account.display_name.clone()),
                        );
                    }
                }
                Ok(())
            })
            .await
    }

    pub async fn persist_claude_code_account(
        &self,
        plugin_id: &str,
        resource_type: &str,
        resource_id: &str,
        account: &crate::provider::providers::claude_code::ClaudeCodeAccountData,
    ) -> Result<()> {
        self.inner
            .state
            .mutate_resource(plugin_id, resource_type, resource_id, |record| {
                record.private_data =
            crate::provider::providers::account_private_data::normalize_account_private_data(
                record.private_data.clone(),
            );
                if let Some(object) = record.private_data.as_object_mut() {
                    object.insert(
                        "accessToken".into(),
                        serde_json::Value::String(account.access_token.clone()),
                    );
                    if let Some(r) = &account.refresh_token {
                        object.insert("refreshToken".into(), serde_json::Value::String(r.clone()));
                    }
                    if let Some(exp) = account.expires_at_ms {
                        object.insert("expiresAtMs".into(), serde_json::Value::Number(exp.into()));
                    }
                }
                Ok(())
            })
            .await
    }

    pub async fn persist_kimi_account(
        &self,
        plugin_id: &str,
        resource_type: &str,
        resource_id: &str,
        account: &crate::provider::providers::kimi::KimiAccountData,
    ) -> Result<()> {
        self.inner
            .state
            .mutate_resource(plugin_id, resource_type, resource_id, |record| {
                record.private_data =
            crate::provider::providers::account_private_data::normalize_account_private_data(
                record.private_data.clone(),
            );
                if let Some(object) = record.private_data.as_object_mut() {
                    object.insert(
                        "accessToken".into(),
                        serde_json::Value::String(account.access_token.clone()),
                    );
                    if let Some(r) = &account.refresh_token {
                        object.insert("refreshToken".into(), serde_json::Value::String(r.clone()));
                    }
                    if let Some(exp) = account.expires_at_ms {
                        object.insert("expiresAtMs".into(), serde_json::Value::Number(exp.into()));
                    }
                }
                Ok(())
            })
            .await
    }

    pub async fn persist_copilot_account(
        &self,
        plugin_id: &str,
        resource_type: &str,
        resource_id: &str,
        account: &crate::provider::providers::copilot::CopilotAccountData,
    ) -> Result<()> {
        self.inner
            .state
            .mutate_resource(plugin_id, resource_type, resource_id, |record| {
                record.private_data =
            crate::provider::providers::account_private_data::normalize_account_private_data(
                record.private_data.clone(),
            );
                if let Some(object) = record.private_data.as_object_mut() {
                    object.insert(
                        "githubToken".into(),
                        serde_json::Value::String(account.github_token.clone()),
                    );
                    if let Some(token) = &account.copilot_token {
                        object.insert(
                            "copilotToken".into(),
                            serde_json::Value::String(token.clone()),
                        );
                    }
                    if let Some(exp) = account.copilot_expires_at_ms {
                        object.insert(
                            "copilotExpiresAtMs".into(),
                            serde_json::Value::Number(exp.into()),
                        );
                    }
                }
                Ok(())
            })
            .await
    }

    pub(crate) async fn fetch_openai_compatible_models(
        client: &reqwest::Client,
        base_url: &str,
        api_key: &str,
    ) -> Result<Vec<StoredModel>> {
        let base = base_url.trim().trim_end_matches('/');
        let url = if base.contains("opencode.ai") {
            if base.ends_with("/models") {
                base.to_string()
            } else if base.ends_with("/v1") {
                format!("{base}/models")
            } else {
                "https://opencode.ai/zen/v1/models".to_string()
            }
        } else if base.ends_with("/models") {
            base.to_string()
        } else if base.ends_with("/v1") {
            format!("{base}/models")
        } else {
            format!("{base}/v1/models")
        };

        let mut req = client
            .get(&url)
            .header("accept", "application/json")
            .header(
                "user-agent",
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36",
            );

        if !api_key.trim().is_empty() {
            req = req.header("authorization", format!("Bearer {}", api_key.trim()));
        }

        let res = req.send().await?;
        let status = res.status();
        let text = res.text().await.unwrap_or_default();

        if !status.is_success() {
            return Err(Error::Provider(format!(
                "Modeller sunucudan çekilemedi ({status}): {text}"
            )));
        }

        let json: serde_json::Value = serde_json::from_str(&text).map_err(|e| {
            Error::Provider(format!(
                "Model listesi JSON olarak ayrıştırılamadı (Yanıt: {text}): {e}"
            ))
        })?;

        let mut models = Vec::new();

        let raw_list = json
            .get("data")
            .and_then(serde_json::Value::as_array)
            .or_else(|| json.as_array());

        if let Some(arr) = raw_list {
            for item in arr {
                if let Some(id) = item.get("id").and_then(serde_json::Value::as_str) {
                    let id = id.trim();
                    if id.is_empty() {
                        continue;
                    }
                    let lower = id.to_ascii_lowercase();
                    if lower.contains("embed")
                        || lower.contains("whisper")
                        || lower.contains("tts")
                        || lower.contains("moderation")
                        || lower.contains("bge")
                        || lower.contains("guard")
                    {
                        continue;
                    }
                    let is_vision = lower.contains("vision")
                        || lower.contains("vl")
                        || lower.contains("4o")
                        || lower.contains("claude")
                        || lower.contains("multimodal");
                    let display_name = item
                        .get("name")
                        .or_else(|| item.get("display_name"))
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or(id);

                    models.push(StoredModel {
                        id: id.to_string(),
                        display_name: display_name.to_string(),
                        description: item
                            .get("description")
                            .and_then(serde_json::Value::as_str)
                            .map(String::from),
                        max_output_tokens: item
                            .get("max_output_tokens")
                            .or_else(|| item.get("context_window"))
                            .and_then(serde_json::Value::as_u64)
                            .or(Some(8192)),
                        images: is_vision,
                        enabled: true,
                        private_data: serde_json::json!({}),
                    });
                }
            }
        }

        if models.is_empty() {
            return Err(Error::Provider(format!(
                "Sağlayıcı geçerli model döndürmedi. Ham yanıt: {text}"
            )));
        }

        Ok(models)
    }
}
