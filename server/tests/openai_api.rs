//! Tests the OpenAI-compatible API routes (/v1/models and /v1/chat/completions).

use std::sync::Arc;

use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use cursor_server::{
    cursor::{
        prompting::{PromptAssets, PromptCompiler},
        transport::TransportRegistry,
    },
    model::ModelInvocation,
    network::NetworkClients,
    provider::{Provider, ProviderStream},
    store::Store,
};
use serde_json::Value;
use tower::ServiceExt;

struct DummyProvider;

impl Provider for DummyProvider {
    fn stream(&self, _: ModelInvocation, _: tokio_util::sync::CancellationToken) -> ProviderStream {
        Box::pin(futures_util::stream::empty())
    }
}

#[tokio::test]
async fn openai_v1_models_endpoint_lists_configured_and_auto_models() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&format!(
        "sqlite://{}",
        directory.path().join("test.db").display()
    ))
    .await
    .unwrap();
    let clients = NetworkClients::new(store.clone());
    let assets =
        PromptAssets::load(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("prompt/cursor"))
            .unwrap();
    let registry =
        TransportRegistry::new(store, Arc::new(DummyProvider), PromptCompiler::new(assets));
    let app = cursor_server::api::router(registry, clients).unwrap();

    let request = Request::builder()
        .method("GET")
        .uri("/v1/models")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let json: Value = serde_json::from_slice(&bytes).unwrap();

    assert_eq!(json["object"], "list");
    let data = json["data"].as_array().unwrap();
    let ids: Vec<&str> = data
        .iter()
        .filter_map(|m| m["id"].as_str())
        .collect();

    assert!(ids.contains(&"auto"));
    assert!(ids.contains(&"auto-smart"));
}
