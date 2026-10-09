//! Exporting and restoring the whole system configuration.

use crate::{store::ProxySettingsInput, Error, Result};

use super::super::settings::{
    PluginResourceBackup, RestoreBackupOutcome, SystemBackupData, SystemSettingsBackup,
};
use super::ControlService;

impl ControlService {
    pub async fn export_system_backup(&self) -> Result<SystemBackupData> {
        let models = self.store.models().await?;
        let combos = self.store.router_combos().await?;
        let auto_router = Some(self.store.auto_router_config().await?);
        let proxy = self.store.proxy_settings_secret().await?;
        let settings = SystemSettingsBackup {
            desktop: Some(self.desktop_settings().await?),
            observability: Some(self.observability().await?),
            ports: Some(self.ports().await?),
            proxy: Some(ProxySettingsInput {
                mode: proxy.mode,
                address: proxy.address,
                auth_enabled: proxy.auth_enabled,
                username: proxy.username,
                password: Some(proxy.password),
            }),
            tab: Some(self.tab_settings().await?),
            commit: Some(self.commit_settings().await?),
            cmdk: Some(self.cmdk_settings().await?),
            pricing: Some(self.pricing_settings().await?),
        };

        let mut plugin_resources = Vec::new();
        let plugins = self.plugins.plugins().await;
        for p in plugins {
            for r in p.resources {
                let records = self
                    .plugins
                    .export_resources(&p.id, &r.resource_type)
                    .await?;
                if records.as_array().map(|a| !a.is_empty()).unwrap_or(false) {
                    plugin_resources.push(PluginResourceBackup {
                        plugin_id: p.id.clone(),
                        resource_type: r.resource_type.clone(),
                        records,
                    });
                }
            }
        }

        Ok(SystemBackupData {
            version: 1,
            app: "Nexusor".into(),
            exported_at: chrono::Utc::now().to_rfc3339(),
            settings,
            models,
            combos,
            auto_router,
            plugin_resources,
        })
    }

    pub async fn restore_system_backup(
        &self,
        backup: SystemBackupData,
    ) -> Result<RestoreBackupOutcome> {
        if backup.version != 1 || backup.app != "Nexusor" {
            return Err(Error::Config(
                "unsupported backup version or application".into(),
            ));
        }
        // Validate resource payloads before changing any persisted configuration.
        let resources = backup
            .plugin_resources
            .into_iter()
            .map(|item| {
                let records: Vec<crate::plugin::ResourceRecord> =
                    serde_json::from_value(item.records)?;
                Ok((item.plugin_id, item.resource_type, records))
            })
            .collect::<Result<Vec<_>>>()?;
        let mut restored_models = 0;
        let mut restored_combos = 0;
        let mut restored_resources = 0;
        let mut errors = Vec::new();
        macro_rules! restore_setting {
            ($label:expr, $operation:expr) => {
                if let Err(error) = $operation.await {
                    errors.push(format!("{}: {error}", $label));
                }
            };
        }

        // 1. Settings
        if let Some(obs) = backup.settings.observability {
            restore_setting!("observability", self.set_observability(obs));
        }
        if let Some(desktop) = backup.settings.desktop {
            restore_setting!("desktop", self.set_desktop_settings(desktop));
        }
        if let Some(tab) = backup.settings.tab {
            restore_setting!("tab", self.set_tab_settings(tab));
        }
        if let Some(commit) = backup.settings.commit {
            restore_setting!("commit", self.set_commit_settings(commit));
        }
        if let Some(cmdk) = backup.settings.cmdk {
            restore_setting!("cmdk", self.set_cmdk_settings(cmdk));
        }
        if let Some(pricing) = backup.settings.pricing {
            restore_setting!("pricing", self.set_pricing_settings(pricing));
        }
        if let Some(proxy_input) = backup.settings.proxy {
            restore_setting!("proxy", self.set_proxy_settings(proxy_input));
        }
        if let Some(ports) = backup.settings.ports {
            restore_setting!("ports", self.set_ports(ports));
        }

        // 2. Models
        if !backup.models.is_empty() {
            match self.store.restore_models(&backup.models).await {
                Ok(count) => restored_models = count,
                Err(error) => errors.push(format!("models: {error}")),
            }
        }

        let mut account_ids: std::collections::HashMap<
            String,
            std::collections::HashMap<String, String>,
        > = std::collections::HashMap::new();
        for (plugin_id, resource_type, records) in resources {
            match self
                .plugins
                .restore_resources(&plugin_id, &resource_type, records)
                .await
            {
                Ok(mapping) => {
                    restored_resources += mapping.len();
                    account_ids.entry(plugin_id).or_default().extend(mapping);
                }
                Err(error) => errors.push(format!("{plugin_id}/{resource_type}: {error}")),
            }
        }
        let remap = |slots: &mut Vec<crate::store::ComboSlot>| {
            for slot in slots {
                if let Some((plugin, _, _)) = crate::plugin::parse_model_id(&slot.model_id) {
                    if let (Some(mapping), Some(ids)) =
                        (account_ids.get(plugin), slot.account_ids.as_mut())
                    {
                        for id in ids {
                            if let Some(next) = mapping.get(id) {
                                *id = next.clone();
                            }
                        }
                    }
                }
            }
        };

        // 3. Combos
        for mut combo in backup.combos {
            remap(&mut combo.slots);
            match self.store.upsert_router_combo(combo).await {
                Ok(_) => restored_combos += 1,
                Err(error) => errors.push(format!("combo: {error}")),
            }
        }

        // 4. Auto Router
        if let Some(mut auto) = backup.auto_router {
            for slots in [
                &mut auto.coding_slots,
                &mut auto.reasoning_slots,
                &mut auto.fast_slots,
                &mut auto.vision_slots,
                &mut auto.subagent_slots,
            ] {
                remap(slots);
            }
            restore_setting!("auto router", self.store.set_auto_router_config(auto));
        }

        Ok(RestoreBackupOutcome {
            success: errors.is_empty(),
            restored_models,
            restored_combos,
            restored_resources,
            message: if errors.is_empty() {
                "Yedek başarıyla geri yüklendi.".into()
            } else {
                format!("Yedek kısmen geri yüklendi: {}", errors.join("; "))
            },
        })
    }
}
