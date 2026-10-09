//! Exercises the actual context-wrapper handshake and selected-range contract.
#[path = "support/fake_provider.rs"]
mod fake_provider;
#[path = "support/fixtures.rs"]
mod fixtures;

use axum::{
    body::{to_bytes, Body},
    http::{header, Request},
};
use cursor_server::{
    api::cursor,
    cursor::{
        prompting::{PromptAssets, PromptCompiler},
        protocol::{connect, proto::cmdk as pb},
        transport::TransportRegistry,
    },
    model::{ContentPart, ProjectedContent},
    network::NetworkClients,
    provider::{FinishReason, ModelEvent},
    store::CmdKSettings,
};
use prost::Message;
use std::sync::Arc;
use tower::ServiceExt;

fn item(value: pb::context_item::Item) -> pb::CachedItem {
    pb::CachedItem {
        item: Some(pb::cached_item::Item::ContextItem(pb::ContextItem {
            item: Some(value),
        })),
    }
}

fn input(terminal: bool) -> pb::Request {
    let mut context_items = vec![item(if terminal {
        pb::context_item::Item::TerminalCmdKQuery(pb::Query {
            query: "Print hello in PowerShell".into(),
        })
    } else {
        pb::context_item::Item::CmdKQuery(pb::Query {
            query: "Replace Hello with Hi".into(),
        })
    })];
    if !terminal {
        context_items.push(item(pb::context_item::Item::CmdKSelection(pb::Selection {
            lines: vec!["  return 'Hello';".into()],
            start_line_number: 7,
        })));
        context_items.push(item(pb::context_item::Item::CmdKImmediateContext(
            pb::ImmediateContext {
                relative_workspace_path: "greet.js".into(),
                lines: vec![pb::Line {
                    line: "function greet() {".into(),
                    line_number: 6,
                }],
                total_number_of_lines_in_file: 12,
                cell_number: None,
            },
        )));
    } else {
        context_items.push(item(pb::context_item::Item::TerminalHistory(
            pb::TerminalHistory {
                history: "PS C:\\workspace>".into(),
                cwd_full: "C:\\workspace".into(),
                ..Default::default()
            },
        )));
    }
    pb::Request {
        context_items,
        cmd_k_options: Some(pb::Options {
            model_details: Some(pb::ModelDetails {
                model_name: Some("default".into()),
            }),
            ..Default::default()
        }),
        session_id: "cmdk-session".into(),
        ..Default::default()
    }
}

async fn router(provider: fake_provider::FakeProvider) -> (tempfile::TempDir, axum::Router) {
    let (dir, store) = fixtures::temp_store().await;
    store
        .set_cmdk_settings(CmdKSettings {
            editor_model_id: "plugin:test/provider/editor".into(),
            terminal_model_id: "plugin:test/provider/terminal".into(),
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

async fn send(
    router: axum::Router,
    input: &pb::Request,
    terminal: bool,
    timeout: &str,
) -> bytes::Bytes {
    use std::io::Write;
    let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gzip.write_all(&input.encode_to_vec()).unwrap();
    let payload = gzip.finish().unwrap();
    let mut frame = vec![1];
    frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    frame.extend_from_slice(&payload);
    let path = if terminal {
        "StreamTerminalCmdK"
    } else {
        "StreamCmdK"
    };
    let response = router
        .oneshot(
            Request::post(format!("/aiserver.v1.CmdKService/{path}"))
                .header(header::CONTENT_TYPE, "application/connect+proto")
                .header("connect-timeout-ms", timeout)
                .header("connect-content-encoding", "gzip")
                .body(Body::from(frame))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    to_bytes(response.into_body(), 65536).await.unwrap()
}

#[tokio::test]
async fn cached_context_is_requested_before_generation_then_full_selection_reaches_model() {
    let provider = fake_provider::FakeProvider::default();
    provider.push(vec![
        ModelEvent::TextDelta("```js\n  return 'Hi';\n```".into()),
        ModelEvent::Done(FinishReason::Stop),
    ]);
    let (_dir, router) = router(provider.clone()).await;
    let mut request = input(false);
    request.context_items.push(pb::CachedItem {
        item: Some(pb::cached_item::Item::ContextItemHash("cached-rule".into())),
    });
    let bytes = send(router.clone(), &request, false, "30000").await;
    let frames = connect::decode_frames(&bytes).unwrap();
    assert!(provider.requests().is_empty());
    let response = pb::Response::decode(frames[0].1.clone()).unwrap();
    let Some(pb::response::Response::MissingContextItems(missing)) = response.response else {
        panic!("expected missing context")
    };
    assert_eq!(missing.missing_context_item_hashes, vec!["cached-rule"]);
    assert_eq!(frames[1].1.as_ref(), b"{}");
    request.context_items.pop();
    request
        .context_items
        .push(item(pb::context_item::Item::ProjectRule(pb::Rule {
            body: Some("Keep semicolons".into()),
            ..Default::default()
        })));
    let bytes = send(router, &request, false, "30000").await;
    let frames = connect::decode_frames(&bytes).unwrap();
    assert_eq!(frames.len(), 4);
    let values: Vec<_> = frames[..3]
        .iter()
        .map(|(_, b)| {
            let Some(pb::response::Response::RealResponse(r)) =
                pb::Response::decode(b.clone()).unwrap().response
            else {
                panic!()
            };
            r.response.unwrap()
        })
        .collect();
    assert!(
        matches!(&values[0], pb::edit_response::Response::EditStart(s) if s.start_line_number == 7 && s.max_end_line_number_exclusive == Some(8))
    );
    assert!(
        matches!(&values[1], pb::edit_response::Response::EditStream(s) if s.text == "  return 'Hi';")
    );
    assert!(
        matches!(&values[2], pb::edit_response::Response::EditEnd(s) if s.end_line_number_exclusive == 8)
    );
    let requests = provider.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].model.model_id, "plugin:test/provider/editor");
    let ProjectedContent::Parts(parts) = &requests[0].history[0].content else {
        panic!()
    };
    let ContentPart::Text { text } = &parts[0] else {
        panic!()
    };
    for value in [
        "Replace Hello",
        "return 'Hello';",
        "greet.js",
        "Keep semicolons",
    ] {
        assert!(text.contains(value), "{value}");
    }
}

#[tokio::test]
async fn terminal_and_chat_use_their_distinct_wire_fields_and_selected_model() {
    for chat in [false, true] {
        let provider = fake_provider::FakeProvider::default();
        provider.push(vec![
            ModelEvent::TextDelta("Write-Output 'hello'".into()),
            ModelEvent::Done(FinishReason::Stop),
        ]);
        let (_dir, router) = router(provider.clone()).await;
        let mut request = input(true);
        request.cmd_k_options.as_mut().unwrap().chat_mode = chat;
        let bytes = send(router, &request, true, "30000").await;
        let frames = connect::decode_frames(&bytes).unwrap();
        assert_eq!(frames.len(), 2);
        let response = pb::TerminalResponse::decode(frames[0].1.clone())
            .unwrap()
            .real_response
            .unwrap()
            .response
            .unwrap();
        assert_eq!(
            matches!(response, pb::terminal_result::Response::Chat(_)),
            chat
        );
        assert_eq!(
            provider.requests()[0].model.model_id,
            "plugin:test/provider/terminal"
        );
        assert!(provider.requests()[0].prompt.tools.is_empty());
    }
}

#[tokio::test]
async fn provider_failure_timeout_and_truncation_never_complete_an_edit_successfully() {
    for case in ["error", "timeout", "truncated", "incomplete"] {
        let provider = fake_provider::FakeProvider::default();
        match case {
            "error" => provider.push_error(cursor_server::Error::Provider("failed".into())),
            "timeout" => provider.push_pending(),
            "truncated" => provider.push(vec![
                ModelEvent::TextDelta("partial".into()),
                ModelEvent::Done(FinishReason::Length),
            ]),
            _ => provider.push(vec![]),
        }
        let (_dir, router) = router(provider).await;
        let bytes = send(router, &input(false), false, "10").await;
        let frames = connect::decode_frames(&bytes).unwrap();
        let (flag, body) = frames.last().unwrap();
        assert_eq!(*flag, connect::END_STREAM_FLAG);
        let error: serde_json::Value = serde_json::from_slice(body).unwrap();
        assert_eq!(error["error"]["code"], "internal", "{case}");
    }
}
