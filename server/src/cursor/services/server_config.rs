//! Preserves official configuration while supporting local-account Agent transport.
use axum::{
    body::Body,
    extract::{Extension, State},
    http::{header, HeaderValue, Request, Response, StatusCode},
};
use prost::Message;

use crate::{api::cursor::proxy, local_app, Result};

const HTTP2_CONFIG_FORCE_ALL_DISABLED: i32 = 1;

#[derive(Clone, PartialEq, Message)]
struct ServerConfigResponse {
    #[prost(string, tag = "6")]
    config_version: String,
    #[prost(int32, tag = "7")]
    http2_config: i32,
    #[prost(message, optional, tag = "10")]
    background_composer_config: Option<BackgroundComposerConfigProto>,
    #[prost(message, optional, tag = "11")]
    auto_context_config: Option<AutoContextConfigProto>,
    #[prost(bool, optional, tag = "28")]
    cli_sandbox_default_enabled: Option<bool>,
}

#[derive(Clone, PartialEq, Message)]
struct BackgroundComposerConfigProto {
    #[prost(bool, tag = "1")]
    enable_background_agent: bool,
    #[prost(bool, tag = "2")]
    show_background_agent_in_beta_settings: bool,
    #[prost(bool, tag = "7")]
    show_background_agent_history_action: bool,
    #[prost(bool, tag = "9")]
    use_modal_experience: bool,
}

#[derive(Clone, PartialEq, Message)]
struct AutoContextConfigProto {
    #[prost(bool, tag = "1")]
    enabled: bool,
    #[prost(bool, tag = "2")]
    enabled_fallback: bool,
    #[prost(bool, tag = "3")]
    enabled_git_graph: bool,
    #[prost(bool, tag = "4")]
    enabled_sem_search: bool,
    #[prost(bool, tag = "5")]
    enabled_v2: bool,
}

pub async fn get(
    State(registry): State<crate::cursor::transport::TransportRegistry>,
    Extension(upstream): Extension<proxy::CursorProxy>,
    request: Request<Body>,
) -> Result<Response<Body>> {
    if !local_app::request_uses_local_cursor_token(request.headers()) {
        if !registry.store().cursor_takeover_enabled().await? {
            return proxy::forward(Extension(upstream), request).await;
        }
        let response = proxy::forward_buffered(&upstream, request).await?;
        if response.status != StatusCode::OK
            || ServerConfigResponse::decode(response.body.clone()).is_err()
        {
            return Ok(response.into_response());
        }
        let mut body = response.body.to_vec();
        // Preserve unknown fields and override only the transport enum (field 7).
        prost::encoding::int32::encode(7, &HTTP2_CONFIG_FORCE_ALL_DISABLED, &mut body);
        return Ok(response.with_body(body.into()));
    }
    let payload = server_config().encode_to_vec();
    let mut response = Response::new(Body::from(payload));
    *response.status_mut() = StatusCode::OK;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/proto"),
    );
    Ok(response)
}

fn server_config() -> ServerConfigResponse {
    ServerConfigResponse {
        config_version: "nexusor_local_agent_v1".into(),
        http2_config: HTTP2_CONFIG_FORCE_ALL_DISABLED,
        background_composer_config: Some(BackgroundComposerConfigProto {
            enable_background_agent: true,
            show_background_agent_in_beta_settings: true,
            show_background_agent_history_action: true,
            use_modal_experience: true,
        }),
        auto_context_config: Some(AutoContextConfigProto {
            enabled: true,
            enabled_fallback: true,
            enabled_git_graph: true,
            enabled_sem_search: true,
            enabled_v2: true,
        }),
        cli_sandbox_default_enabled: Some(true),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forces_agent_cli_to_use_the_selected_legacy_endpoint() {
        let config = server_config();
        assert_eq!(config.http2_config, HTTP2_CONFIG_FORCE_ALL_DISABLED);
        assert_eq!(config.cli_sandbox_default_enabled, Some(true));
    }
}
