use super::*;

impl PluginRegistry {
    pub async fn import_resources(
        &self,
        plugin_id: &str,
        resource_type: &str,
        files: serde_json::Value,
    ) -> Result<ImportResponse> {
        let mut added = 0;
        let mut updated = 0;
        let mut warnings = Vec::new();

        // Files payload can be array of { name: String, content: String } or direct array of records
        let records_to_import: Vec<serde_json::Value> = if let Some(arr) = files.as_array() {
            let mut extracted = Vec::new();
            for item in arr {
                if let Some(content_str) = item.get("content").and_then(serde_json::Value::as_str) {
                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(content_str) {
                        if let Some(inner_arr) = parsed.as_array() {
                            extracted.extend(inner_arr.clone());
                        } else if parsed.is_object() {
                            extracted.push(parsed);
                        }
                    }
                } else if item.is_object() {
                    extracted.push(item.clone());
                }
            }
            extracted
        } else if files.is_object() {
            vec![files]
        } else {
            Vec::new()
        };

        if plugin_id == "dev.nexusor.plugins.claude-code" && records_to_import.is_empty() {
            if let Some((access, refresh, expires)) =
                crate::provider::providers::claude_code::oauth::detect_local_credentials()
            {
                let data = serde_json::json!({
                    "accessToken": access,
                    "refreshToken": refresh,
                    "expiresAtMs": expires,
                    "displayName": "Claude (Local CLI)",
                });
                self.inner
                    .state
                    .upsert_resources(
                        plugin_id,
                        resource_type,
                        vec![ResourceDraft {
                            key: "claude:local".into(),
                            private_data: data,
                            state: None,
                        }],
                    )
                    .await?;
                let _ = self.sync_models(plugin_id, "claude-code").await;
                return Ok(ImportResponse {
                    added: 1,
                    updated: 0,
                    warnings: Vec::new(),
                    model_sync_error: None,
                });
            }
        }

        for val in records_to_import {
            let key = val.get("key").and_then(serde_json::Value::as_str);
            let private_data = val.get("private_data").or_else(|| val.get("privateData"));
            if let (Some(key), Some(data)) = (key, private_data) {
                let existing = self
                    .inner
                    .state
                    .resources(plugin_id, resource_type)
                    .await
                    .unwrap_or_default();
                let exists = existing.iter().any(|r| r.key == key);
                let draft = ResourceDraft {
                    key: key.to_string(),
                    private_data: data.clone(),
                    state: None,
                };
                if self
                    .inner
                    .state
                    .upsert_resources(plugin_id, resource_type, vec![draft])
                    .await
                    .is_ok()
                {
                    if exists {
                        updated += 1;
                    } else {
                        added += 1;
                    }
                }
            } else {
                warnings.push("Geçersiz kaynak formatı: key veya private_data eksik".into());
            }
        }

        // Auto-refresh plugin models and quota after import
        let model_sync_error =
            if let Some(provider_id) = provider_id_for_resource(plugin_id, resource_type) {
                self.sync_models(plugin_id, provider_id)
                    .await
                    .err()
                    .map(|e| e.to_string())
            } else {
                None
            };

        Ok(ImportResponse {
            added,
            updated,
            warnings,
            model_sync_error,
        })
    }

    pub async fn export_resources(
        &self,
        plugin_id: &str,
        resource_type: &str,
    ) -> Result<serde_json::Value> {
        let records = self.inner.state.resources(plugin_id, resource_type).await?;
        Ok(serde_json::to_value(&records)?)
    }

    pub async fn restore_resources(
        &self,
        plugin_id: &str,
        resource_type: &str,
        records: Vec<ResourceRecord>,
    ) -> Result<HashMap<String, String>> {
        self.inner
            .state
            .restore_resources(plugin_id, resource_type, records)
            .await
    }

    pub async fn refresh_all_quotas(&self) -> Result<()> {
        let plugins = self.plugins().await;
        for plugin in plugins {
            for res_group in plugin.resources {
                for res in res_group.resources {
                    let _ = self
                        .refresh_resource(&plugin.id, &res_group.resource_type, &res.id)
                        .await;
                }
            }
        }
        Ok(())
    }

    pub fn start_background_quota_poller(&self) {
        let registry = self.clone();
        tokio::spawn(async move {
            // Initial probe 1s after startup (immediate check so user gets live status right away)
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            loop {
                tracing::info!("running periodic background quota poller for native accounts");
                if let Err(e) = registry.refresh_all_quotas().await {
                    tracing::debug!(error = %e, "periodic quota poller encounter");
                }
                // Poll every 5 minutes (300 seconds)
                tokio::time::sleep(std::time::Duration::from_secs(300)).await;
            }
        });
    }

    pub async fn delete_resource(
        &self,
        plugin_id: &str,
        resource_type: &str,
        resource_id: &str,
    ) -> Result<()> {
        let _ = self
            .inner
            .state
            .remove_resource(plugin_id, resource_type, resource_id)
            .await?;
        let remaining = self
            .inner
            .state
            .resources(plugin_id, resource_type)
            .await
            .unwrap_or_default();
        if remaining.is_empty() {
            if let Some(provider_id) = provider_id_for_resource(plugin_id, resource_type) {
                self.inner
                    .state
                    .replace_models(plugin_id, provider_id, &[])
                    .await?;
            }
        }
        Ok(())
    }

    pub async fn patch_resource(
        &self,
        plugin_id: &str,
        resource_type: &str,
        resource_id: &str,
        patch: ResourcePatch,
    ) -> Result<()> {
        self.inner
            .state
            .apply_patch(plugin_id, resource_type, resource_id, patch)
            .await
    }

    pub async fn wakeup_resource(
        &self,
        plugin_id: &str,
        resource_type: &str,
        resource_id: &str,
    ) -> Result<String> {
        let client = self.client().await?;
        if plugin_id == "dev.nexusor.plugins.antigravity-auth"
            || plugin_id == "dev.cursorbyok.plugins.antigravity-auth"
        {
            let mut records = self.inner.state.resources(plugin_id, resource_type).await?;
            let Some(record) = records.iter_mut().find(|r| r.id == resource_id) else {
                return Err(Error::RunNotFound(format!("resource {resource_id}")));
            };
            record.private_data =
                crate::provider::providers::account_private_data::normalize_account_private_data(
                    record.private_data.clone(),
                );
            let mut data =
                crate::provider::providers::antigravity::AntigravityAccountData::from_record(
                    record,
                )
                .ok_or_else(|| Error::Provider("Antigravity hesap verisi okunamadı".into()))?;
            let _ = crate::provider::providers::antigravity::tokens::ensure_fresh_account(
                &client, &mut data,
            )
            .await;
            let project = data.project_id.clone().unwrap_or_default();
            crate::provider::providers::antigravity::send_wakeup_ping(
                &client,
                &data.access_token,
                &project,
            )
            .await?;
            let _ = self
                .refresh_resource(plugin_id, resource_type, resource_id)
                .await;
            Ok("Antigravity hesabı başarıyla uyandırıldı! 5 saatlik kota sıfırlama sayacı başlatıldı.".into())
        } else if plugin_id == "dev.nexusor.examples.codex-auth"
            || plugin_id == "dev.cursorbyok.examples.codex-auth"
        {
            let _ = self
                .refresh_resource(plugin_id, resource_type, resource_id)
                .await;
            Ok("Codex hesabı başarıyla uyandırıldı ve kotalar senkronize edildi.".into())
        } else {
            let _ = self
                .refresh_resource(plugin_id, resource_type, resource_id)
                .await;
            Ok("Hesap başarıyla uyandırıldı ve kotalar senkronize edildi.".into())
        }
    }
}
