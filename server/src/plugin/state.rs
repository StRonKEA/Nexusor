//! Owns core-side persistence of plugin resources and model catalogs.
use serde::{Deserialize, Serialize};

use super::data::PluginDataStore;
use crate::{Error, Result};

/// 核心理解的资源运行状态;插件只能通过 draft/patch/report 改变它。
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ResourceState {
    Ready,
    Cooling {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        retry_at_ms: Option<i64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },
    Invalid {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },
    Disabled,
}

impl ResourceState {
    /// 冷却到期后自动恢复可用。
    pub fn is_ready(&self, now_ms: i64) -> bool {
        match self {
            Self::Ready => true,
            Self::Cooling { retry_at_ms, .. } => retry_at_ms.is_some_and(|at| at <= now_ms),
            Self::Invalid { .. } => false,
            Self::Disabled => false,
        }
    }
}

/// 核心持久化的一条插件资源。`private_data` 只回传给插件。
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ResourceRecord {
    pub id: String,
    pub key: String,
    pub private_data: serde_json::Value,
    pub state: ResourceState,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

impl ResourceRecord {
    /// 传给插件的快照形状(SDK 的 ResourceSnapshot)。
    pub fn snapshot(&self, resource_type: &str) -> serde_json::Value {
        serde_json::json!({
            "id": self.id,
            "type": resource_type,
            "key": self.key,
            "privateData": self.private_data,
            "state": state_json(&self.state),
        })
    }
}

fn state_json(state: &ResourceState) -> serde_json::Value {
    match state {
        ResourceState::Ready => serde_json::json!({ "status": "ready" }),
        ResourceState::Cooling {
            retry_at_ms,
            message,
        } => serde_json::json!({
            "status": "cooling",
            "retryAtMs": retry_at_ms,
            "message": message,
        }),
        ResourceState::Invalid { message } => serde_json::json!({
            "status": "invalid",
            "message": message,
        }),
        ResourceState::Disabled => serde_json::json!({
            "status": "disabled",
        }),
    }
}

/// 插件返回的新资源(SDK 的 ResourceDraft)。
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResourceDraft {
    pub key: String,
    pub private_data: serde_json::Value,
    #[serde(default)]
    pub state: Option<ResourceStateInput>,
}

/// 插件对单条资源的部分更新(SDK 的 ResourcePatch)。
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResourcePatch {
    #[serde(default)]
    pub private_data: Option<serde_json::Value>,
    #[serde(default)]
    pub state: Option<ResourceStateInput>,
}

/// SDK 侧 camelCase 状态输入,转换成核心存储形状。
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ResourceStateInput {
    Ready,
    Cooling {
        #[serde(default, rename = "retryAtMs")]
        retry_at_ms: Option<i64>,
        #[serde(default)]
        message: Option<String>,
    },
    Invalid {
        #[serde(default)]
        message: Option<String>,
    },
    Disabled,
}

impl From<ResourceStateInput> for ResourceState {
    fn from(input: ResourceStateInput) -> Self {
        match input {
            ResourceStateInput::Ready => Self::Ready,
            ResourceStateInput::Cooling {
                retry_at_ms,
                message,
            } => Self::Cooling {
                retry_at_ms,
                message,
            },
            ResourceStateInput::Invalid { message } => Self::Invalid { message },
            ResourceStateInput::Disabled => Self::Disabled,
        }
    }
}

/// 插件发现的一个模型(SDK 的 ModelDefinition),由核心整体替换目录。
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct StoredModel {
    pub id: String,
    pub display_name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub max_output_tokens: Option<u64>,
    #[serde(default)]
    pub images: bool,
    #[serde(default = "default_model_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub private_data: serde_json::Value,
}

fn default_model_enabled() -> bool {
    true
}

impl StoredModel {
    pub fn from_definition(value: &serde_json::Value) -> Result<Self> {
        let object = value
            .as_object()
            .ok_or_else(|| Error::Protocol("plugin model definition must be an object".into()))?;
        let id = object
            .get("id")
            .and_then(serde_json::Value::as_str)
            .filter(|id| !id.trim().is_empty())
            .ok_or_else(|| Error::Protocol("plugin model definition requires id".into()))?;
        let display_name = object
            .get("displayName")
            .and_then(serde_json::Value::as_str)
            .filter(|name| !name.trim().is_empty())
            .ok_or_else(|| {
                Error::Protocol("plugin model definition requires displayName".into())
            })?;
        let capabilities = object
            .get("capabilities")
            .and_then(|value| value.as_object());
        let capability = |name: &str| {
            capabilities
                .and_then(|value| value.get(name))
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
        };
        Ok(Self {
            id: id.to_owned(),
            display_name: display_name.to_owned(),
            description: object
                .get("description")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned),
            max_output_tokens: object
                .get("maxOutputTokens")
                .and_then(serde_json::Value::as_u64),
            images: capability("images"),
            enabled: true,
            private_data: object
                .get("privateData")
                .cloned()
                .unwrap_or(serde_json::Value::Null),
        })
    }

    /// 传给插件的模型快照(SDK 的 ModelSnapshot)。
    pub fn snapshot(&self) -> serde_json::Value {
        serde_json::json!({
            "id": self.id,
            "displayName": self.display_name,
            "description": self.description,
            "maxOutputTokens": self.max_output_tokens,
            "capabilities": { "images": self.images },
            "privateData": self.private_data,
        })
    }
}

const POOL_STRATEGY_KEY: &str = "pool_strategy.json";

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PoolStrategy {
    #[default]
    Failover,
    RoundRobin,
    MostQuota,
    FillFirst,
    Single,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PoolStrategyConfig {
    pub strategy: PoolStrategy,
}

/// 资源与模型目录的核心存储,构建在插件私有 JSON 文件之上。
#[derive(Clone)]
pub struct PluginStateStore {
    data: PluginDataStore,
    operations: std::sync::Arc<tokio::sync::Mutex<()>>,
}

pub struct UpsertOutcome {
    pub added: usize,
    pub updated: usize,
}

impl PluginStateStore {
    pub fn new(data: PluginDataStore) -> Self {
        Self {
            data,
            operations: Default::default(),
        }
    }

    pub async fn pool_strategy(&self, plugin_id: &str) -> Result<PoolStrategy> {
        let _operation = self.operations.lock().await;
        let mut value = self.data.read(plugin_id, POOL_STRATEGY_KEY).await?;
        if value.is_null() {
            if let Some(suffix) = plugin_id.strip_prefix("dev.nexusor.") {
                let legacy_id = format!("dev.cursorbyok.{suffix}");
                if let Ok(legacy_val) = self.data.read(&legacy_id, POOL_STRATEGY_KEY).await {
                    if !legacy_val.is_null() {
                        let _ = self
                            .data
                            .update(plugin_id, POOL_STRATEGY_KEY, &legacy_val)
                            .await;
                        value = legacy_val;
                    }
                }
            }
        }
        if value.is_null() {
            return Ok(PoolStrategy::default());
        }
        let config: PoolStrategyConfig = serde_json::from_value(value)?;
        Ok(config.strategy)
    }

    pub async fn set_pool_strategy(&self, plugin_id: &str, strategy: PoolStrategy) -> Result<()> {
        let _operation = self.operations.lock().await;
        let config = PoolStrategyConfig { strategy };
        self.data
            .update(plugin_id, POOL_STRATEGY_KEY, &serde_json::to_value(config)?)
            .await
    }

    pub async fn resources(
        &self,
        plugin_id: &str,
        resource_type: &str,
    ) -> Result<Vec<ResourceRecord>> {
        let _operation = self.operations.lock().await;
        self.resources_unlocked(plugin_id, resource_type).await
    }

    async fn resources_unlocked(
        &self,
        plugin_id: &str,
        resource_type: &str,
    ) -> Result<Vec<ResourceRecord>> {
        let mut value = self
            .data
            .read(plugin_id, &resource_key(resource_type))
            .await?;
        if value.is_null() {
            if let Some(suffix) = plugin_id.strip_prefix("dev.nexusor.") {
                let legacy_id = format!("dev.cursorbyok.{suffix}");
                if let Ok(legacy_val) = self
                    .data
                    .read(&legacy_id, &resource_key(resource_type))
                    .await
                {
                    if !legacy_val.is_null() {
                        let _ = self
                            .data
                            .update(plugin_id, &resource_key(resource_type), &legacy_val)
                            .await;
                        value = legacy_val;
                    }
                }
            }
        }
        if value.is_null() {
            return Ok(Vec::new());
        }
        Ok(serde_json::from_value(value)?)
    }

    pub async fn upsert_resources(
        &self,
        plugin_id: &str,
        resource_type: &str,
        drafts: Vec<ResourceDraft>,
    ) -> Result<UpsertOutcome> {
        let _operation = self.operations.lock().await;
        let mut records = self.resources_unlocked(plugin_id, resource_type).await?;
        let now = now_ms();
        let mut outcome = UpsertOutcome {
            added: 0,
            updated: 0,
        };
        for draft in drafts {
            if draft.key.trim().is_empty() {
                return Err(Error::Protocol("plugin resource draft requires key".into()));
            }
            match records.iter_mut().find(|record| record.key == draft.key) {
                Some(existing) => {
                    existing.private_data = draft.private_data;
                    if let Some(state_input) = draft.state {
                        existing.state = ResourceState::from(state_input);
                    }
                    existing.updated_at_ms = now;
                    outcome.updated += 1;
                }
                None => {
                    let state = draft
                        .state
                        .map_or(ResourceState::Ready, ResourceState::from);
                    records.push(ResourceRecord {
                        id: uuid::Uuid::new_v4().to_string(),
                        key: draft.key,
                        private_data: draft.private_data,
                        state,
                        created_at_ms: now,
                        updated_at_ms: now,
                    });
                    outcome.added += 1;
                }
            }
        }
        self.save_resources(plugin_id, resource_type, &records)
            .await?;
        Ok(outcome)
    }

    pub async fn restore_resources(
        &self,
        plugin_id: &str,
        resource_type: &str,
        incoming: Vec<ResourceRecord>,
    ) -> Result<std::collections::HashMap<String, String>> {
        let _operation = self.operations.lock().await;
        let mut records = self.resources_unlocked(plugin_id, resource_type).await?;
        let mut mapping = std::collections::HashMap::new();
        let mut keys = std::collections::HashSet::new();
        for mut record in incoming {
            if record.id.is_empty()
                || record.key.trim().is_empty()
                || mapping.contains_key(&record.id)
                || !keys.insert(record.key.clone())
            {
                return Err(Error::Config(
                    "backup contains invalid or duplicate resources".into(),
                ));
            }
            let old_id = record.id.clone();
            if let Some(existing) = records.iter_mut().find(|r| r.key == record.key) {
                record.id = existing.id.clone();
                mapping.insert(old_id, record.id.clone());
                *existing = record;
            } else {
                if records.iter().any(|r| r.id == record.id) {
                    return Err(Error::Config(
                        "backup resource ID belongs to another account".into(),
                    ));
                }
                mapping.insert(old_id, record.id.clone());
                records.push(record);
            }
        }
        self.save_resources(plugin_id, resource_type, &records)
            .await?;
        Ok(mapping)
    }

    pub async fn apply_patch(
        &self,
        plugin_id: &str,
        resource_type: &str,
        resource_id: &str,
        patch: ResourcePatch,
    ) -> Result<()> {
        self.mutate_resource(plugin_id, resource_type, resource_id, |record| {
            if let Some(private_data) = patch.private_data {
                record.private_data = private_data;
            }
            if let Some(state) = patch.state {
                record.state = state.into();
            }
            Ok(())
        })
        .await
    }

    pub async fn mutate_resource(
        &self,
        plugin_id: &str,
        resource_type: &str,
        resource_id: &str,
        update: impl FnOnce(&mut ResourceRecord) -> Result<()> + Send,
    ) -> Result<()> {
        let _operation = self.operations.lock().await;
        let mut records = self.resources_unlocked(plugin_id, resource_type).await?;
        let record = records
            .iter_mut()
            .find(|record| record.id == resource_id)
            .ok_or_else(|| Error::RunNotFound(format!("plugin resource {resource_id}")))?;
        update(record)?;
        record.updated_at_ms = now_ms();
        self.save_resources(plugin_id, resource_type, &records)
            .await
    }

    pub async fn remove_resource(
        &self,
        plugin_id: &str,
        resource_type: &str,
        resource_id: &str,
    ) -> Result<ResourceRecord> {
        let _operation = self.operations.lock().await;
        let mut records = self.resources_unlocked(plugin_id, resource_type).await?;
        let index = records
            .iter()
            .position(|record| record.id == resource_id)
            .ok_or_else(|| Error::RunNotFound(format!("plugin resource {resource_id}")))?;
        let removed = records.remove(index);
        self.save_resources(plugin_id, resource_type, &records)
            .await?;
        Ok(removed)
    }

    pub async fn models(&self, plugin_id: &str, provider_id: &str) -> Result<Vec<StoredModel>> {
        let _operation = self.operations.lock().await;
        self.models_unlocked(plugin_id, provider_id).await
    }

    async fn models_unlocked(
        &self,
        plugin_id: &str,
        provider_id: &str,
    ) -> Result<Vec<StoredModel>> {
        let mut value = self.data.read(plugin_id, &model_key(provider_id)).await?;
        if value.is_null() {
            if let Some(suffix) = plugin_id.strip_prefix("dev.nexusor.") {
                let legacy_id = format!("dev.cursorbyok.{suffix}");
                if let Ok(legacy_val) = self.data.read(&legacy_id, &model_key(provider_id)).await {
                    if !legacy_val.is_null() {
                        let _ = self
                            .data
                            .update(plugin_id, &model_key(provider_id), &legacy_val)
                            .await;
                        value = legacy_val;
                    }
                }
            }
        }
        if value.is_null() {
            return Ok(Vec::new());
        }
        Ok(serde_json::from_value(value)?)
    }

    pub async fn replace_models(
        &self,
        plugin_id: &str,
        provider_id: &str,
        models: &[StoredModel],
    ) -> Result<()> {
        let _operation = self.operations.lock().await;
        let previous = self.models_unlocked(plugin_id, provider_id).await?;
        let models = models
            .iter()
            .cloned()
            .map(|mut model| {
                if let Some(old) = previous.iter().find(|old| old.id == model.id) {
                    model.enabled = old.enabled;
                }
                model
            })
            .collect::<Vec<_>>();
        self.data
            .update(
                plugin_id,
                &model_key(provider_id),
                &serde_json::to_value(models)?,
            )
            .await
    }

    pub async fn set_model_enabled(
        &self,
        plugin_id: &str,
        provider_id: &str,
        model_id: &str,
        enabled: bool,
    ) -> Result<()> {
        let _operation = self.operations.lock().await;
        let mut models = self.models_unlocked(plugin_id, provider_id).await?;
        let model = models
            .iter_mut()
            .find(|model| model.id == model_id)
            .ok_or_else(|| Error::RunNotFound(format!("plugin model {model_id}")))?;
        model.enabled = enabled;
        self.data
            .update(
                plugin_id,
                &model_key(provider_id),
                &serde_json::to_value(models)?,
            )
            .await
    }

    pub async fn clear(&self, plugin_id: &str) -> Result<()> {
        let _operation = self.operations.lock().await;
        self.data.clear(plugin_id).await
    }

    async fn save_resources(
        &self,
        plugin_id: &str,
        resource_type: &str,
        records: &[ResourceRecord],
    ) -> Result<()> {
        self.data
            .update(
                plugin_id,
                &resource_key(resource_type),
                &serde_json::to_value(records)?,
            )
            .await
    }
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or_default()
}

fn resource_key(resource_type: &str) -> String {
    format!("resources-{resource_type}")
}

fn model_key(provider_id: &str) -> String {
    format!("models-{provider_id}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, PluginStateStore) {
        let root = tempfile::tempdir().unwrap();
        let data = PluginDataStore::for_test(root.path().join("data")).unwrap();
        (root, PluginStateStore::new(data))
    }

    #[tokio::test]
    async fn upserts_resources_by_key_and_applies_patches() {
        let (_root, store) = store();
        let outcome = store
            .upsert_resources(
                "dev.example",
                "account",
                vec![ResourceDraft {
                    key: "acct-1".into(),
                    private_data: serde_json::json!({"token":"one"}),
                    state: None,
                }],
            )
            .await
            .unwrap();
        assert_eq!(outcome.added, 1);
        let outcome = store
            .upsert_resources(
                "dev.example",
                "account",
                vec![ResourceDraft {
                    key: "acct-1".into(),
                    private_data: serde_json::json!({"token":"two"}),
                    state: None,
                }],
            )
            .await
            .unwrap();
        assert_eq!(outcome.updated, 1);
        let records = store.resources("dev.example", "account").await.unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].private_data["token"], "two");

        store
            .apply_patch(
                "dev.example",
                "account",
                &records[0].id,
                ResourcePatch {
                    private_data: None,
                    state: Some(ResourceStateInput::Cooling {
                        retry_at_ms: Some(200),
                        message: None,
                    }),
                },
            )
            .await
            .unwrap();
        let records = store.resources("dev.example", "account").await.unwrap();
        assert!(!records[0].state.is_ready(100));
        assert!(records[0].state.is_ready(300), "cooling expires over time");
    }

    #[tokio::test]
    async fn replaces_model_catalogs() {
        let (_root, store) = store();
        let model = StoredModel::from_definition(&serde_json::json!({
            "id": "gpt-test",
            "displayName": "GPT Test",
            "capabilities": {"images": true},
            "privateData": {"reasoningEfforts": ["low"]},
        }))
        .unwrap();
        store
            .replace_models("dev.example", "codex", &[model])
            .await
            .unwrap();
        let models = store.models("dev.example", "codex").await.unwrap();
        assert_eq!(models.len(), 1);
        assert!(models[0].images);
        assert_eq!(models[0].private_data["reasoningEfforts"][0], "low");
    }

    #[tokio::test]
    async fn concurrent_patches_preserve_both_updates_and_deleted_records_stay_deleted() {
        let (_root, store) = store();
        store
            .upsert_resources(
                "dev.example",
                "account",
                ["a", "b"]
                    .into_iter()
                    .map(|key| ResourceDraft {
                        key: key.into(),
                        private_data: serde_json::json!({}),
                        state: None,
                    })
                    .collect(),
            )
            .await
            .unwrap();
        let records = store.resources("dev.example", "account").await.unwrap();
        let disable = |id| {
            store.apply_patch(
                "dev.example",
                "account",
                id,
                ResourcePatch {
                    private_data: None,
                    state: Some(ResourceStateInput::Disabled),
                },
            )
        };
        let (a, b) = tokio::join!(disable(&records[0].id), disable(&records[1].id));
        a.unwrap();
        b.unwrap();
        assert!(store
            .resources("dev.example", "account")
            .await
            .unwrap()
            .iter()
            .all(|r| r.state == ResourceState::Disabled));
        store
            .remove_resource("dev.example", "account", &records[0].id)
            .await
            .unwrap();
        assert!(disable(&records[0].id).await.is_err());
        assert_eq!(
            store
                .resources("dev.example", "account")
                .await
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn restore_preserves_state_and_maps_existing_account_identity() {
        let (_root, source) = store();
        source
            .upsert_resources(
                "dev.example",
                "account",
                vec![ResourceDraft {
                    key: "same-account".into(),
                    private_data: serde_json::json!({"token":"backup"}),
                    state: Some(ResourceStateInput::Disabled),
                }],
            )
            .await
            .unwrap();
        let backup = source.resources("dev.example", "account").await.unwrap();
        let (_other_root, destination) = store();
        destination
            .upsert_resources(
                "dev.example",
                "account",
                vec![ResourceDraft {
                    key: "same-account".into(),
                    private_data: serde_json::json!({}),
                    state: None,
                }],
            )
            .await
            .unwrap();
        let existing = destination
            .resources("dev.example", "account")
            .await
            .unwrap()
            .remove(0);
        let mapping = destination
            .restore_resources("dev.example", "account", backup.clone())
            .await
            .unwrap();
        assert_eq!(mapping[&backup[0].id], existing.id);
        let restored = destination
            .resources("dev.example", "account")
            .await
            .unwrap();
        assert_eq!(restored.len(), 1);
        assert_eq!(restored[0].state, ResourceState::Disabled);
        assert_eq!(restored[0].private_data["token"], "backup");
        let (_empty_root, empty) = store();
        empty
            .restore_resources("dev.example", "account", backup.clone())
            .await
            .unwrap();
        let restored = empty.resources("dev.example", "account").await.unwrap();
        assert_eq!(restored.len(), 1);
        assert_eq!(restored[0].id, backup[0].id);
        assert_eq!(restored[0].state, backup[0].state);
        assert_eq!(restored[0].private_data, backup[0].private_data);
    }
}
