//! Builds the top-level server router.

use crate::{cursor::transport::TransportRegistry, network::NetworkClients, Result};

pub fn router(registry: TransportRegistry, clients: NetworkClients) -> Result<axum::Router> {
    let cursor_router = super::cursor::router(registry.clone(), clients)?;
    let openai_router = super::openai::router(registry);
    Ok(cursor_router.merge(openai_router))
}
