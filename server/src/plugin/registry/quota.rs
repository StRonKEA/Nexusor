use super::*;

impl PluginRegistry {
    pub async fn refresh_resource(
        &self,
        plugin_id: &str,
        resource_type: &str,
        resource_id: &str,
    ) -> Result<()> {
        let mut records = self.inner.state.resources(plugin_id, resource_type).await?;
        let Some(record) = records.iter_mut().find(|r| r.id == resource_id) else {
            return Err(Error::RunNotFound(format!("resource {resource_id}")));
        };

        let client = self.client().await?;
        let mut patch = serde_json::Map::new();
        let mut refresh_error = None;

        if plugin_id == "dev.nexusor.examples.codex-auth"
            || plugin_id == "dev.cursorbyok.examples.codex-auth"
        {
            record.private_data =
                crate::provider::providers::account_private_data::normalize_account_private_data(
                    record.private_data.clone(),
                );
            let mut data: crate::provider::providers::codex::AccountData =
                serde_json::from_value(record.private_data.clone())?;
            if crate::provider::providers::codex::ensure_fresh_account(&client, &mut data).await? {
                patch.insert(
                    "accessToken".into(),
                    serde_json::Value::String(data.access_token.clone()),
                );
                if let Some(r) = &data.refresh_token {
                    patch.insert("refreshToken".into(), serde_json::Value::String(r.clone()));
                }
            }
            match crate::provider::providers::codex::usage::query_usage(
                &client,
                &data.access_token,
                data.account_id.as_deref(),
            )
            .await
            {
                Ok(quota) => {
                    patch.insert("quota".into(), serde_json::to_value(quota)?);
                }
                Err(error) => refresh_error = Some(error),
            }
            // Accounts linked before e-mail extraction still carry the raw account UUID.
            let (_, display_name) = crate::provider::providers::codex::resources::account_identity(
                &data.access_token,
                data.account_id.as_deref(),
                None,
            );
            if display_name != data.display_name {
                patch.insert(
                    "displayName".into(),
                    serde_json::Value::String(display_name),
                );
            }
        } else if plugin_id == "dev.nexusor.examples.grok-auth"
            || plugin_id == "dev.cursorbyok.examples.grok-auth"
        {
            record.private_data =
                crate::provider::providers::account_private_data::normalize_account_private_data(
                    record.private_data.clone(),
                );
            let mut data: crate::provider::providers::grok::GrokAccountData =
                serde_json::from_value(record.private_data.clone())?;
            if crate::provider::providers::grok::ensure_fresh_account(&client, &mut data).await? {
                patch.insert(
                    "accessToken".into(),
                    serde_json::Value::String(data.access_token.clone()),
                );
                if let Some(r) = &data.refresh_token {
                    patch.insert("refreshToken".into(), serde_json::Value::String(r.clone()));
                }
            }
            match crate::provider::providers::grok::usage::query_usage(&client, &data.access_token)
                .await
            {
                Ok(quota) => {
                    patch.insert("quota".into(), serde_json::to_value(quota)?);
                }
                Err(error) => refresh_error = Some(error),
            }
            let email = crate::provider::providers::grok::resources::fetch_user_email(
                &client,
                &data.access_token,
            )
            .await;
            let (_, display_name) = crate::provider::providers::grok::resources::account_identity(
                email.as_deref(),
                &data.access_token,
            );
            if display_name != data.display_name {
                patch.insert(
                    "displayName".into(),
                    serde_json::Value::String(display_name),
                );
            }
        } else if plugin_id == "dev.nexusor.plugins.antigravity-auth"
            || plugin_id == "dev.cursorbyok.plugins.antigravity-auth"
        {
            record.private_data =
                crate::provider::providers::account_private_data::normalize_account_private_data(
                    record.private_data.clone(),
                );
            let mut data: crate::provider::providers::antigravity::AntigravityAccountData =
                serde_json::from_value(record.private_data.clone())?;
            if crate::provider::providers::antigravity::ensure_fresh_account(&client, &mut data)
                .await?
            {
                patch.insert(
                    "accessToken".into(),
                    serde_json::Value::String(data.access_token.clone()),
                );
                if let Some(refresh) = data.refresh_token.clone() {
                    patch.insert("refreshToken".into(), serde_json::Value::String(refresh));
                }
                if let Some(expires_at_ms) = data.expires_at_ms {
                    patch.insert(
                        "expiresAtMs".into(),
                        serde_json::Value::Number(expires_at_ms.into()),
                    );
                }
            }
            match crate::provider::providers::antigravity::usage::query_usage(
                &client,
                &data.access_token,
            )
            .await
            {
                Ok(quota) => {
                    if data.project_id.as_deref().unwrap_or("").is_empty() {
                        patch.insert(
                            "projectId".into(),
                            serde_json::Value::String(quota.project_id.clone()),
                        );
                    }
                    patch.insert("quota".into(), serde_json::to_value(quota)?);
                }
                Err(error) => refresh_error = Some(error),
            }
            if let Some(email) =
                crate::provider::providers::antigravity::userinfo::fetch_user_email(
                    &client,
                    &data.access_token,
                )
                .await
            {
                let (_, new_display) =
                    crate::provider::providers::antigravity::resources::account_identity(
                        Some(&email),
                        &data.access_token,
                    );
                patch.insert("displayName".into(), serde_json::Value::String(new_display));
            }
        } else if plugin_id == "dev.nexusor.plugins.github-copilot" {
            record.private_data =
                crate::provider::providers::account_private_data::normalize_account_private_data(
                    record.private_data.clone(),
                );
            let mut data: crate::provider::providers::copilot::CopilotAccountData =
                serde_json::from_value(record.private_data.clone())?;
            if crate::provider::providers::copilot::ensure_fresh_account(&client, &mut data).await?
            {
                if let Some(token) = &data.copilot_token {
                    patch.insert(
                        "copilotToken".into(),
                        serde_json::Value::String(token.clone()),
                    );
                }
                if let Some(exp) = data.copilot_expires_at_ms {
                    patch.insert(
                        "copilotExpiresAtMs".into(),
                        serde_json::Value::Number(exp.into()),
                    );
                }
            }
            let token = data.copilot_token.as_deref().unwrap_or(&data.github_token);
            if let Err(error) =
                crate::provider::providers::copilot::models::fetch_models(&client, token).await
            {
                refresh_error = Some(error);
            }
        } else if plugin_id == "dev.nexusor.plugins.kimi-auth" {
            record.private_data =
                crate::provider::providers::account_private_data::normalize_account_private_data(
                    record.private_data.clone(),
                );
            let mut data: crate::provider::providers::kimi::KimiAccountData =
                serde_json::from_value(record.private_data.clone())?;
            if crate::provider::providers::kimi::ensure_fresh_account(&client, &mut data).await? {
                patch.insert(
                    "accessToken".into(),
                    serde_json::Value::String(data.access_token.clone()),
                );
                if let Some(r) = &data.refresh_token {
                    patch.insert("refreshToken".into(), serde_json::Value::String(r.clone()));
                }
                if let Some(exp) = data.expires_at_ms {
                    patch.insert("expiresAtMs".into(), serde_json::Value::Number(exp.into()));
                }
            }
            match crate::provider::providers::kimi::usage::query_usage(&client, &data.access_token)
                .await
            {
                Ok(quota) => {
                    patch.insert("quota".into(), serde_json::to_value(quota)?);
                }
                Err(error) => refresh_error = Some(error),
            }
        } else if plugin_id == "dev.nexusor.plugins.claude-code" {
            record.private_data =
                crate::provider::providers::account_private_data::normalize_account_private_data(
                    record.private_data.clone(),
                );
            let mut data: crate::provider::providers::claude_code::ClaudeCodeAccountData =
                serde_json::from_value(record.private_data.clone())?;
            if crate::provider::providers::claude_code::ensure_fresh_account(&client, &mut data)
                .await?
            {
                patch.insert(
                    "accessToken".into(),
                    serde_json::Value::String(data.access_token.clone()),
                );
                if let Some(r) = &data.refresh_token {
                    patch.insert("refreshToken".into(), serde_json::Value::String(r.clone()));
                }
                if let Some(exp) = data.expires_at_ms {
                    patch.insert("expiresAtMs".into(), serde_json::Value::Number(exp.into()));
                }
            }
            match crate::provider::providers::claude_code::usage::query_usage(
                &client,
                &data.access_token,
            )
            .await
            {
                Ok(quota) => {
                    patch.insert("quota".into(), serde_json::to_value(quota)?);
                }
                Err(error) => refresh_error = Some(error),
            }
        } else if plugin_id == "dev.nexusor.plugins.nvidia-nim" {
            let api_key = record
                .private_data
                .get("apiKey")
                .or_else(|| record.private_data.get("api_key"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            Self::fetch_openai_compatible_models(
                &client,
                "https://integrate.api.nvidia.com/v1",
                api_key,
            )
            .await?;
            if !api_key.is_empty() {
                let mask = if api_key.len() > 8 {
                    format!(
                        "NVIDIA (...{})",
                        api_key
                            .chars()
                            .rev()
                            .take(6)
                            .collect::<Vec<_>>()
                            .into_iter()
                            .rev()
                            .collect::<String>()
                    )
                } else {
                    "NVIDIA Account".into()
                };
                patch.insert("displayName".into(), serde_json::Value::String(mask));
            }
        } else if plugin_id == "dev.nexusor.plugins.opencode" {
            let api_key = record
                .private_data
                .get("apiKey")
                .or_else(|| record.private_data.get("api_key"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            let base_url = record
                .private_data
                .get("baseUrl")
                .or_else(|| record.private_data.get("base_url"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or("https://opencode.ai/zen/v1");
            Self::fetch_openai_compatible_models(&client, base_url, api_key).await?;
            if !api_key.is_empty() {
                let mask = if api_key.len() > 8 {
                    format!(
                        "OpenCode (...{})",
                        api_key
                            .chars()
                            .rev()
                            .take(6)
                            .collect::<Vec<_>>()
                            .into_iter()
                            .rev()
                            .collect::<String>()
                    )
                } else {
                    "OpenCode Account".into()
                };
                patch.insert("displayName".into(), serde_json::Value::String(mask));
            }
        } else if plugin_id == "dev.nexusor.plugins.groq-lpu" {
            let api_key = record
                .private_data
                .get("apiKey")
                .or_else(|| record.private_data.get("api_key"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            Self::fetch_openai_compatible_models(
                &client,
                "https://api.groq.com/openai/v1",
                api_key,
            )
            .await?;
            if !api_key.is_empty() {
                let mask = if api_key.len() > 8 {
                    format!(
                        "Groq (...{})",
                        api_key
                            .chars()
                            .rev()
                            .take(6)
                            .collect::<Vec<_>>()
                            .into_iter()
                            .rev()
                            .collect::<String>()
                    )
                } else {
                    "Groq Account".into()
                };
                patch.insert("displayName".into(), serde_json::Value::String(mask));
            }
        } else {
            return Err(Error::Config(format!(
                "unsupported resource refresh: {plugin_id}"
            )));
        }

        let mut target_state = None;
        if let Some(quota_val) = patch.get("quota") {
            let is_exhausted = match plugin_id {
                "dev.nexusor.examples.grok-auth" | "dev.cursorbyok.examples.grok-auth" => quota_val
                    .get("remaining_percent")
                    .and_then(serde_json::Value::as_f64)
                    .is_some_and(|p| p <= 0.0),
                "dev.nexusor.examples.codex-auth" | "dev.cursorbyok.examples.codex-auth" => {
                    quota_val
                        .pointer("/primary_window/used_percent")
                        .and_then(serde_json::Value::as_f64)
                        .is_some_and(|p| p >= 100.0)
                }
                "dev.nexusor.plugins.antigravity-auth"
                | "dev.cursorbyok.plugins.antigravity-auth" => {
                    let g_empty = quota_val
                        .pointer("/gemini/remaining_percent")
                        .and_then(serde_json::Value::as_f64)
                        .is_some_and(|p| p <= 0.0);
                    let c_empty = quota_val
                        .pointer("/claude/remaining_percent")
                        .and_then(serde_json::Value::as_f64)
                        .is_some_and(|p| p <= 0.0);
                    g_empty && c_empty
                }
                "dev.nexusor.plugins.kimi-auth" | "dev.nexusor.plugins.claude-code" => quota_val
                    .get("remaining_percent")
                    .and_then(serde_json::Value::as_f64)
                    .is_some_and(|p| p <= 0.0),
                _ => false,
            };

            let now = now_ms();
            if is_exhausted {
                let reset_at_ms =
                    Self::extract_quota_reset_ms(&serde_json::json!({ "quota": quota_val }))
                        .unwrap_or(now + 3_600_000);
                let target_retry_at_ms = reset_at_ms.saturating_add(60_000);
                target_state = Some(ResourceStateInput::Cooling {
                    retry_at_ms: Some(target_retry_at_ms),
                    message: Some("upstream quota exhausted".into()),
                });
            } else if matches!(record.state, ResourceState::Cooling { .. }) {
                if let ResourceState::Cooling { retry_at_ms, .. } = record.state {
                    if retry_at_ms.is_none_or(|t| now >= t) {
                        target_state = Some(ResourceStateInput::Ready);
                    }
                }
            }
        }

        if !patch.is_empty() || target_state.is_some() {
            self.inner
                .state
                .mutate_resource(plugin_id, resource_type, resource_id, |current| {
                    current.private_data =
                crate::provider::providers::account_private_data::normalize_account_private_data(
                    current.private_data.clone(),
                );
                    if let Some(obj) = current.private_data.as_object_mut() {
                        for (k, v) in patch {
                            obj.insert(k, v);
                        }
                    }
                    // A delayed network response must not undo a user's state change.
                    if current.state == record.state
                        && !matches!(
                            current.state,
                            ResourceState::Disabled | ResourceState::Invalid { .. }
                        )
                    {
                        if let Some(state) = target_state {
                            current.state = state.into();
                        }
                    }
                    Ok(())
                })
                .await?;
        }

        match refresh_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}
