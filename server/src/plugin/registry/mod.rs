//! Orchestrates native plugin capabilities, resources, and model catalogs.
//!
//! One responsibility per submodule; each is an impl PluginRegistry block that
//! shares the state declared here.

mod accounts;
mod actions;
mod cooldown;
mod descriptor;
mod management;
mod models;
mod oauth;
mod quota;
mod resources;
mod selection;
mod stream;
mod sync_models;

#[cfg(test)]
mod tests;

use base64::Engine;
pub(crate) use descriptor::find_provider;
use std::{collections::HashMap, path::Path, sync::Arc};

use serde::Serialize;
use tokio::sync::{Mutex, RwLock};
use tokio_util::sync::CancellationToken;

use super::{
    catalog::{PluginCatalog, PluginEntry},
    data::PluginDataStore,
    descriptor::{
        parse_model_id, PluginDescriptor, PluginModelDescriptor, PluginProviderDescriptor,
        PluginResourceDescriptor, PluginResourceView, ProviderDefinition, ResourceMetric,
    },
    oauth_callback::{self, CallbackHandle},
    runtime::PluginRuntime,
    state::{
        now_ms, PluginStateStore, PoolStrategy, ResourceDraft, ResourcePatch, ResourceRecord,
        ResourceState, ResourceStateInput, StoredModel,
    },
};
use crate::{
    model::ModelInvocation,
    provider::{CallRecorder, ProviderStream},
    store::Store,
    Error, Result,
};
#[derive(Clone, Debug)]
pub struct PluginInvocationPlan {
    pub model: PluginModelDescriptor,
    pub request_url: String,
}

#[derive(Clone)]
pub struct PluginRegistry {
    inner: Arc<RegistryInner>,
}

struct RegistryInner {
    catalog: PluginCatalog,
    state: PluginStateStore,
    entries: RwLock<Option<Vec<PluginEntry>>>,
    oauth_sessions: Mutex<HashMap<String, OAuthSession>>,
    resource_cursors: Mutex<HashMap<String, usize>>,
    circuit_breakers: Mutex<HashMap<String, CircuitBreakerEntry>>,
    conversation_affinity: Mutex<HashMap<String, (String, i64)>>,
    clients: crate::network::NetworkClients,
    #[cfg(test)]
    antigravity_test_urls: RwLock<Option<Vec<String>>>,
}

/// Quarantines expire after 30s; entries are dropped once they are this old so the
/// map tracks live state instead of every id ever seen.
const CIRCUIT_BREAKER_RETENTION_MS: i64 = 10 * 60 * 1000;

#[derive(Clone, Debug, Default)]
pub struct CircuitBreakerEntry {
    pub consecutive_failures: u32,
    pub quarantined_until_ms: i64,
}

struct OAuthSession {
    plugin_id: String,
    resource_type: String,
    session: serde_json::Value,
    poll_interval_ms: i64,
    flow: OAuthFlow,
}

enum OAuthFlow {
    DeviceCode,
    AuthorizationCode {
        redirect_uri: String,
        code_verifier: String,
        callback: CallbackHandle,
    },
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthBeginResponse {
    pub session_id: String,
    pub user_code: Option<String>,
    pub verification_url: Option<String>,
    pub verification_url_complete: Option<String>,
    pub expires_at_ms: i64,
    pub poll_interval_ms: i64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthPollResponse {
    pub status: String,
    pub message: Option<String>,
    pub model_sync_error: Option<String>,
    pub poll_interval_ms: i64,
}

fn oauth_poll_status(
    status: &str,
    message: Option<String>,
    poll_interval_ms: i64,
) -> OAuthPollResponse {
    OAuthPollResponse {
        status: status.into(),
        message,
        model_sync_error: None,
        poll_interval_ms,
    }
}

fn provider_id_for_resource(plugin_id: &str, resource_type: &str) -> Option<&'static str> {
    match (plugin_id, resource_type) {
        (
            "dev.nexusor.examples.codex-auth" | "dev.cursorbyok.examples.codex-auth",
            "chatgpt-account",
        ) => Some("codex"),
        (
            "dev.nexusor.examples.grok-auth" | "dev.cursorbyok.examples.grok-auth",
            "grok-account",
        ) => Some("grok"),
        (
            "dev.nexusor.plugins.antigravity-auth" | "dev.cursorbyok.plugins.antigravity-auth",
            "google-account",
        ) => Some("antigravity"),
        ("dev.nexusor.plugins.github-copilot", "github-copilot-account") => Some("copilot"),
        ("dev.nexusor.plugins.kimi-auth", "kimi-account") => Some("kimi"),
        ("dev.nexusor.plugins.claude-code", "claude-code-account") => Some("claude-code"),
        _ => None,
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportResponse {
    pub added: usize,
    pub updated: usize,
    pub warnings: Vec<String>,
    pub model_sync_error: Option<String>,
}

pub fn extract_remaining_quota_percent(record: &ResourceRecord) -> f64 {
    let pdata = &record.private_data;

    // 1. Antigravity: check gemini or claude quota
    if let Some(gemini) = pdata.pointer("/quota/gemini") {
        if let Some(pct) = gemini
            .get("fraction_remaining")
            .and_then(serde_json::Value::as_f64)
        {
            return (pct * 100.0).clamp(0.0, 100.0);
        }
    }
    if let Some(claude) = pdata.pointer("/quota/claude") {
        if let Some(pct) = claude
            .get("fraction_remaining")
            .and_then(serde_json::Value::as_f64)
        {
            return (pct * 100.0).clamp(0.0, 100.0);
        }
    }

    // 2. Grok: remaining_percent
    if let Some(rem) = pdata
        .pointer("/quota/remaining_percent")
        .or_else(|| pdata.pointer("/quota/remainingPercent"))
        .and_then(serde_json::Value::as_f64)
    {
        return rem.clamp(0.0, 100.0);
    }

    // 3. Codex / Kimi / Claude Code: used_percent (remaining = 100 - used)
    let window = pdata
        .pointer("/quota/primary_window")
        .or_else(|| pdata.pointer("/quota/primaryWindow"))
        .or_else(|| pdata.pointer("/quota/secondary_window"))
        .or_else(|| pdata.pointer("/quota/secondaryWindow"))
        .or_else(|| pdata.get("quota"));

    if let Some(used) = window
        .and_then(|w| w.get("used_percent").or_else(|| w.get("usedPercent")))
        .and_then(serde_json::Value::as_f64)
    {
        return (100.0 - used).clamp(0.0, 100.0);
    }

    100.0
}

impl PluginRegistry {
    #[cfg(test)]
    pub(crate) fn for_test(store: Store, root: &Path) -> Self {
        Self {
            inner: Arc::new(RegistryInner {
                catalog: PluginCatalog::for_test(),
                state: PluginStateStore::new(PluginDataStore::for_test(root.join("data")).unwrap()),
                entries: RwLock::new(None),
                oauth_sessions: Mutex::new(HashMap::new()),
                resource_cursors: Mutex::new(HashMap::new()),
                circuit_breakers: Mutex::new(HashMap::new()),
                conversation_affinity: Mutex::new(HashMap::new()),
                clients: crate::network::NetworkClients::new(store),
                antigravity_test_urls: RwLock::new(None),
            }),
        }
    }

    pub fn managed(store: Store, _runtime: PluginRuntime, app_version: String) -> Result<Self> {
        let data = PluginDataStore::managed()?;
        let clients = crate::network::NetworkClients::new(store);

        Ok(Self {
            inner: Arc::new(RegistryInner {
                catalog: PluginCatalog::managed(app_version)?,
                state: PluginStateStore::new(data),
                entries: RwLock::new(None),
                oauth_sessions: Mutex::new(HashMap::new()),
                resource_cursors: Mutex::new(HashMap::new()),
                circuit_breakers: Mutex::new(HashMap::new()),
                conversation_affinity: Mutex::new(HashMap::new()),
                clients,
                #[cfg(test)]
                antigravity_test_urls: RwLock::new(None),
            }),
        })
    }

    pub async fn client(&self) -> Result<reqwest::Client> {
        self.inner.clients.default_client().await
    }

    #[cfg(test)]
    pub(crate) async fn antigravity_test_urls(&self) -> Option<Vec<String>> {
        self.inner.antigravity_test_urls.read().await.clone()
    }

    #[cfg(test)]
    pub(crate) async fn set_antigravity_test_urls(&self, urls: Vec<String>) {
        *self.inner.antigravity_test_urls.write().await = Some(urls);
    }

    #[cfg(test)]
    pub(crate) async fn enable_test_image_model(&self) {
        let plugin = "dev.nexusor.plugins.antigravity-auth";
        let manifest = serde_json::from_value(serde_json::json!({
            "apiVersion":1,"id":plugin,"name":"Fixture","version":"1.0.0","icon":"icon.svg","entry":"main.js"
        })).unwrap();
        let definition = serde_json::from_value(serde_json::json!({"providers":[{
            "id":"antigravity","displayName":"Fixture","providerType":"openai-chat",
            "resourceType":"google-account","hasModels":true
        }]}))
        .unwrap();
        *self.inner.entries.write().await = Some(vec![PluginEntry {
            manifest,
            definition,
            icon: String::new(),
        }]);
        let model = StoredModel::from_definition(
            &serde_json::json!({"id":"gemini-3.1-flash-image","displayName":"Fixture"}),
        )
        .unwrap();
        self.inner
            .state
            .replace_models(plugin, "antigravity", &[model])
            .await
            .unwrap();
    }

    pub async fn invalidate_clients(&self) {
        self.inner.clients.invalidate().await;
    }
}
