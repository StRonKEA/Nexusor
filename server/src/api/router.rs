//! Builds the top-level server router.

use axum::http::{header, Method};
use tower_http::cors::{Any, CorsLayer};

use crate::{cursor::transport::TransportRegistry, network::NetworkClients, Result};

pub fn router(registry: TransportRegistry, clients: NetworkClients) -> Result<axum::Router> {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([
            header::AUTHORIZATION,
            header::CONTENT_TYPE,
            header::ACCEPT,
            header::ORIGIN,
            header::USER_AGENT,
            header::HeaderName::from_static("x-api-key"),
            header::HeaderName::from_static("anthropic-version"),
            header::HeaderName::from_static("anthropic-beta"),
        ]);

    let cursor_router = super::cursor::router(registry.clone(), clients)?;
    let openai_router = super::openai::router(registry.clone());
    let claude_router = super::claude::router(registry);

    Ok(cursor_router
        .merge(openai_router)
        .merge(claude_router)
        .layer(cors))
}
