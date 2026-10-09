use super::*;

impl PluginRegistry {
    pub async fn plugins(&self) -> Vec<PluginDescriptor> {
        let executable = Path::new("native");
        let mut plugins = Vec::new();
        for entry in self.entries(executable).await {
            self.clear_orphan_models(&entry).await;
            plugins.push(self.descriptor(&entry).await);
        }
        plugins
    }

    async fn clear_orphan_models(&self, entry: &PluginEntry) {
        for provider in &entry.definition.providers {
            let Some(resource_type) = provider.resource_type.as_deref() else {
                continue;
            };
            let has_account = self
                .inner
                .state
                .resources(&entry.manifest.id, resource_type)
                .await
                .map(|records| !records.is_empty())
                .unwrap_or(false);
            if has_account {
                continue;
            }
            let has_models = self
                .inner
                .state
                .models(&entry.manifest.id, &provider.id)
                .await
                .map(|models| !models.is_empty())
                .unwrap_or(false);
            if has_models {
                let _ = self
                    .inner
                    .state
                    .replace_models(&entry.manifest.id, &provider.id, &[])
                    .await;
            }
        }
    }

    pub async fn configured_models(&self) -> Vec<PluginModelDescriptor> {
        let executable = Path::new("native");
        let mut models = Vec::new();
        for entry in self.entries(executable).await {
            for provider in &entry.definition.providers {
                if !self.provider_configured(&entry, provider).await {
                    continue;
                }
                let stored = self
                    .inner
                    .state
                    .models(&entry.manifest.id, &provider.id)
                    .await
                    .unwrap_or_default();
                models.extend(stored.iter().filter(|model| model.enabled).map(|model| {
                    PluginModelDescriptor::new(
                        &entry.manifest.id,
                        &entry.manifest.name,
                        &entry.icon,
                        provider,
                        model,
                    )
                }));
            }
        }
        models
    }

    pub async fn model_descriptor(&self, model_id: &str) -> Result<PluginModelDescriptor> {
        let (plugin_id, provider_id, upstream_id) = parse_model_id(model_id)
            .ok_or_else(|| Error::Provider(format!("invalid plugin model ID: {model_id}")))?;
        let executable = Path::new("native");
        let entry = self.find_entry(executable, plugin_id).await?;
        let provider = find_provider(&entry, provider_id)?;
        let stored = self
            .inner
            .state
            .models(plugin_id, provider_id)
            .await?
            .into_iter()
            .find(|model| model.id == upstream_id)
            .ok_or_else(|| Error::RunNotFound(format!("plugin model {model_id}")))?;
        Ok(PluginModelDescriptor::new(
            &entry.manifest.id,
            &entry.manifest.name,
            &entry.icon,
            provider,
            &stored,
        ))
    }

    pub async fn model_effort_tiers(
        &self,
        plugin_id: &str,
        provider_id: &str,
        model_id: &str,
    ) -> Vec<String> {
        self.inner
            .state
            .models(plugin_id, provider_id)
            .await
            .ok()
            .and_then(|models| models.into_iter().find(|model| model.id == model_id))
            .and_then(|model| model.private_data.get("effortTiers").cloned())
            .and_then(|value| serde_json::from_value(value).ok())
            .unwrap_or_default()
    }

    pub async fn plan_model(&self, model_id: &str) -> Result<PluginInvocationPlan> {
        let descriptor = self.model_descriptor(model_id).await?;
        let request_url = match descriptor.provider_id.as_str() {
            "codex" => crate::provider::providers::codex::RESPONSES_URL.to_string(),
            "grok" => crate::provider::providers::grok::COMPLETIONS_URL.to_string(),
            "antigravity" => crate::provider::providers::antigravity::primary_stream_url(),
            _ => "https://localhost".to_string(),
        };
        Ok(PluginInvocationPlan {
            model: descriptor,
            request_url,
        })
    }
}
