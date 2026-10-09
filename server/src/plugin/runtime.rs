use crate::{store::Store, Result};
use serde::Serialize;
use std::path::PathBuf;

#[derive(Clone)]
pub struct PluginRuntime;

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PluginRuntimeState {
    Ready,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct PluginRuntimeStatus {
    pub state: PluginRuntimeState,
    pub version: String,
    pub target: Option<String>,
    pub phase: Option<String>,
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
    pub error: Option<String>,
}

impl PluginRuntime {
    pub fn managed() -> Result<Self> {
        Ok(Self)
    }

    pub fn status(&self) -> PluginRuntimeStatus {
        PluginRuntimeStatus {
            state: PluginRuntimeState::Ready,
            version: "native-rust".into(),
            target: Some("windows".into()),
            phase: None,
            downloaded_bytes: 0,
            total_bytes: None,
            error: None,
        }
    }

    pub fn executable(&self) -> Option<PathBuf> {
        Some(PathBuf::from("native"))
    }

    pub fn initialize(&self, _store: Store) -> PluginRuntimeStatus {
        self.status()
    }

    pub fn cancel_initialization(&self) -> PluginRuntimeStatus {
        self.status()
    }
}
