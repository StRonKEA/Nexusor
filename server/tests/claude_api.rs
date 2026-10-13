//! Tests the Anthropic Claude compatible API route (/v1/messages).

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
use serde_json::{json, Value};
use tower::ServiceExt;

struct DummyProvider;

impl Provider for DummyProvider {
    fn stream(&self, _: ModelInvocation, _: tokio_util::sync::CancellationToken) -> ProviderStream {
        Box::pin(futures_util::stream::empty())
    }
}

#[tokio::test]
async fn claude_v1_messages_endpoint_responds_in_anthropic_format() {
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

    let payload = json!({
        "model": "claude-3-5-sonnet-20241022",
        "system": "You are a helpful assistant.",
        "messages": [
            {
                "role": "user",
                "content": "Hello world"
            }
        ],
        "max_tokens": 1024
    });

    let request = Request::builder()
        .method("POST")
        .uri("/v1/messages")
        .header("content-type", "application/json")
        .body(Body::from(payload.to_string()))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let json: Value = serde_json::from_slice(&bytes).unwrap();

    assert_eq!(json["type"], "message");
    assert_eq!(json["role"], "assistant");
    assert_eq!(json["model"], "claude-3-5-sonnet-20241022");
    assert_eq!(json["stop_reason"], "end_turn");
}

#[tokio::test]
async fn claude_v1_messages_count_tokens_endpoint_returns_input_tokens() {
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

    let payload = json!({
        "model": "claude-3-5-sonnet-20241022",
        "system": "You are a helpful assistant.",
        "messages": [
            {
                "role": "user",
                "content": "This is a prompt to count tokens for."
            }
        ]
    });

    let request = Request::builder()
        .method("POST")
        .uri("/v1/messages/count_tokens")
        .header("content-type", "application/json")
        .body(Body::from(payload.to_string()))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let json: Value = serde_json::from_slice(&bytes).unwrap();

    assert!(json["input_tokens"].as_u64().unwrap() > 0);
}

