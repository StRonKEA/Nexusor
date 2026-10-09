//! The control API surface: the shared state and one method per operation.

mod backup;
mod discovery;
mod model_test;
mod models;
mod plugins;
mod settings;
mod tracing;
mod types;

use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use tokio_util::sync::CancellationToken;

use crate::{
    local_app::CursorHarness,
    plugin::{PluginRegistry, PluginRuntime},
    provider::Provider,
    store::Store,
    Result,
};

pub use types::{
    CallDetail, CallSummary, DiscoveredModels, LegacyModelImportPreview, LegacyModelImportResult,
    ModelConnectivityResult, ModelDiscoveryInput, ObservabilitySettings,
};

/// The shared state every control operation runs against.
#[derive(Clone)]
pub struct ControlService {
    store: Store,
    cursor_harness: CursorHarness,
    provider: Arc<dyn Provider>,
    plugin_runtime: PluginRuntime,
    plugins: PluginRegistry,
    clients: crate::network::NetworkClients,
    model_tests: Arc<Mutex<BTreeMap<String, CancellationToken>>>,
}

impl ControlService {
    pub fn new(
        store: Store,
        provider: Arc<dyn Provider>,
        plugin_runtime: PluginRuntime,
        plugins: PluginRegistry,
        clients: crate::network::NetworkClients,
    ) -> Result<Self> {
        Ok(Self {
            cursor_harness: CursorHarness::with_plugins(store.clone(), Some(plugins.clone()))?,
            store,
            provider,
            plugin_runtime,
            plugins,
            clients,
            model_tests: Arc::new(Mutex::new(BTreeMap::new())),
        })
    }

    pub fn cursor_harness(&self) -> &CursorHarness {
        &self.cursor_harness
    }

    pub fn store(&self) -> &Store {
        &self.store
    }
}
