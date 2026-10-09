//! Exposes the local control API.
mod calls;
mod combos;
mod harness;
mod models;
mod overview;
mod plugins;
mod service;
mod settings;

use axum::{
    body::{to_bytes, Body},
    extract::{DefaultBodyLimit, State},
    http::{header, header::CONTENT_TYPE, HeaderValue, Method, Request, Response, StatusCode},
    routing::{any, get, post, put},
    Router,
};
use tower_http::{
    cors::{AllowOrigin, CorsLayer},
    services::ServeDir,
};
use url::{Host, Url};

pub use service::{
    CallDetail, CallSummary, ControlService, DiscoveredModels, LegacyModelImportPreview,
    LegacyModelImportResult, ModelConnectivityResult, ModelDiscoveryInput, ObservabilitySettings,
};

pub fn web_router(service: ControlService, assets: impl AsRef<std::path::Path>) -> Router {
    Router::new()
        .nest_service(
            "/__byok-api__",
            ServeDir::new(assets).append_index_html_on_directories(true),
        )
        .merge(api_router(service))
}

pub fn proxy_web_router(service: ControlService, target: Url) -> Router {
    frontend_proxy_router(target).merge(api_router(service))
}

fn frontend_proxy_router(target: Url) -> Router {
    let state = FrontendProxy {
        client: reqwest::Client::new(),
        target: target.as_str().trim_end_matches('/').to_string(),
    };
    Router::new()
        .route("/__byok-api__/", any(proxy_frontend))
        .route("/__byok-api__/{*path}", any(proxy_frontend))
        .with_state(state)
}

#[derive(Clone)]
struct FrontendProxy {
    client: reqwest::Client,
    target: String,
}

async fn proxy_frontend(
    State(proxy): State<FrontendProxy>,
    request: Request<Body>,
) -> Response<Body> {
    let (parts, body) = request.into_parts();
    let path = parts
        .uri
        .path_and_query()
        .map(|value| value.as_str())
        .unwrap_or("/__byok-api__/");
    let mut upstream = proxy
        .client
        .request(parts.method, format!("{}{path}", proxy.target));
    for (name, value) in &parts.headers {
        if name != header::HOST && name != header::CONNECTION {
            upstream = upstream.header(name, value);
        }
    }
    let body = match to_bytes(body, 64 * 1024 * 1024).await {
        Ok(body) => body,
        Err(error) => return proxy_error(error),
    };
    let upstream = match upstream.body(body).send().await {
        Ok(response) => response,
        Err(error) => return proxy_error(error),
    };
    let status = upstream.status();
    let headers = upstream.headers().clone();
    let body = match upstream.bytes().await {
        Ok(body) => body,
        Err(error) => return proxy_error(error),
    };
    let mut response = Response::new(Body::from(body));
    *response.status_mut() = status;
    for (name, value) in &headers {
        if name != header::CONNECTION
            && name != header::TRANSFER_ENCODING
            && name != header::CONTENT_LENGTH
        {
            response.headers_mut().insert(name, value.clone());
        }
    }
    response
}

fn proxy_error(error: impl std::fmt::Display) -> Response<Body> {
    tracing::warn!(%error, "frontend development proxy failed");
    Response::builder()
        .status(StatusCode::BAD_GATEWAY)
        .body(Body::from("frontend development server is unavailable"))
        .expect("static proxy error response")
}

pub fn api_router(service: ControlService) -> Router {
    Router::new()
        .route(
            "/__byok-api__/api/models",
            get(models::list).post(models::create),
        )
        .route("/__byok-api__/api/models/discover", post(models::discover))
        .route(
            "/__byok-api__/api/models/import-v0049",
            get(models::preview_v0049).post(models::import_v0049),
        )
        .route("/__byok-api__/api/models/order", put(models::reorder))
        .route("/__byok-api__/api/overview", get(overview::get))
        .route(
            "/__byok-api__/api/models/customizations",
            get(combos::list_customizations),
        )
        .route(
            "/__byok-api__/api/models/customizations/{model_id}",
            put(combos::set_customization),
        )
        .route(
            "/__byok-api__/api/router/combos",
            get(combos::list_combos).post(combos::upsert_combo),
        )
        .route(
            "/__byok-api__/api/router/combos/{combo_id}",
            axum::routing::delete(combos::delete_combo),
        )
        .route(
            "/__byok-api__/api/router/auto",
            get(combos::get_auto_router).put(combos::set_auto_router),
        )
        .route(
            "/__byok-api__/api/models/{model_hash}",
            put(models::update).delete(models::remove),
        )
        .route(
            "/__byok-api__/api/models/{model_hash}/test/{test_id}",
            post(models::test).delete(models::cancel),
        )
        .route("/__byok-api__/api/llm-calls", get(calls::list))
        .route("/__byok-api__/api/llm-calls/{call_id}", get(calls::detail))
        .route("/__byok-api__/api/plugins", get(plugins::list))
        .route(
            "/__byok-api__/api/plugins/runtime",
            get(plugins::runtime_status)
                .post(plugins::initialize_runtime)
                .delete(plugins::cancel_runtime_initialization),
        )
        .route(
            "/__byok-api__/api/plugins/oauth/{session_id}/poll",
            post(plugins::oauth_poll),
        )
        .route(
            "/__byok-api__/api/plugins/{plugin_id}",
            axum::routing::delete(plugins::remove),
        )
        .route(
            "/__byok-api__/api/plugins/{plugin_id}/pool-strategy",
            get(plugins::get_pool_strategy).put(plugins::set_pool_strategy),
        )
        .route(
            "/__byok-api__/api/plugins/{plugin_id}/resources/{resource_type}/add/{method_id}/begin",
            post(plugins::oauth_begin),
        )
        .route(
            "/__byok-api__/api/plugins/{plugin_id}/resources/{resource_type}/import",
            post(plugins::import),
        )
        .route(
            "/__byok-api__/api/plugins/{plugin_id}/resources/{resource_type}/export",
            get(plugins::export_resources),
        )
        .route(
            "/__byok-api__/api/plugins/{plugin_id}/resources/{resource_type}/{resource_id}",
            axum::routing::delete(plugins::delete_resource),
        )
        .route(
            "/__byok-api__/api/plugins/{plugin_id}/resources/{resource_type}/{resource_id}/actions/{action_id}",
            post(plugins::action),
        )
        .route(
            "/__byok-api__/api/plugins/{plugin_id}/resources/{resource_type}/{resource_id}/refresh",
            post(plugins::refresh_resource),
        )
        .route(
            "/__byok-api__/api/plugins/{plugin_id}/resources/{resource_type}/{resource_id}/enabled",
            put(plugins::set_resource_enabled),
        )
        .route(
            "/__byok-api__/api/plugins/{plugin_id}/providers/{provider_id}/models/sync",
            post(plugins::sync_models),
        )
        .route(
            "/__byok-api__/api/plugins/{plugin_id}/providers/{provider_id}/models/enabled",
            put(plugins::set_model_enabled),
        )
        .route(
            "/__byok-api__/api/settings/observability",
            get(settings::get).put(settings::update),
        )
        .route(
            "/__byok-api__/api/settings/ports",
            get(settings::get_ports).put(settings::update_ports),
        )
        .route(
            "/__byok-api__/api/settings/storage/statistics",
            get(settings::get_storage).delete(settings::clear_storage),
        )
        .route(
            "/__byok-api__/api/settings/proxy",
            get(settings::get_proxy).put(settings::update_proxy),
        )
        .route(
            "/__byok-api__/api/settings/tab",
            get(settings::get_tab).put(settings::update_tab),
        )
        .route(
            "/__byok-api__/api/settings/desktop",
            get(settings::get_desktop).put(settings::update_desktop),
        )
        .route(
            "/__byok-api__/api/settings/commit",
            get(settings::get_commit).put(settings::update_commit),
        )
        .route(
            "/__byok-api__/api/settings/cmdk",
            get(settings::get_cmdk).put(settings::update_cmdk),
        )
        .route(
            "/__byok-api__/api/settings/pricing",
            get(settings::get_pricing_settings).put(settings::update_pricing_settings),
        )
        .route(
            "/__byok-api__/api/settings/backup/export",
            get(settings::export_backup),
        )
        .route(
            "/__byok-api__/api/settings/backup/restore",
            post(settings::restore_backup),
        )
        .route(
            "/__byok-api__/api/harness/cursor/status",
            get(harness::status),
        )
        .route(
            "/__byok-api__/api/harness/cursor/ca/initialize",
            post(harness::initialize_ca),
        )
        .route(
            "/__byok-api__/api/harness/cursor/enabled",
            put(harness::set_enabled),
        )
        .route(
            "/__byok-api__/api/harness/cursor/repair",
            post(harness::repair),
        )
        .with_state(service)
        // The Cursor router caps bodies at 128 MiB; the console API needs its own
        // ceiling. Backup restore is the largest legitimate payload, and it still
        // stays far below this bound.
        .layer(DefaultBodyLimit::max(CONTROL_MAX_BODY_BYTES))
        .layer(desktop_cors())
}

/// Console payloads are JSON configuration, never bulk media.
const CONTROL_MAX_BODY_BYTES: usize = 32 * 1024 * 1024;

fn desktop_cors() -> CorsLayer {
    CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(|origin, _| local_origin(origin)))
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
        .allow_headers([
            CONTENT_TYPE,
            header::ACCEPT_LANGUAGE,
            header::HeaderName::from_static("disable-ad-ids"),
        ])
}

fn local_origin(origin: &HeaderValue) -> bool {
    let Ok(origin) = origin.to_str() else {
        return false;
    };
    if origin.eq_ignore_ascii_case("tauri://localhost") {
        return true;
    }
    let Ok(origin) = Url::parse(origin) else {
        return false;
    };
    if !matches!(origin.scheme(), "http" | "https")
        || !origin.username().is_empty()
        || origin.password().is_some()
        || origin.path() != "/"
        || origin.query().is_some()
        || origin.fragment().is_some()
    {
        return false;
    }
    // The console API carries no bearer token, so the origin check is its only
    // authorisation boundary. Loopback only: a private/LAN address would let any
    // other machine on the network drive this API from a browser. The dev frontend
    // on a private address was the only reason `is_private` was ever accepted, and
    // it can use loopback instead.
    match origin.host() {
        Some(Host::Domain(host)) => {
            host.eq_ignore_ascii_case("localhost") || host.eq_ignore_ascii_case("tauri.localhost")
        }
        Some(Host::Ipv4(address)) => address.is_loopback(),
        Some(Host::Ipv6(address)) => address.is_loopback(),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn origin(value: &str) -> HeaderValue {
        HeaderValue::from_str(value).expect("valid header value")
    }

    #[test]
    fn console_api_accepts_only_loopback_origins() {
        // The desktop shell and a loopback dev server.
        assert!(local_origin(&origin("tauri://localhost")));
        assert!(local_origin(&origin("http://localhost:1420")));
        assert!(local_origin(&origin("http://127.0.0.1:1420")));
        assert!(local_origin(&origin("http://[::1]:1420")));
    }

    #[test]
    fn console_api_rejects_lan_and_remote_origins() {
        // A machine on the same network must not be able to drive this API.
        assert!(!local_origin(&origin("http://192.168.1.50:1420")));
        assert!(!local_origin(&origin("http://10.0.0.5")));
        assert!(!local_origin(&origin("http://172.16.4.2")));
        assert!(!local_origin(&origin("http://169.254.10.10")));
        assert!(!local_origin(&origin("http://[fd00::1]")));
        assert!(!local_origin(&origin("http://evil.example.com")));
    }

    #[test]
    fn console_api_rejects_ambiguous_and_non_http_origins() {
        assert!(!local_origin(&origin("null")));
        assert!(!local_origin(&origin("file:///tmp")));
        assert!(!local_origin(&origin("http://user:pass@127.0.0.1:1420")));
        assert!(!local_origin(&origin("http://127.0.0.1:1420/path")));
        assert!(!local_origin(&origin("http://127.0.0.1:1420/?x=1")));
        assert!(!local_origin(&origin("http://127.0.0.1:1420#frag")));
    }
}
