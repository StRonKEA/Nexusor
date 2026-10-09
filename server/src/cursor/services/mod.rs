//! Exposes Cursor services outside the Agent loop.

pub mod account;
pub mod analytics;
pub mod blob_sync;
pub mod cmdk;
pub mod commit_message;
pub mod compatibility;
mod context_ranking;
pub mod context_sync;
mod generated_text;
pub mod image;
pub mod knowledge;
pub mod model_catalog;
pub mod observability;
pub mod server_config;
pub mod tab;
mod tab_completion;
mod tab_edits;
pub mod terminal;
pub mod usage;
