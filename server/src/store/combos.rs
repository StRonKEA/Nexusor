//! Persists model display customization and smart combo routing rules.

use serde::{Deserialize, Serialize};
use sqlx::Row;

use super::{now_ms, Store};
use crate::Result;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct ModelCustomization {
    pub model_id: String,
    pub custom_name: Option<String>,
    pub enabled: bool,
    pub updated_at_ms: i64,
}

/// Per-slot configuration inside a combo.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct ComboSlot {
    /// Plugin model ID or builtin model_hash.
    pub model_id: String,
    /// Specific account/resource IDs to use for this slot.
    /// `None` or empty = use all active accounts (plugin-global pool strategy).
    pub account_ids: Option<Vec<String>>,
    /// How to rotate among the selected accounts for this slot.
    /// "failover" | "round_robin" | "single"
    pub slot_strategy: String,
}

impl ComboSlot {
    pub fn pool_strategy(&self) -> Option<crate::plugin::PoolStrategy> {
        use crate::plugin::PoolStrategy;
        match self.slot_strategy.as_str() {
            "failover" | "fallback" => Some(PoolStrategy::Failover),
            "round_robin" => Some(PoolStrategy::RoundRobin),
            "most_quota" => Some(PoolStrategy::MostQuota),
            "fill_first" => Some(PoolStrategy::FillFirst),
            "single" => Some(PoolStrategy::Single),
            _ => None,
        }
    }
    pub fn simple(model_id: impl Into<String>) -> Self {
        Self {
            model_id: model_id.into(),
            account_ids: None,
            slot_strategy: "failover".into(),
        }
    }
}

fn default_true() -> bool {
    true
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct AutoRouterConfig {
    pub enabled: bool,
    #[serde(default)]
    pub coding_slots: Vec<ComboSlot>,
    #[serde(default)]
    pub reasoning_slots: Vec<ComboSlot>,
    #[serde(default)]
    pub fast_slots: Vec<ComboSlot>,
    #[serde(default = "default_true")]
    pub vision_auto: bool,
    #[serde(default)]
    pub vision_slots: Vec<ComboSlot>,
    #[serde(default = "default_true")]
    pub subagent_auto: bool,
    #[serde(default)]
    pub subagent_slots: Vec<ComboSlot>,
    #[serde(default = "default_true")]
    pub subagent_write_access: bool,
    pub updated_at_ms: i64,
}

impl AutoRouterConfig {
    pub const SUBAGENT_ROUTE: &'static str = "nexusor-subagent";

    pub fn manual_subagent_route(&self) -> Option<String> {
        (!self.subagent_auto && !self.subagent_slots.is_empty())
            .then(|| Self::SUBAGENT_ROUTE.to_owned())
    }
}

impl Default for AutoRouterConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            coding_slots: Vec::new(),
            reasoning_slots: Vec::new(),
            fast_slots: Vec::new(),
            vision_auto: true,
            vision_slots: Vec::new(),
            subagent_auto: true,
            subagent_slots: Vec::new(),
            subagent_write_access: true,
            updated_at_ms: 0,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct RouterCombo {
    pub combo_id: String,
    pub name: String,
    pub description: Option<String>,
    /// Chain-level strategy: "fallback" | "round_robin"
    pub strategy: String,
    /// Legacy flat model list (kept for backward compat). Derived from slots.
    #[serde(default)]
    pub models: Vec<String>,
    /// Rich per-slot config. Takes precedence over `models` when present.
    #[serde(default)]
    pub slots: Vec<ComboSlot>,
    pub enabled: bool,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

impl Store {
    pub async fn model_customizations(&self) -> Result<Vec<ModelCustomization>> {
        let rows = sqlx::query(
            "SELECT model_id, custom_name, enabled, updated_at_ms FROM model_customizations",
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| ModelCustomization {
                model_id: r.get(0),
                custom_name: r.get(1),
                enabled: r.get::<i64, _>(2) != 0,
                updated_at_ms: r.get(3),
            })
            .collect())
    }

    pub async fn set_model_customization(
        &self,
        model_id: &str,
        custom_name: Option<&str>,
        enabled: bool,
    ) -> Result<ModelCustomization> {
        let _write = self.writes.lock().await;
        let now = now_ms();
        sqlx::query(
            "INSERT INTO model_customizations(model_id, custom_name, enabled, updated_at_ms)
             VALUES (?, ?, ?, ?)
             ON CONFLICT(model_id) DO UPDATE SET
                custom_name = excluded.custom_name,
                enabled = excluded.enabled,
                updated_at_ms = excluded.updated_at_ms",
        )
        .bind(model_id)
        .bind(custom_name)
        .bind(if enabled { 1 } else { 0 })
        .bind(now)
        .execute(&self.pool)
        .await?;

        Ok(ModelCustomization {
            model_id: model_id.to_string(),
            custom_name: custom_name.map(str::to_string),
            enabled,
            updated_at_ms: now,
        })
    }

    pub async fn router_combos(&self) -> Result<Vec<RouterCombo>> {
        let rows = sqlx::query(
            "SELECT combo_id, name, description, strategy, models_json, enabled, created_at_ms, updated_at_ms, slots_json
             FROM router_combos ORDER BY created_at_ms ASC"
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| {
                let models_json: String = r.get(4);
                let models: Vec<String> = serde_json::from_str(&models_json).unwrap_or_default();
                // slots_json: nullable column added in migration
                let slots_json: Option<String> = r.try_get(8).ok().flatten();
                let slots: Vec<ComboSlot> = slots_json
                    .and_then(|j| serde_json::from_str(&j).ok())
                    .unwrap_or_else(|| models.iter().map(ComboSlot::simple).collect());
                RouterCombo {
                    combo_id: r.get(0),
                    name: r.get(1),
                    description: r.get(2),
                    strategy: r.get(3),
                    models: slots.iter().map(|s| s.model_id.clone()).collect(),
                    slots,
                    enabled: r.get::<i64, _>(5) != 0,
                    created_at_ms: r.get(6),
                    updated_at_ms: r.get(7),
                }
            })
            .collect())
    }

    pub async fn upsert_router_combo(&self, combo: RouterCombo) -> Result<RouterCombo> {
        let _write = self.writes.lock().await;
        let now = now_ms();
        // Derive slots from models if slots is empty (backward compat)
        let slots = if combo.slots.is_empty() {
            combo
                .models
                .iter()
                .map(ComboSlot::simple)
                .collect::<Vec<_>>()
        } else {
            combo.slots.clone()
        };
        let models_json =
            serde_json::to_string(&slots.iter().map(|s| &s.model_id).collect::<Vec<_>>())?;
        let slots_json = serde_json::to_string(&slots)?;
        sqlx::query(
            "INSERT INTO router_combos(combo_id, name, description, strategy, models_json, enabled, created_at_ms, updated_at_ms, slots_json)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(combo_id) DO UPDATE SET
                name = excluded.name,
                description = excluded.description,
                strategy = excluded.strategy,
                models_json = excluded.models_json,
                slots_json = excluded.slots_json,
                enabled = excluded.enabled,
                updated_at_ms = excluded.updated_at_ms"
        )
        .bind(&combo.combo_id)
        .bind(&combo.name)
        .bind(&combo.description)
        .bind(&combo.strategy)
        .bind(&models_json)
        .bind(if combo.enabled { 1 } else { 0 })
        .bind(combo.created_at_ms.max(now))
        .bind(now)
        .bind(&slots_json)
        .execute(&self.pool)
        .await?;

        let mut saved = combo;
        saved.slots = slots.clone();
        saved.models = slots.iter().map(|s| s.model_id.clone()).collect();
        saved.updated_at_ms = now;
        Ok(saved)
    }

    pub async fn delete_router_combo(&self, combo_id: &str) -> Result<()> {
        let _write = self.writes.lock().await;
        sqlx::query("DELETE FROM router_combos WHERE combo_id = ?")
            .bind(combo_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn auto_router_config(&self) -> Result<AutoRouterConfig> {
        let row = sqlx::query(
            "SELECT enabled, coding_slots_json, reasoning_slots_json, fast_slots_json, updated_at_ms,
                    vision_auto, vision_slots_json, subagent_auto, subagent_slots_json, subagent_write_access
             FROM cursor_auto_router WHERE id = 'default'",
        )
        .fetch_optional(&self.pool)
        .await?;

        if let Some(r) = row {
            let enabled = r.get::<i64, _>(0) != 0;
            let coding_json: String = r.get(1);
            let reasoning_json: String = r.get(2);
            let fast_json: String = r.get(3);
            let updated_at_ms: i64 = r.get(4);
            let vision_auto = r.try_get::<i64, _>(5).map(|v| v != 0).unwrap_or(true);
            let vision_json: String = r.try_get(6).unwrap_or_else(|_| "[]".into());
            let subagent_auto = r.try_get::<i64, _>(7).map(|v| v != 0).unwrap_or(true);
            let subagent_json: String = r.try_get(8).unwrap_or_else(|_| "[]".into());
            let subagent_write_access = r.try_get::<i64, _>(9).map(|v| v != 0).unwrap_or(true);

            let coding_slots = serde_json::from_str(&coding_json).unwrap_or_default();
            let reasoning_slots = serde_json::from_str(&reasoning_json).unwrap_or_default();
            let fast_slots = serde_json::from_str(&fast_json).unwrap_or_default();
            let vision_slots = serde_json::from_str(&vision_json).unwrap_or_default();
            let subagent_slots = serde_json::from_str(&subagent_json).unwrap_or_default();

            Ok(AutoRouterConfig {
                enabled,
                coding_slots,
                reasoning_slots,
                fast_slots,
                vision_auto,
                vision_slots,
                subagent_auto,
                subagent_slots,
                subagent_write_access,
                updated_at_ms,
            })
        } else {
            Ok(AutoRouterConfig::default())
        }
    }

    pub async fn set_auto_router_config(
        &self,
        config: AutoRouterConfig,
    ) -> Result<AutoRouterConfig> {
        let _write = self.writes.lock().await;
        let now = now_ms();
        let coding_json =
            serde_json::to_string(&config.coding_slots).unwrap_or_else(|_| "[]".into());
        let reasoning_json =
            serde_json::to_string(&config.reasoning_slots).unwrap_or_else(|_| "[]".into());
        let fast_json = serde_json::to_string(&config.fast_slots).unwrap_or_else(|_| "[]".into());
        let vision_json =
            serde_json::to_string(&config.vision_slots).unwrap_or_else(|_| "[]".into());
        let subagent_json =
            serde_json::to_string(&config.subagent_slots).unwrap_or_else(|_| "[]".into());

        sqlx::query(
            "INSERT INTO cursor_auto_router(id, enabled, coding_slots_json, reasoning_slots_json, fast_slots_json, updated_at_ms,
                                            vision_auto, vision_slots_json, subagent_auto, subagent_slots_json, subagent_write_access)
             VALUES ('default', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET
                enabled = excluded.enabled,
                coding_slots_json = excluded.coding_slots_json,
                reasoning_slots_json = excluded.reasoning_slots_json,
                fast_slots_json = excluded.fast_slots_json,
                updated_at_ms = excluded.updated_at_ms,
                vision_auto = excluded.vision_auto,
                vision_slots_json = excluded.vision_slots_json,
                subagent_auto = excluded.subagent_auto,
                subagent_slots_json = excluded.subagent_slots_json,
                subagent_write_access = excluded.subagent_write_access",
        )
        .bind(if config.enabled { 1 } else { 0 })
        .bind(&coding_json)
        .bind(&reasoning_json)
        .bind(&fast_json)
        .bind(now)
        .bind(if config.vision_auto { 1 } else { 0 })
        .bind(&vision_json)
        .bind(if config.subagent_auto { 1 } else { 0 })
        .bind(&subagent_json)
        .bind(if config.subagent_write_access { 1 } else { 0 })
        .execute(&self.pool)
        .await?;

        Ok(AutoRouterConfig {
            enabled: config.enabled,
            coding_slots: config.coding_slots,
            reasoning_slots: config.reasoning_slots,
            fast_slots: config.fast_slots,
            vision_auto: config.vision_auto,
            vision_slots: config.vision_slots,
            subagent_auto: config.subagent_auto,
            subagent_slots: config.subagent_slots,
            subagent_write_access: config.subagent_write_access,
            updated_at_ms: now,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn model_customizations_round_trip() {
        let directory = tempfile::tempdir().unwrap();
        let url = format!("sqlite://{}", directory.path().join("test.db").display());
        let store = Store::connect(&url).await.unwrap();

        let cust = store
            .set_model_customization("claude-3-7-sonnet", Some("Claude 3.7 Özel"), false)
            .await
            .unwrap();
        assert_eq!(cust.custom_name.as_deref(), Some("Claude 3.7 Özel"));
        assert!(!cust.enabled);

        let list = store.model_customizations().await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].model_id, "claude-3-7-sonnet");
        assert_eq!(list[0].custom_name.as_deref(), Some("Claude 3.7 Özel"));
        assert!(!list[0].enabled);
    }

    #[tokio::test]
    async fn router_combos_crud_round_trip() {
        let directory = tempfile::tempdir().unwrap();
        let url = format!("sqlite://{}", directory.path().join("test.db").display());
        let store = Store::connect(&url).await.unwrap();

        let combo = RouterCombo {
            combo_id: "coding-chain".into(),
            name: "Kodlama Zinciri".into(),
            description: Some("Claude Sonnet biterse Gemini 2.5 Pro".into()),
            strategy: "fallback".into(),
            models: vec!["claude-3-7-sonnet".into(), "gemini-2.5-pro".into()],
            slots: Vec::new(),
            enabled: true,
            created_at_ms: 0,
            updated_at_ms: 0,
        };

        let saved = store.upsert_router_combo(combo.clone()).await.unwrap();
        assert_eq!(saved.name, "Kodlama Zinciri");

        let combos = store.router_combos().await.unwrap();
        assert_eq!(combos.len(), 1);
        assert_eq!(combos[0].models.len(), 2);

        store.delete_router_combo("coding-chain").await.unwrap();
        let empty = store.router_combos().await.unwrap();
        assert!(empty.is_empty());
    }
}
