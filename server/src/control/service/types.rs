//! The request and response shapes the control API exchanges.

use serde::{Deserialize, Serialize};

use crate::model::{
    CursorRunTraceSummary, LlmCallRequest, LlmCallResponseChunk, LlmCallSummary, ModelType,
};

#[derive(Clone, Debug, Serialize)]
pub struct DiscoveredModels {
    pub models: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct LegacyModelImportResult {
    pub imported: usize,
    pub skipped: usize,
    pub total: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct LegacyModelImportPreview {
    pub source: String,
    pub total: usize,
    pub new_models: usize,
    pub existing_models: usize,
    pub models: Vec<LegacyModelImportPreviewItem>,
}

#[derive(Clone, Debug, Serialize)]
pub struct LegacyModelImportPreviewItem {
    pub model_hash: String,
    pub display_name: String,
    pub model_id: String,
    #[serde(rename = "type")]
    pub model_type: ModelType,
    pub existing: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ModelDiscoveryInput {
    #[serde(rename = "type")]
    pub model_type: ModelType,
    pub base_url: String,
    pub api_key: String,
    #[serde(default)]
    pub custom_headers_enabled: bool,
    #[serde(default = "empty_json_object")]
    pub custom_headers: serde_json::Value,
}

pub(crate) fn empty_json_object() -> serde_json::Value {
    serde_json::json!({})
}

/// One shared empty object, so a provider call that sends no custom headers does not
/// allocate a fresh value per request.
pub(crate) fn empty_json_object_ref() -> &'static serde_json::Value {
    static EMPTY: std::sync::OnceLock<serde_json::Value> = std::sync::OnceLock::new();
    EMPTY.get_or_init(empty_json_object)
}

#[derive(Clone, Debug, Serialize)]
pub struct ModelConnectivityResult {
    pub duration_ms: u64,
    pub first_valid_response_ms: Option<u64>,
    pub output_tokens: u64,
    pub tokens_per_second: f64,
    pub tokens_estimated: bool,
    pub output: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct CallDetail {
    pub call: CallSummary,
    pub request: Option<LlmCallRequest>,
    pub response_chunks: Vec<LlmCallResponseChunk>,
    pub cursor_trace: Option<CursorTraceDetail>,
}

#[derive(Clone, Debug, Serialize)]
pub struct CallSummary {
    #[serde(flatten)]
    pub call: LlmCallSummary,
    pub call_kind: &'static str,
    pub route: &'static str,
}

#[derive(Clone, Debug, Serialize)]
pub struct CursorTraceDetail {
    pub trace: CursorRunTraceSummary,
    pub artifacts: Vec<CursorTraceArtifactDetail>,
}

#[derive(Clone, Debug, Serialize)]
pub struct CursorTraceArtifactDetail {
    pub seq: i64,
    pub artifact_type: String,
    pub source: String,
    pub metadata: serde_json::Value,
    pub created_at_ms: i64,
    pub byte_count: usize,
    pub encoding: &'static str,
    pub data: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub struct ObservabilitySettings {
    pub detailed: bool,
}
