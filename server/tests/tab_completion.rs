#[path = "support/fake_provider.rs"]
mod fake_provider;
#[path = "support/fixtures.rs"]
mod fixtures;

use axum::{
    body::{to_bytes, Body},
    http::{header, Request},
};
use base64::{engine::general_purpose::STANDARD, Engine};
use cursor_server::{
    api::cursor,
    cursor::{
        prompting::{PromptAssets, PromptCompiler},
        protocol::{
            connect,
            proto::{aiserver::v1 as ai, tab as pb},
        },
        transport::TransportRegistry,
    },
    network::NetworkClients,
    provider::{FinishReason, ModelEvent},
    store::CmdKSettings,
};
use prost::Message;
use std::sync::Arc;
use tower::ServiceExt;

async fn setup(provider: fake_provider::FakeProvider) -> (tempfile::TempDir, axum::Router) {
    let (dir, store) = fixtures::temp_store().await;
    store
        .set_cmdk_settings(CmdKSettings {
            tab_model_id: "plugin:test/provider/tab".into(),
            ..Default::default()
        })
        .await
        .unwrap();
    let assets =
        PromptAssets::load(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("prompt/cursor"))
            .unwrap();
    let clients = NetworkClients::new(store.clone());
    let registry = TransportRegistry::new(store, Arc::new(provider), PromptCompiler::new(assets));
    (dir, cursor::router(registry, clients).unwrap())
}

async fn send(router: axum::Router, input: &pb::Request) -> bytes::Bytes {
    let r = router
        .oneshot(
            Request::post("/aiserver.v1.AiService/StreamCpp")
                .header(header::CONTENT_TYPE, "application/connect+proto")
                .body(Body::from(connect::encode_message(input).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    to_bytes(r.into_body(), 65536).await.unwrap()
}

#[tokio::test]
async fn filesync_retry_then_unicode_insertion_preserves_prefix_suffix_and_wire_order() {
    let provider = fake_provider::FakeProvider::default();
    provider.push(vec![
        ModelEvent::TextDelta("hello".into()),
        ModelEvent::Done(FinishReason::Stop),
    ]);
    let (_dir, router) = setup(provider.clone()).await;
    let mut input = pb::Request {
        current_file: Some(pb::CurrentFile {
            rely_on_filesync: true,
            ..Default::default()
        }),
        ..Default::default()
    };
    let bytes = send(router.clone(), &input).await;
    let frames = connect::decode_frames(&bytes).unwrap();
    let error: serde_json::Value = serde_json::from_slice(&frames[0].1).unwrap();
    let detail = STANDARD
        .decode(error["error"]["details"][0]["value"].as_str().unwrap())
        .unwrap();
    assert_eq!(
        ai::ErrorDetails::decode(detail.as_slice()).unwrap().error,
        33
    );
    assert!(provider.requests().is_empty());
    input.current_file = Some(pb::CurrentFile {
        relative_workspace_path: "emoji.js".into(),
        contents: "// header\r\nconst x = '😀';".into(),
        cursor_position: Some(pb::Position {
            line: 1,
            column: 13,
        }),
        ..Default::default()
    });
    let bytes = send(router, &input).await;
    let frames = connect::decode_frames(&bytes).unwrap();
    let values: Vec<_> = frames[..frames.len() - 1]
        .iter()
        .map(|(_, b)| pb::Response::decode(b.clone()).unwrap())
        .collect();
    assert!(values[0].model_info.is_some());
    assert!(values[0].text.is_empty());
    assert_eq!(
        values[1]
            .range_to_replace
            .as_ref()
            .unwrap()
            .start_line_number,
        2
    );
    assert!(values[1].text.is_empty());
    assert_eq!(values[2].text, "const x = '😀hello';");
    assert!(values.iter().all(|value| value.done_edit.is_none()));
    assert_eq!(values[3].done_stream, Some(true));
    assert_eq!(
        provider.requests()[0].model.model_id,
        "plugin:test/provider/tab"
    );
}

#[tokio::test]
async fn incomplete_generation_does_not_emit_a_partial_suggestion() {
    let provider = fake_provider::FakeProvider::default();
    provider.push(vec![
        ModelEvent::TextDelta("partial".into()),
        ModelEvent::Done(FinishReason::Length),
    ]);
    let (_dir, router) = setup(provider).await;
    let bytes = send(
        router,
        &pb::Request {
            current_file: Some(pb::CurrentFile {
                contents: "return ".into(),
                cursor_position: Some(pb::Position { line: 0, column: 7 }),
                ..Default::default()
            }),
            ..Default::default()
        },
    )
    .await;
    let frames = connect::decode_frames(&bytes).unwrap();
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].0, connect::END_STREAM_FLAG);
    assert!(
        serde_json::from_slice::<serde_json::Value>(&frames[0].1).unwrap()["error"].is_object()
    );
}

#[tokio::test]
async fn multidiff_adjusts_following_ranges_and_marks_each_edit_boundary() {
    let provider = fake_provider::FakeProvider::default();
    provider.push(vec![ModelEvent::TextDelta(r#"{"edits":[{"start_line":1,"end_line":1,"text":"const name = 1;\n// added"},{"start_line":3,"end_line":3,"text":"use(name);"}]}"#.into()), ModelEvent::Done(FinishReason::Stop)]);
    let (_dir, router) = setup(provider).await;
    let bytes = send(
        router,
        &pb::Request {
            current_file: Some(pb::CurrentFile {
                contents: "const old = 1;\n// gap\nuse(old);".into(),
                cursor_position: Some(pb::Position { line: 0, column: 5 }),
                ..Default::default()
            }),
            ..Default::default()
        },
    )
    .await;
    let frames = connect::decode_frames(&bytes).unwrap();
    let values: Vec<_> = frames
        .iter()
        .filter(|(flag, _)| *flag == 0)
        .map(|(_, body)| pb::Response::decode(body.clone()).unwrap())
        .collect();
    assert!(values[0].model_info.as_ref().unwrap().is_multidiff_model);
    assert_eq!(
        values.iter().filter(|v| v.done_edit == Some(true)).count(),
        2
    );
    assert_eq!(values[4].begin_edit, Some(true));
    assert_eq!(
        values[5]
            .range_to_replace
            .as_ref()
            .unwrap()
            .start_line_number,
        4
    );
    assert_eq!(values[5].should_remove_leading_eol, Some(true));
    assert_eq!(values[6].text, "\nuse(name);");
}

#[tokio::test]
async fn prediction_is_limited_to_supplied_files_and_rejects_invalid_edits_atomically() {
    let provider = fake_provider::FakeProvider::default();
    provider.push(vec![
        ModelEvent::TextDelta(r#"{"edits":[],"next":{"path":"other.js","line":8}}"#.into()),
        ModelEvent::Done(FinishReason::Stop),
    ]);
    provider.push(vec![ModelEvent::TextDelta(r#"{"edits":[{"start_line":1,"end_line":1,"text":"ok"},{"start_line":8,"end_line":8,"text":"bad"}]}"#.into()), ModelEvent::Done(FinishReason::Stop)]);
    provider.push(vec![
        ModelEvent::TextDelta(r#"{"edits":[],"next":{"path":"../secret","line":1}}"#.into()),
        ModelEvent::Done(FinishReason::Stop),
    ]);
    let (_dir, router) = setup(provider).await;
    let input = pb::Request {
        current_file: Some(pb::CurrentFile {
            contents: "hello".into(),
            cursor_position: Some(pb::Position { line: 0, column: 5 }),
            ..Default::default()
        }),
        supports_cpt: Some(true),
        additional_files: vec![pb::AdditionalFile {
            relative_workspace_path: "other.js".into(),
            visible_range_content: vec!["first\ntarget".into()],
            start_line_number_one_indexed: vec![7],
            ..Default::default()
        }],
        ..Default::default()
    };
    let frames = connect::decode_frames(&send(router.clone(), &input).await).unwrap();
    let target = pb::Response::decode(frames[1].1.clone())
        .unwrap()
        .cursor_prediction_target
        .unwrap();
    assert_eq!(target.relative_path, "other.js");
    assert_eq!(target.line_number_one_indexed, 8);
    assert_eq!(target.expected_content, "target");
    for _ in 0..2 {
        let frames = connect::decode_frames(&send(router.clone(), &input).await).unwrap();
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].0, connect::END_STREAM_FLAG);
        assert!(
            serde_json::from_slice::<serde_json::Value>(&frames[0].1).unwrap()["error"].is_object()
        );
    }
}

#[tokio::test]
async fn multidiff_prediction_uses_post_edit_coordinates() {
    let provider = fake_provider::FakeProvider::default();
    provider.push(vec![
        ModelEvent::TextDelta(r#"{"edits":[{"start_line":1,"end_line":1,"text":"first\nadded"},{"start_line":3,"end_line":3,"text":"changed"}],"next":{"path":"test.js","line":5}}"#.into()),
        ModelEvent::Done(FinishReason::Stop),
    ]);
    let (_dir, router) = setup(provider).await;
    let input = pb::Request {
        current_file: Some(pb::CurrentFile {
            relative_workspace_path: "test.js".into(),
            contents: "first\ngap\nold\ngap\ntarget".into(),
            cursor_position: Some(pb::Position { line: 0, column: 5 }),
            ..Default::default()
        }),
        supports_cpt: Some(true),
        ..Default::default()
    };
    let frames = connect::decode_frames(&send(router, &input).await).unwrap();
    let target = frames
        .iter()
        .filter(|(flag, _)| *flag == 0)
        .find_map(|(_, body)| {
            pb::Response::decode(body.clone())
                .unwrap()
                .cursor_prediction_target
        })
        .unwrap();
    assert_eq!(target.line_number_one_indexed, 6);
    assert_eq!(target.expected_content, "target");
}
