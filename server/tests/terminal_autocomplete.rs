//! Checks the terminal completion RPC using the current Cursor wire schema.
#[path = "support/fake_provider.rs"]
mod fake_provider;
#[path = "support/fixtures.rs"]
mod fixtures;

use std::sync::Arc;

use axum::{
    body::{to_bytes, Body},
    http::{header, Request, StatusCode},
};
use cursor_server::{
    api::cursor,
    cursor::{
        prompting::{PromptAssets, PromptCompiler},
        protocol::{connect, proto::aiserver::v1 as ai},
        transport::TransportRegistry,
    },
    model::{ContentPart, ProjectedContent},
    network::NetworkClients,
    provider::{FinishReason, ModelEvent},
};
use prost::Message;
use tower::ServiceExt;

async fn response(
    provider: fake_provider::FakeProvider,
    timeout: &str,
) -> axum::response::Response {
    let (_directory, store) = fixtures::temp_store().await;
    let assets =
        PromptAssets::load(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("prompt/cursor"))
            .unwrap();
    let clients = NetworkClients::new(store.clone());
    let registry = TransportRegistry::new(store, Arc::new(provider), PromptCompiler::new(assets));
    let router = cursor::router(registry, clients).unwrap();
    let input = ai::StreamTerminalAutocompleteRequest {
        current_command: "git st".into(),
        command_history: vec!["git diff".into(), "git log".into()],
        model_name: Some("plugin:test/provider/model".into()),
        git_diff: Some("diff --git a/example b/example".into()),
        commit_history: vec!["fix: terminal".into()],
        past_results: vec!["working tree clean".into()],
        ..Default::default()
    };
    router
        .oneshot(
            Request::post("/aiserver.v1.AiService/StreamTerminalAutocomplete")
                .header(header::CONTENT_TYPE, "application/connect+proto")
                .header("connect-timeout-ms", timeout)
                .body(Body::from(connect::encode_message(&input).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn streams_model_suffix_and_current_context_with_completion_marker() {
    let provider = fake_provider::FakeProvider::default();
    provider.push(vec![
        ModelEvent::TextDelta("`at".into()),
        ModelEvent::TextDelta("us`".into()),
        ModelEvent::Done(FinishReason::Stop),
    ]);
    let response = response(provider.clone(), "30000").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()[header::CONTENT_TYPE],
        "application/connect+proto"
    );
    let bytes = to_bytes(response.into_body(), 65536).await.unwrap();
    let frames = connect::decode_frames(&bytes).unwrap();
    assert_eq!(frames.len(), 2);
    let messages: Vec<_> = frames[..1]
        .iter()
        .map(|(flag, bytes)| {
            assert_eq!(*flag, 0);
            ai::StreamTerminalAutocompleteResponse::decode(bytes.clone()).unwrap()
        })
        .collect();
    assert_eq!(
        messages.iter().map(|m| m.text.as_str()).collect::<String>(),
        "atus"
    );
    assert_eq!(messages[0].done_stream, Some(true));
    assert_eq!(frames[1].0, connect::END_STREAM_FLAG);
    assert_eq!(frames[1].1.as_ref(), b"{}");
    let requests = provider.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].model.model_id, "plugin:test/provider/model");
    assert!(requests[0].prompt.tools.is_empty());
    let ProjectedContent::Parts(parts) = &requests[0].history[0].content else {
        panic!("expected parts")
    };
    let ContentPart::Text { text } = &parts[0] else {
        panic!("expected text")
    };
    for expected in [
        "git st",
        "git diff",
        "git log",
        "diff --git a/example",
        "fix: terminal",
        "working tree clean",
    ] {
        assert!(text.contains(expected), "missing {expected}");
    }
}

#[tokio::test]
async fn failed_incomplete_and_timed_out_streams_end_with_protocol_errors() {
    for case in [
        "provider",
        "incomplete",
        "timeout",
        "tool",
        "truncated",
        "oversized",
    ] {
        let provider = fake_provider::FakeProvider::default();
        match case {
            "provider" => {
                provider.push_error(cursor_server::Error::Provider("test failure".into()))
            }
            "incomplete" => provider.push(vec![ModelEvent::TextDelta("partial".into())]),
            "truncated" => provider.push(vec![
                ModelEvent::TextDelta("partial".into()),
                ModelEvent::Done(FinishReason::Length),
            ]),
            "oversized" => provider.push(vec![
                ModelEvent::TextDelta("x".repeat(65537)),
                ModelEvent::Done(FinishReason::Stop),
            ]),
            "timeout" => provider.push_pending(),
            _ => provider.push(vec![ModelEvent::ToolCallStart {
                index: 0,
                call_id: "call".into(),
                name: "shell".into(),
            }]),
        }
        let response = response(provider, "10").await;
        let bytes = to_bytes(response.into_body(), 65536).await.unwrap();
        let frames = connect::decode_frames(&bytes).unwrap();
        assert_eq!(frames.len(), 1, "{case}");
        assert_eq!(frames[0].0, connect::END_STREAM_FLAG);
        let payload: serde_json::Value = serde_json::from_slice(&frames[0].1).unwrap();
        assert_eq!(payload["error"]["code"], "internal");
        assert!(!payload["error"]["message"].as_str().unwrap().is_empty());
    }
}
