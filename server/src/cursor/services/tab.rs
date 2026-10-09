//! Implements Cursor tab metadata services.
use axum::{
    body::Body,
    extract::{Extension, State},
    http::{Request, Response},
    routing::post,
    Router,
};

use crate::{api::cursor::proxy, cursor::transport::TransportRegistry, Result};
use prost::Message;

pub const TAB_PATHS: [&str; 15] = [
    "/aiserver.v1.AiService/StreamCpp",
    "/aiserver.v1.AiService/StreamNextCursorPrediction",
    "/aiserver.v1.AiService/GetCppEditClassification",
    "/aiserver.v1.AiService/RefreshTabContext",
    "/aiserver.v1.AiService/CppConfig",
    "/aiserver.v1.AiService/CppEditHistoryStatus",
    "/aiserver.v1.AiService/CppAppend",
    "/aiserver.v1.AiService/CppEditHistoryAppend",
    "/aiserver.v1.AiService/ReportAiCodeChangeMetrics",
    "/aiserver.v1.CppService/AvailableModels",
    "/aiserver.v1.CppService/RecordCppFate",
    "/aiserver.v1.FileSyncService/FSSyncFile",
    "/aiserver.v1.FileSyncService/FSIsEnabledForUser",
    "/aiserver.v1.FileSyncService/FSConfig",
    "/aiserver.v1.FileSyncService/FSUploadFile",
];

pub fn is_tab_path(path: &str) -> bool {
    TAB_PATHS.contains(&path)
}

pub fn router() -> Router<TransportRegistry> {
    TAB_PATHS.into_iter().fold(Router::new(), |router, path| {
        if path == "/aiserver.v1.AiService/StreamCpp" {
            router.route(path, post(super::tab_completion::complete))
        } else if path == "/aiserver.v1.AiService/CppConfig" {
            router.route(path, post(config))
        } else {
            router.route(path, post(forward))
        }
    })
}

async fn config(
    State(registry): State<TransportRegistry>,
    Extension(upstream): Extension<proxy::CursorProxy>,
    request: Request<Body>,
) -> Result<Response<Body>> {
    if registry
        .store()
        .cmdk_settings()
        .await?
        .tab_model_id
        .is_empty()
        || registry
            .store()
            .tab_settings()
            .await?
            .service_url()
            .is_some()
    {
        return forward(State(registry), Extension(upstream), request).await;
    }
    let response = proxy::forward_buffered(&upstream, request).await?;
    if !response.status.is_success()
        || crate::cursor::protocol::proto::tab::ConfigResponse::decode(response.body.clone())
            .is_err()
    {
        return Ok(response.into_response());
    }
    let mut body = response.body.to_vec();
    // Preserve the upstream configuration and unknown fields. Recently viewed
    // file content is required for validated cross-file Tab navigation.
    prost::encoding::bool::encode(10, &true, &mut body);
    prost::encoding::bool::encode(22, &true, &mut body);
    Ok(response.with_body(body.into()))
}

pub(super) async fn forward(
    State(registry): State<TransportRegistry>,
    Extension(upstream): Extension<proxy::CursorProxy>,
    request: Request<Body>,
) -> Result<Response<Body>> {
    let settings = registry.store().tab_settings().await?;
    match settings.service_url() {
        Some(service_url) => proxy::forward_to_service(&upstream, request, service_url).await,
        None => proxy::forward(Extension(upstream), request).await,
    }
}
