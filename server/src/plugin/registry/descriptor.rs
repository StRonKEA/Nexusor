use super::*;

impl PluginRegistry {
    pub(crate) async fn descriptor(&self, entry: &PluginEntry) -> PluginDescriptor {
        let mut providers = Vec::new();
        for provider in &entry.definition.providers {
            let configured = self.provider_configured(entry, provider).await;
            let models = self
                .inner
                .state
                .models(&entry.manifest.id, &provider.id)
                .await
                .unwrap_or_default()
                .into_iter()
                .map(|model| {
                    PluginModelDescriptor::new(
                        &entry.manifest.id,
                        &entry.manifest.name,
                        &entry.icon,
                        provider,
                        &model,
                    )
                })
                .collect();
            providers.push(PluginProviderDescriptor {
                id: provider.id.clone(),
                plugin_id: entry.manifest.id.clone(),
                display_name: provider.display_name.clone(),
                description: provider.description.clone(),
                provider_type: provider.provider_type.clone(),
                resource_type: provider.resource_type.clone(),
                has_models: provider.has_models,
                configured,
                models,
            });
        }
        let mut resources = Vec::new();
        for resource in &entry.definition.resources {
            let views = self
                .inner
                .state
                .resources(&entry.manifest.id, &resource.resource_type)
                .await
                .unwrap_or_default()
                .into_iter()
                .map(|record| {
                    let mut metrics = Vec::new();
                    let mut desc = serde_json::json!("");

                    if let Some(quota) = record.private_data.get("quota") {
                        if entry.manifest.id == "dev.nexusor.examples.codex-auth"
                            || entry.manifest.id == "dev.cursorbyok.examples.codex-auth"
                        {
                            if let Some(plan) = quota
                                .get("plan_type")
                                .or_else(|| quota.get("planType"))
                                .and_then(serde_json::Value::as_str)
                            {
                                desc = serde_json::json!(format!(
                                    "Plan: {}",
                                    plan.to_ascii_uppercase()
                                ));
                            }

                            if let Some(primary) = quota
                                .get("primary_window")
                                .or_else(|| quota.get("primaryWindow"))
                            {
                                let used = primary
                                    .get("used_percent")
                                    .or_else(|| primary.get("usedPercent"))
                                    .and_then(serde_json::Value::as_f64)
                                    .unwrap_or(0.0);
                                let reset = primary
                                    .get("reset_at_ms")
                                    .or_else(|| primary.get("resetAtMs"))
                                    .and_then(serde_json::Value::as_i64);
                                let limit_secs = primary
                                    .get("limit_window_seconds")
                                    .or_else(|| primary.get("limitWindowSeconds"))
                                    .and_then(serde_json::Value::as_i64)
                                    .unwrap_or(18000);

                                let label = if limit_secs <= 86400 {
                                    serde_json::json!("5 Saatlik Kota")
                                } else {
                                    serde_json::json!("Haftalık Kota")
                                };

                                metrics.push(ResourceMetric {
                                    id: "primary-quota".into(),
                                    label,
                                    unit: "percent".into(),
                                    value: (100.0 - used).max(0.0),
                                    reset_at_ms: reset,
                                });
                            }
                            if let Some(sec) = quota
                                .get("secondary_window")
                                .or_else(|| quota.get("secondaryWindow"))
                            {
                                let used = sec
                                    .get("used_percent")
                                    .or_else(|| sec.get("usedPercent"))
                                    .and_then(serde_json::Value::as_f64)
                                    .unwrap_or(0.0);
                                let reset = sec
                                    .get("reset_at_ms")
                                    .or_else(|| sec.get("resetAtMs"))
                                    .and_then(serde_json::Value::as_i64);
                                let limit_secs = sec
                                    .get("limit_window_seconds")
                                    .or_else(|| sec.get("limitWindowSeconds"))
                                    .and_then(serde_json::Value::as_i64)
                                    .unwrap_or(604800);

                                let label = if limit_secs > 86400 {
                                    serde_json::json!("Haftalık Kota")
                                } else {
                                    serde_json::json!("5 Saatlik Kota")
                                };

                                metrics.push(ResourceMetric {
                                    id: "secondary-quota".into(),
                                    label,
                                    unit: "percent".into(),
                                    value: (100.0 - used).max(0.0),
                                    reset_at_ms: reset,
                                });
                            }
                            if let Some(credits) = quota
                                .get("reset_credits")
                                .or_else(|| quota.get("resetCredits"))
                                .and_then(serde_json::Value::as_i64)
                            {
                                metrics.push(ResourceMetric {
                                    id: "reset-credits".into(),
                                    label: serde_json::json!("Sıfırlama Kredisi"),
                                    unit: "count".into(),
                                    value: credits as f64,
                                    reset_at_ms: None,
                                });
                            }
                        } else if entry.manifest.id == "dev.nexusor.examples.grok-auth"
                            || entry.manifest.id == "dev.cursorbyok.examples.grok-auth"
                        {
                            let rem = quota
                                .get("remaining_percent")
                                .or_else(|| quota.get("remainingPercent"))
                                .and_then(serde_json::Value::as_f64)
                                .unwrap_or(100.0);
                            let reset = quota
                                .get("reset_at_ms")
                                .or_else(|| quota.get("resetAtMs"))
                                .and_then(serde_json::Value::as_i64);
                            metrics.push(ResourceMetric {
                                id: "credits".into(),
                                label: serde_json::json!("Haftalık Kredi"),
                                unit: "percent".into(),
                                value: rem,
                                reset_at_ms: reset,
                            });
                        } else if entry.manifest.id == "dev.nexusor.plugins.github-copilot" {
                            metrics.push(ResourceMetric {
                                id: "copilot-status".into(),
                                label: serde_json::json!("Abonelik"),
                                unit: "percent".into(),
                                value: 100.0,
                                reset_at_ms: None,
                            });
                        } else if entry.manifest.id == "dev.nexusor.plugins.antigravity-auth"
                            || entry.manifest.id == "dev.cursorbyok.plugins.antigravity-auth"
                        {
                            if let Some(plan) = quota
                                .get("plan_label")
                                .or_else(|| quota.get("planLabel"))
                                .and_then(serde_json::Value::as_str)
                            {
                                desc = serde_json::json!(format!("Plan: {plan}"));
                            }
                            if let Some(claude) =
                                quota.get("claude").and_then(serde_json::Value::as_object)
                            {
                                let rem = claude
                                    .get("remaining_percent")
                                    .or_else(|| claude.get("remainingPercent"))
                                    .and_then(serde_json::Value::as_f64)
                                    .unwrap_or(0.0);
                                let reset = claude
                                    .get("reset_at_ms")
                                    .or_else(|| claude.get("resetAtMs"))
                                    .and_then(serde_json::Value::as_i64);
                                metrics.push(ResourceMetric {
                                    id: "claude-quota".into(),
                                    label: serde_json::json!("Claude (5 Saatlik)"),
                                    unit: "percent".into(),
                                    value: rem,
                                    reset_at_ms: reset,
                                });
                            }
                            if let Some(claude_w) = quota
                                .get("claude_weekly")
                                .or_else(|| quota.get("claudeWeekly"))
                                .and_then(serde_json::Value::as_object)
                            {
                                let rem = claude_w
                                    .get("remaining_percent")
                                    .or_else(|| claude_w.get("remainingPercent"))
                                    .and_then(serde_json::Value::as_f64)
                                    .unwrap_or(0.0);
                                let reset = claude_w
                                    .get("reset_at_ms")
                                    .or_else(|| claude_w.get("resetAtMs"))
                                    .and_then(serde_json::Value::as_i64);
                                metrics.push(ResourceMetric {
                                    id: "claude-weekly-quota".into(),
                                    label: serde_json::json!("Claude (Haftalık)"),
                                    unit: "percent".into(),
                                    value: rem,
                                    reset_at_ms: reset,
                                });
                            }
                            if let Some(gemini) =
                                quota.get("gemini").and_then(serde_json::Value::as_object)
                            {
                                let rem = gemini
                                    .get("remaining_percent")
                                    .or_else(|| gemini.get("remainingPercent"))
                                    .and_then(serde_json::Value::as_f64)
                                    .unwrap_or(0.0);
                                let reset = gemini
                                    .get("reset_at_ms")
                                    .or_else(|| gemini.get("resetAtMs"))
                                    .and_then(serde_json::Value::as_i64);
                                metrics.push(ResourceMetric {
                                    id: "gemini-quota".into(),
                                    label: serde_json::json!("Gemini (5 Saatlik)"),
                                    unit: "percent".into(),
                                    value: rem,
                                    reset_at_ms: reset,
                                });
                            }
                            if let Some(gemini_w) = quota
                                .get("gemini_weekly")
                                .or_else(|| quota.get("geminiWeekly"))
                                .and_then(serde_json::Value::as_object)
                            {
                                let rem = gemini_w
                                    .get("remaining_percent")
                                    .or_else(|| gemini_w.get("remainingPercent"))
                                    .and_then(serde_json::Value::as_f64)
                                    .unwrap_or(0.0);
                                let reset = gemini_w
                                    .get("reset_at_ms")
                                    .or_else(|| gemini_w.get("resetAtMs"))
                                    .and_then(serde_json::Value::as_i64);
                                metrics.push(ResourceMetric {
                                    id: "gemini-weekly-quota".into(),
                                    label: serde_json::json!("Gemini (Haftalık)"),
                                    unit: "percent".into(),
                                    value: rem,
                                    reset_at_ms: reset,
                                });
                            }
                        } else if entry.manifest.id == "dev.nexusor.plugins.kimi-auth" {
                            if let Some(fh) =
                                quota.get("five_hour").or_else(|| quota.get("fiveHour"))
                            {
                                let used = fh
                                    .get("percent")
                                    .and_then(serde_json::Value::as_f64)
                                    .unwrap_or(0.0);
                                let reset = fh
                                    .get("reset_at_ms")
                                    .or_else(|| fh.get("resetAtMs"))
                                    .and_then(serde_json::Value::as_i64);
                                metrics.push(ResourceMetric {
                                    id: "kimi-5h".into(),
                                    label: serde_json::json!("Kimi (5 Saatlik)"),
                                    unit: "percent".into(),
                                    value: (100.0 - used).clamp(0.0, 100.0),
                                    reset_at_ms: reset,
                                });
                            }
                            if let Some(w) = quota.get("weekly") {
                                let used = w
                                    .get("percent")
                                    .and_then(serde_json::Value::as_f64)
                                    .unwrap_or(0.0);
                                let reset = w
                                    .get("reset_at_ms")
                                    .or_else(|| w.get("resetAtMs"))
                                    .and_then(serde_json::Value::as_i64);
                                metrics.push(ResourceMetric {
                                    id: "kimi-weekly".into(),
                                    label: serde_json::json!("Kimi (Haftalık)"),
                                    unit: "percent".into(),
                                    value: (100.0 - used).clamp(0.0, 100.0),
                                    reset_at_ms: reset,
                                });
                            }
                        } else if entry.manifest.id == "dev.nexusor.plugins.claude-code" {
                            if let Some(fh) =
                                quota.get("five_hour").or_else(|| quota.get("fiveHour"))
                            {
                                let used = fh
                                    .get("percent")
                                    .and_then(serde_json::Value::as_f64)
                                    .unwrap_or(0.0);
                                let reset = fh
                                    .get("reset_at_ms")
                                    .or_else(|| fh.get("resetAtMs"))
                                    .and_then(serde_json::Value::as_i64);
                                metrics.push(ResourceMetric {
                                    id: "claude-code-5h".into(),
                                    label: serde_json::json!("Claude Code (5 Saatlik)"),
                                    unit: "percent".into(),
                                    value: (100.0 - used).clamp(0.0, 100.0),
                                    reset_at_ms: reset,
                                });
                            }
                            if let Some(w) = quota.get("weekly") {
                                let used = w
                                    .get("percent")
                                    .and_then(serde_json::Value::as_f64)
                                    .unwrap_or(0.0);
                                let reset = w
                                    .get("reset_at_ms")
                                    .or_else(|| w.get("resetAtMs"))
                                    .and_then(serde_json::Value::as_i64);
                                metrics.push(ResourceMetric {
                                    id: "claude-code-weekly".into(),
                                    label: serde_json::json!("Claude Code (Haftalık)"),
                                    unit: "percent".into(),
                                    value: (100.0 - used).clamp(0.0, 100.0),
                                    reset_at_ms: reset,
                                });
                            }
                            if let Some(s) = quota
                                .get("sonnet_weekly")
                                .or_else(|| quota.get("sonnetWeekly"))
                            {
                                let used = s
                                    .get("percent")
                                    .and_then(serde_json::Value::as_f64)
                                    .unwrap_or(0.0);
                                let reset = s
                                    .get("reset_at_ms")
                                    .or_else(|| s.get("resetAtMs"))
                                    .and_then(serde_json::Value::as_i64);
                                metrics.push(ResourceMetric {
                                    id: "claude-code-sonnet".into(),
                                    label: serde_json::json!("Sonnet (Haftalık)"),
                                    unit: "percent".into(),
                                    value: (100.0 - used).clamp(0.0, 100.0),
                                    reset_at_ms: reset,
                                });
                            }
                        } else if entry.manifest.id == "dev.nexusor.plugins.nvidia-nim" {
                            metrics.push(ResourceMetric {
                                id: "nvidia-status".into(),
                                label: serde_json::json!("NVIDIA NIM Kredisi"),
                                unit: "status".into(),
                                value: 100.0,
                                reset_at_ms: None,
                            });
                        } else if entry.manifest.id == "dev.nexusor.plugins.opencode" {
                            metrics.push(ResourceMetric {
                                id: "opencode-status".into(),
                                label: serde_json::json!("OpenCode Durumu"),
                                unit: "status".into(),
                                value: 100.0,
                                reset_at_ms: None,
                            });
                        } else if entry.manifest.id == "dev.nexusor.plugins.groq-lpu" {
                            metrics.push(ResourceMetric {
                                id: "groq-status".into(),
                                label: serde_json::json!("Groq LPU Motoru"),
                                unit: "status".into(),
                                value: 100.0,
                                reset_at_ms: None,
                            });
                        }
                    }

                    let display_name = resource_display_name(&record);
                    let note = record
                        .private_data
                        .get("user_note")
                        .or_else(|| record.private_data.get("userNote"))
                        .or_else(|| record.private_data.get("note"))
                        .and_then(serde_json::Value::as_str)
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(String::from);

                    PluginResourceView {
                        id: record.id.clone(),
                        state: record.state,
                        display_name,
                        description: desc,
                        metrics,
                        created_at_ms: record.created_at_ms,
                        note,
                    }
                })
                .collect();
            resources.push(PluginResourceDescriptor {
                resource_type: resource.resource_type.clone(),
                display_name: resource.display_name.clone(),
                add: resource.add.clone(),
                import: resource.import.clone(),
                actions: resource.actions.clone(),
                can_refresh: resource.can_refresh,
                can_remove: resource.can_remove,
                resources: views,
            });
        }
        PluginDescriptor {
            id: entry.manifest.id.clone(),
            name: entry.manifest.name.clone(),
            version: entry.manifest.version.clone(),
            author: entry.manifest.author.clone(),
            icon: entry.icon.clone(),
            providers,
            resources,
        }
    }

    pub(crate) async fn provider_configured(
        &self,
        entry: &PluginEntry,
        provider: &ProviderDefinition,
    ) -> bool {
        let has_models = self
            .inner
            .state
            .models(&entry.manifest.id, &provider.id)
            .await
            .map(|models| !models.is_empty())
            .unwrap_or(false);
        if !has_models {
            return false;
        }
        match &provider.resource_type {
            Some(resource_type) => self
                .inner
                .state
                .resources(&entry.manifest.id, resource_type)
                .await
                .map(|records| !records.is_empty())
                .unwrap_or(false),
            None => true,
        }
    }

    pub(crate) async fn entries(&self, _executable: &Path) -> Vec<PluginEntry> {
        if let Some(entries) = self.inner.entries.read().await.as_ref() {
            return entries.clone();
        }
        let loaded = self.inner.catalog.entries(Path::new("native")).await;
        *self.inner.entries.write().await = Some(loaded.clone());
        loaded
    }

    pub(crate) async fn find_entry(
        &self,
        executable: &Path,
        plugin_id: &str,
    ) -> Result<PluginEntry> {
        self.entries(executable)
            .await
            .into_iter()
            .find(|entry| entry.manifest.id == plugin_id)
            .ok_or_else(|| Error::RunNotFound(format!("plugin {plugin_id}")))
    }
}

pub(crate) fn find_provider<'a>(
    entry: &'a PluginEntry,
    provider_id: &str,
) -> Result<&'a ProviderDefinition> {
    entry
        .definition
        .providers
        .iter()
        .find(|provider| provider.id == provider_id)
        .ok_or_else(|| {
            Error::RunNotFound(format!(
                "plugin {} provider {}",
                entry.manifest.id, provider_id
            ))
        })
}

fn resource_display_name(record: &ResourceRecord) -> String {
    let from_private = record
        .private_data
        .get("displayName")
        .or_else(|| record.private_data.get("display_name"))
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);

    if let Some(name) = from_private {
        if let Some(email) = name
            .strip_prefix("Google (")
            .and_then(|value| value.strip_suffix(')'))
            .filter(|value| !value.is_empty())
        {
            return email.to_owned();
        }
        return name;
    }

    for prefix in ["antigravity:", "codex:", "grok:"] {
        if let Some(rest) = record.key.strip_prefix(prefix) {
            if !rest.is_empty() {
                return rest.to_owned();
            }
        }
    }
    record.key.clone()
}
