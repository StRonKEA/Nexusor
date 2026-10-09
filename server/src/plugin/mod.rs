//! Owns filesystem plugin discovery and native plugin providers.
mod builtin;
mod catalog;
mod data;
pub mod descriptor;
mod manifest;
mod oauth_callback;
pub mod protocol;
pub mod registry;
mod runtime;
mod state;
pub mod wire;

pub use descriptor::{
    parse_model_id, PluginDescriptor, PluginModelDescriptor, PluginProviderDescriptor,
    PluginResourceDescriptor, PluginResourceView, ADAPTER_ID_PREFIX,
};
pub use registry::{ImportResponse, OAuthBeginResponse, OAuthPollResponse, PluginRegistry};
pub use runtime::{PluginRuntime, PluginRuntimeState, PluginRuntimeStatus};
pub use state::{
    PoolStrategy, PoolStrategyConfig, ResourcePatch, ResourceRecord, ResourceState,
    ResourceStateInput, StoredModel,
};
