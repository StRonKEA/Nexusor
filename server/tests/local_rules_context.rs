//! Verifies local markdown rules are merged into the request-context message.
#[path = "support/fake_provider.rs"]
mod fake_provider;
#[path = "support/fixtures.rs"]
mod fixtures;

use std::sync::Arc;

use cursor_server::{
    cursor::{
        prompting::{PromptAssets, PromptCompiler},
        protocol::connect,
        protocol::proto::agent::v1 as pb,
        TransportCommand, TransportRegistry,
    },
    model::{ContentPart, ProjectedContent},
    provider::{FinishReason, ModelEvent},
};
use prost::Message;

#[tokio::test]
async fn local_markdown_rules_land_in_the_request_context_message() {
    let (_store_dir, store) = fixtures::temp_store().await;
    let provider = fake_provider::FakeProvider::default();
    provider.push(vec![
        ModelEvent::Start {
            model_call_id: "call-1".into(),
        },
        ModelEvent::TextStart,
        ModelEvent::TextDelta("ok".into()),
        ModelEvent::TextEnd,
        ModelEvent::Done(FinishReason::Stop),
    ]);
    let assets = PromptAssets::load(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("prompt/cursor")
            .as_path(),
    )
    .unwrap();

    let rules_dir = tempfile::tempdir().unwrap();
    let rules_root = rules_dir.path().join("rules");
    std::fs::create_dir_all(&rules_root).unwrap();
    std::fs::write(rules_root.join("17353272.md"), "Always answer in haiku.").unwrap();

    let selected_blob = store
        .put_blob(b"blob-backed selected context", &[])
        .await
        .unwrap();
    let mut run = user_run();
    let Some(pb::agent_client_message::Message::RunRequest(request)) = run.message.as_mut() else {
        unreachable!()
    };
    let Some(pb::conversation_action::Action::UserMessageAction(action)) =
        request.action.as_mut().unwrap().action.as_mut()
    else {
        unreachable!()
    };
    action.request_context = Some(pb::RequestContext {
        hooks_additional_context: Some("request hook guidance".into()),
        ..Default::default()
    });
    action.user_message.as_mut().unwrap().selected_context = Some(pb::SelectedContext {
        extra_context_entries: vec![
            pb::ExtraContextEntry {
                data_or_blob_id: Some(pb::extra_context_entry::DataOrBlobId::Data(
                    "inline selected context".into(),
                )),
            },
            pb::ExtraContextEntry {
                data_or_blob_id: Some(pb::extra_context_entry::DataOrBlobId::BlobId(
                    selected_blob.as_bytes().to_vec(),
                )),
            },
        ],
        git_diff: Some(pb::SelectedGitDiff {
            content: "diff-selected-marker".into(),
            ..Default::default()
        }),
        selected_browsers: vec![pb::SelectedBrowser {
            browser_id: "browser-selected-marker".into(),
            url: "https://example.com".into(),
            ..Default::default()
        }],
        ui_elements: vec![pb::SelectedUiElement {
            text_content: "ui-selected-marker".into(),
            ..Default::default()
        }],
        folders: vec![pb::SelectedFolder {
            path: "C:/project".into(),
            directory_tree: Some(pb::LsDirectoryTreeNode {
                abs_path: "C:/project".into(),
                children_dirs: vec![pb::LsDirectoryTreeNode {
                    abs_path: "C:/project/src".into(),
                    children_files: vec![pb::ls_directory_tree_node::File {
                        name: "nested-folder-marker.rs".into(),
                        ..Default::default()
                    }],
                    children_were_processed: true,
                    num_files: 1,
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        }],
        git_pr_diff_selections: vec![pb::SelectedGitPrDiffSelection {
            pr_url: "https://example.com/pull/1".into(),
            file_path: "pr-file-marker.rs".into(),
            start_line: 7,
            end_line: 12,
            blob_id: Some(selected_blob.as_bytes().to_vec()),
            ..Default::default()
        }],
        selected_pull_requests: vec![pb::SelectedPullRequest {
            number: 1,
            title: Some("pr-title-marker".into()),
            summary_json: Some("pr-summary-marker".into()),
            description: Some("pr-description-marker".into()),
            ..Default::default()
        }],
        selected_documents: vec![
            pb::SelectedDocument {
                filename: "inline.txt".into(),
                mime_type: "text/plain; charset=utf-8".into(),
                data_or_blob_id: Some(pb::selected_document::DataOrBlobId::Data(
                    b"inline-document-marker".to_vec(),
                )),
                ..Default::default()
            },
            pb::SelectedDocument {
                filename: "blob.txt".into(),
                mime_type: "text/plain".into(),
                data_or_blob_id: Some(pb::selected_document::DataOrBlobId::BlobId(
                    selected_blob.as_bytes().to_vec(),
                )),
                ..Default::default()
            },
            pb::SelectedDocument {
                filename: "report.pdf".into(),
                mime_type: "application/pdf".into(),
                path: "C:/project/report&notes.pdf".into(),
                ..Default::default()
            },
        ],
        selected_videos: vec![pb::SelectedVideo {
            filename: "demo.mp4".into(),
            path: "C:/project/demo.mp4".into(),
            mime_type: "video/mp4".into(),
            ..Default::default()
        }],
        recent_agents_context: Some(pb::RecentAgentsContext {
            recent_agents: vec![pb::RecentAgent {
                name: "Previous investigation".into(),
                path: "C:/transcripts/previous.jsonl".into(),
                overview: Some("recent-overview-marker".into()),
            }],
        }),
        external_links: vec![pb::SelectedExternalLink {
            url: "https://example.com/report.pdf".into(),
            blob_id: Some(selected_blob.as_bytes().to_vec()),
            ..Default::default()
        }],
        ..Default::default()
    });
    let registry = TransportRegistry::with_local_rules(
        store,
        Arc::new(provider.clone()),
        PromptCompiler::new(assets),
        rules_root,
    );
    let handle = registry.get_or_create("rules-request").await.unwrap();
    let mut output = handle.subscribe();
    handle
        .command(TransportCommand::Append {
            seqno: 0,
            message: Box::new(run),
        })
        .await
        .unwrap();

    let mut append_seqno = 1;
    loop {
        let frame = tokio::time::timeout(std::time::Duration::from_secs(5), output.recv())
            .await
            .expect("run finishes within timeout")
            .expect("output stays open until EndStream");
        let (flags, payload) = connect::decode_frames(&frame).unwrap().pop().unwrap();
        if flags & connect::END_STREAM_FLAG != 0 {
            break;
        }
        // The Run waits for the client to confirm every conversation Blob write,
        // so the stream only advances once each KvServerMessage is acknowledged.
        if let Some(pb::agent_server_message::Message::KvServerMessage(kv)) =
            pb::AgentServerMessage::decode(payload).unwrap().message
        {
            handle
                .command(TransportCommand::Append {
                    seqno: append_seqno,
                    message: Box::new(set_blob_result(kv.id)),
                })
                .await
                .unwrap();
            append_seqno += 1;
        }
    }

    let requests = provider.requests();
    assert_eq!(requests.len(), 1);
    let context_texts = requests[0]
        .history
        .iter()
        .filter(|message| message.message_id.starts_with("request-context:"))
        .map(|message| {
            let ProjectedContent::Parts(parts) = &message.content else {
                panic!("request context message must be parts")
            };
            let [ContentPart::Text { text }] = parts.as_slice() else {
                panic!("request context message must be one text part")
            };
            text.clone()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        context_texts.len(),
        1,
        "exactly one request-context message is projected"
    );
    assert!(
        context_texts[0].contains("<user_rule>\nAlways answer in haiku.\n</user_rule>"),
        "local markdown rule must appear as a user rule: {}",
        context_texts[0]
    );

    assert!(context_texts[0].contains("request hook guidance"));
    let all_text = requests[0]
        .history
        .iter()
        .flat_map(|message| match &message.content {
            ProjectedContent::Parts(parts) => parts
                .iter()
                .filter_map(|part| match part {
                    ContentPart::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            _ => Vec::new(),
        })
        .collect::<Vec<_>>()
        .join("\n");
    for marker in [
        "inline selected context",
        "blob-backed selected context",
        "diff-selected-marker",
        "browser-selected-marker",
        "ui-selected-marker",
        "nested-folder-marker.rs",
        "pr-file-marker.rs",
        "start_line=\"7\" end_line=\"12\"",
        "pr-title-marker",
        "pr-summary-marker",
        "pr-description-marker",
    ] {
        assert!(
            all_text.contains(marker),
            "provider context is missing {marker}"
        );
    }
    for marker in [
        "inline-document-marker",
        "filename=\"blob.txt\"",
        "C:/project/report&amp;notes.pdf",
        "Read the attached path",
        "C:/project/demo.mp4",
        "Raw audio is not included",
        "bounded timestamped transcript",
        "respect its coverage limits",
        "recent-overview-marker",
        "C:/transcripts/previous.jsonl",
        "https://example.com/report.pdf\nblob-backed selected context",
    ] {
        assert!(
            all_text.contains(marker),
            "provider attachment context is missing {marker}"
        );
    }
    registry.shutdown().await;
}

fn set_blob_result(id: u32) -> pb::AgentClientMessage {
    pb::AgentClientMessage {
        message: Some(pb::agent_client_message::Message::KvClientMessage(
            pb::KvClientMessage {
                id,
                message: Some(pb::kv_client_message::Message::SetBlobResult(
                    pb::SetBlobResult { error: None },
                )),
            },
        )),
    }
}

fn user_run() -> pb::AgentClientMessage {
    pb::AgentClientMessage {
        message: Some(pb::agent_client_message::Message::RunRequest(
            pb::AgentRunRequest {
                action: Some(pb::ConversationAction {
                    action: Some(pb::conversation_action::Action::UserMessageAction(
                        pb::UserMessageAction {
                            user_message: Some(pb::UserMessage {
                                text: "hello".into(),
                                message_id: "rules-user".into(),
                                mode: pb::AgentMode::Agent as i32,
                                ..Default::default()
                            }),
                            ..Default::default()
                        },
                    )),
                    ..Default::default()
                }),
                conversation_id: Some("rules-conversation".into()),
                run_id: Some("rules-request".into()),
                requested_model: Some(pb::RequestedModel {
                    model_id: "test-model".into(),
                    ..Default::default()
                }),
                ..Default::default()
            },
        )),
    }
}
