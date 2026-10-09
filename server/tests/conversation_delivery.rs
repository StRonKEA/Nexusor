//! Verifies message delivery before, during, and after a Run.
#[path = "support/fake_provider.rs"]
mod fake_provider;
#[path = "support/fixtures.rs"]
mod fixtures;

use std::{collections::HashMap, sync::Arc};

use cursor_server::{
    cursor::{
        prompting::{PromptAssets, PromptCompiler},
        protocol::connect,
        protocol::proto::agent::v1 as pb,
        TransportCommand, TransportHandle, TransportRegistry,
    },
    model::{ContentPart, MessageContent, ProjectedContent, Role},
    provider::{FinishReason, ModelEvent},
};
use prost::Message;

const FOLLOW_UP: &str = "Perform any necessary follow-up actions in response to the subagent completion above. If no follow-up work is needed, no further action is required. If you mention an agent or subagent in your response, link it with the `[Name](id)` Don't use generic label such as `[agent]`, `[worker]`, or `[subagent]`.";
const SHELL_FOLLOW_UP: &str = "Briefly inform the user about the task result and perform any follow-up actions (if needed). If there's no follow-ups needed, don't explicitly say that.";

#[tokio::test]
async fn nexusor_stop_closes_active_transport_instead_of_leaving_heartbeat_only() {
    let (_directory, store) = fixtures::temp_store().await;
    let provider = fake_provider::FakeProvider::default();
    let _gate = provider.push_gated(stop_response("held", "must not finish"));
    let assets = PromptAssets::load(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("prompt/cursor")
            .as_path(),
    )
    .unwrap();
    let registry = TransportRegistry::new(
        store.clone(),
        Arc::new(provider.clone()),
        PromptCompiler::new(assets),
    );
    let handle = registry
        .get_or_create("explicit-stop-regression")
        .await
        .unwrap();
    let mut output = handle.subscribe();
    handle
        .command(TransportCommand::Append {
            seqno: 0,
            message: Box::new(completion_run(
                "child",
                "stop-regression",
                pb::ConversationStateStructure::default(),
            )),
        })
        .await
        .unwrap();
    let mut seqno = 1;
    let mut stopped = false;
    tokio::time::timeout(std::time::Duration::from_secs(8), async {
        loop {
            if !stopped && !provider.requests().is_empty() {
                let (reply, receive) = tokio::sync::oneshot::channel();
                handle
                    .command(TransportCommand::InspectLocal { reply })
                    .await
                    .unwrap();
                let active = receive.await.unwrap().unwrap();
                let request = serde_json::from_value(
                    serde_json::json!({"run_id":active.run_id,"action":"stop_run"}),
                )
                .unwrap();
                let (reply, receive) = tokio::sync::oneshot::channel();
                handle
                    .command(TransportCommand::LocalControl { request, reply })
                    .await
                    .unwrap();
                assert_eq!(receive.await.unwrap().unwrap().status, "run_stop_requested");
                stopped = true;
            }
            let frame = tokio::select! {
                frame=output.recv()=>frame.expect("terminal error frame before closing"),
                _=tokio::time::sleep(std::time::Duration::from_millis(10)),if !stopped=>continue,
            };
            let (flags, payload) = connect::decode_frames(&frame).unwrap().pop().unwrap();
            if flags & connect::END_STREAM_FLAG != 0 {
                assert!(stopped);
                let error: serde_json::Value = serde_json::from_slice(&payload).unwrap();
                assert_eq!(error["error"]["code"], "invalid_argument");
                assert!(error["error"]["message"]
                    .as_str()
                    .unwrap()
                    .contains("stopped from Nexusor"));
                break;
            }
            let message = pb::AgentServerMessage::decode(payload).unwrap();
            let response = match message.message {
                Some(pb::agent_server_message::Message::ExecServerMessage(exec)) => {
                    assert_eq!(exec.id, 0);
                    pb::AgentClientMessage {
                        message: Some(pb::agent_client_message::Message::ExecClientMessage(
                            pb::ExecClientMessage {
                                id: 0,
                                message: Some(
                                    pb::exec_client_message::Message::RequestContextResult(
                                        pb::RequestContextResult {
                                            result: Some(
                                                pb::request_context_result::Result::Success(
                                                    pb::RequestContextSuccess {
                                                        request_context: Some(
                                                            pb::RequestContext::default(),
                                                        ),
                                                        ..Default::default()
                                                    },
                                                ),
                                            ),
                                        },
                                    ),
                                ),
                                ..Default::default()
                            },
                        )),
                    }
                }
                Some(pb::agent_server_message::Message::KvServerMessage(kv)) => kv_ack(kv.id),
                _ => continue,
            };
            handle
                .command(TransportCommand::Append {
                    seqno,
                    message: Box::new(response),
                })
                .await
                .unwrap();
            seqno += 1;
        }
    })
    .await
    .expect("explicit stop must close the transport");
    assert_eq!(provider.requests().len(), 1);
    let status: String = sqlx::query_scalar(
        "SELECT status FROM runs WHERE cursor_request_id='explicit-stop-regression'",
    )
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(status, "cancelled");
    registry.shutdown().await;
}

#[tokio::test]
async fn background_subagent_completion_starts_a_simulated_parent_turn() {
    let (_directory, store) = fixtures::temp_store().await;
    let provider = fake_provider::FakeProvider::default();
    provider.push(stop_response("model-call", "followed up"));
    let assets = PromptAssets::load(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("prompt/cursor")
            .as_path(),
    )
    .unwrap();
    let registry = TransportRegistry::new(
        store.clone(),
        Arc::new(provider.clone()),
        PromptCompiler::new(assets),
    );
    let handle = registry.get_or_create("completion-request").await.unwrap();
    let (checkpoint, blobs) = drive_completion(
        &handle,
        completion_run(
            "child-id",
            "reusable-parent-run",
            pb::ConversationStateStructure {
                mode: Some(pb::AgentMode::Multitask as i32),
                ..Default::default()
            },
        ),
    )
    .await;

    let requests = provider.requests();
    assert_eq!(requests.len(), 1);
    let [runtime] = requests[0].history.as_slice() else {
        panic!("completion Run must add exactly one runtime message")
    };
    assert_eq!(runtime.role, Role::User);
    let ProjectedContent::Parts(parts) = &runtime.content else {
        panic!("completion context must be text")
    };
    let [ContentPart::Text { text }] = parts.as_slice() else {
        panic!("completion context must have one text part")
    };
    assert!(text.contains("kind: subagent"));
    assert!(text.contains("agent_id: child-id"));
    assert!(text.contains("child result"));
    assert!(text.contains(FOLLOW_UP));

    let messages = store
        .load_current_messages(&cursor_server::model::ConversationId::new(
            "parent-conversation",
        ))
        .await
        .unwrap();
    assert!(messages.iter().any(|message| {
        message.runtime_event_id.as_deref()
            == Some("background-completed:BACKGROUND_TASK_KIND_SUBAGENT:child-id:task-call")
            && matches!(&message.content, MessageContent::Parts { parts } if !parts.is_empty())
    }));

    let turn = pb::ConversationTurnStructure::decode(
        blobs
            .get(checkpoint.turns.last().expect("completion Turn"))
            .expect("completion Turn Blob")
            .as_slice(),
    )
    .unwrap();
    let pb::conversation_turn_structure::Turn::AgentConversationTurn(turn) = turn.turn.unwrap()
    else {
        panic!("expected agent conversation Turn")
    };
    let user = pb::UserMessage::decode(
        blobs
            .get(&turn.user_message)
            .expect("simulated UserMessage Blob")
            .as_slice(),
    )
    .unwrap();
    assert!(user.text.contains(FOLLOW_UP));
    assert_eq!(user.is_simulated_msg, Some(true));
    assert_eq!(
        user.simulated_msg_reason,
        Some(pb::SimulatedMsgReason::BackgroundTaskCompletion as i32)
    );
    assert_eq!(
        user.simulated_message_metadata.unwrap().task_id.as_deref(),
        Some("child-id")
    );

    provider.push(stop_response("model-call-2", "followed up again"));
    let second = registry
        .get_or_create("completion-request-2")
        .await
        .unwrap();
    drive_completion(
        &second,
        completion_run("child-id-2", "reusable-parent-run-2", checkpoint),
    )
    .await;

    let requests = provider.requests();
    assert_eq!(requests.len(), 2);
    let runtime_ids = requests[1]
        .history
        .iter()
        .map(|message| message.message_id.as_str())
        .filter(|id| id.starts_with("runtime:"))
        .collect::<Vec<_>>();
    assert_eq!(
        runtime_ids,
        [
            "runtime:background-completed:BACKGROUND_TASK_KIND_SUBAGENT:child-id:task-call",
            "runtime:background-completed:BACKGROUND_TASK_KIND_SUBAGENT:child-id-2:task-call"
        ]
    );
}

#[tokio::test]
async fn background_completion_joins_the_active_run_instead_of_replacing_it() {
    let (_directory, store) = fixtures::temp_store().await;
    let provider = fake_provider::FakeProvider::default();
    let first_ready = provider.push_gated(stop_response("model-call-1", "first response"));
    provider.push(stop_response("model-call-2", "processed both completions"));
    let assets = PromptAssets::load(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("prompt/cursor")
            .as_path(),
    )
    .unwrap();
    let registry = TransportRegistry::new(
        store.clone(),
        Arc::new(provider.clone()),
        PromptCompiler::new(assets),
    );
    let first = registry.get_or_create("active-completion-1").await.unwrap();
    let first_run = tokio::spawn(async move {
        drive_completion(
            &first,
            completion_run(
                "child-1",
                "parent-run-1",
                pb::ConversationStateStructure::default(),
            ),
        )
        .await
    });
    while provider.requests().is_empty() {
        tokio::task::yield_now().await;
    }

    let second = registry.get_or_create("active-completion-2").await.unwrap();
    let second_run = tokio::spawn(async move {
        drive_forwarded_completion(
            &second,
            completion_run(
                "child-2",
                "parent-run-2",
                pb::ConversationStateStructure::default(),
            ),
        )
        .await
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    first_ready.notify_one();

    second_run.await.unwrap();
    first_run.await.unwrap();
    let requests = provider.requests();
    assert_eq!(requests.len(), 2);
    let history = serde_json::to_string(&requests[1].history).unwrap();
    assert!(history.contains("child-1"));
    assert!(history.contains("first response"));
    assert!(history.contains("child-2"));
    let statuses: Vec<String> = sqlx::query_scalar(
        "SELECT status FROM runs WHERE conversation_id = 'parent-conversation' ORDER BY created_at_ms",
    )
    .fetch_all(store.pool())
    .await
    .unwrap();
    assert_eq!(statuses, ["completed"]);
}

#[tokio::test]
async fn retrying_one_background_completion_reuses_its_runtime_message() {
    let (_directory, store) = fixtures::temp_store().await;
    let provider = fake_provider::FakeProvider::default();
    provider.push(stop_response("model-call", "followed up"));
    let assets = PromptAssets::load(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("prompt/cursor")
            .as_path(),
    )
    .unwrap();
    let registry = TransportRegistry::new(
        store.clone(),
        Arc::new(provider.clone()),
        PromptCompiler::new(assets),
    );
    let first = registry.get_or_create("completion-retry-1").await.unwrap();
    let (checkpoint, _) = drive_completion(
        &first,
        completion_run(
            "retry-child",
            "completion-retry-run-1",
            pb::ConversationStateStructure {
                mode: Some(pb::AgentMode::Multitask as i32),
                ..Default::default()
            },
        ),
    )
    .await;

    provider.push(stop_response("model-call-2", "followed up again"));
    let second = registry.get_or_create("completion-retry-2").await.unwrap();
    drive_completion(
        &second,
        completion_run("retry-child", "completion-retry-run-2", checkpoint),
    )
    .await;

    let messages = store
        .load_current_messages(&cursor_server::model::ConversationId::new(
            "parent-conversation",
        ))
        .await
        .unwrap();
    assert_eq!(
        messages
            .iter()
            .filter(|message| {
                message.runtime_event_id.as_deref()
                    == Some(
                        "background-completed:BACKGROUND_TASK_KIND_SUBAGENT:retry-child:task-call",
                    )
            })
            .count(),
        1
    );
}

#[tokio::test]
async fn background_shell_completion_wakes_the_parent_with_the_captured_notification() {
    let (_directory, store) = fixtures::temp_store().await;
    let provider = fake_provider::FakeProvider::default();
    provider.push(stop_response(
        "shell-wakeup",
        "The background server was stopped.",
    ));
    let assets = PromptAssets::load(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("prompt/cursor")
            .as_path(),
    )
    .unwrap();
    let registry = TransportRegistry::new(
        store,
        Arc::new(provider.clone()),
        PromptCompiler::new(assets),
    );
    let handle = registry
        .get_or_create("shell-completion-request")
        .await
        .unwrap();
    let (checkpoint, blobs) = drive_completion(
        &handle,
        shell_completion_run(pb::ConversationStateStructure {
            mode: Some(pb::AgentMode::Agent as i32),
            ..Default::default()
        }),
    )
    .await;

    let requests = provider.requests();
    let [runtime] = requests[0].history.as_slice() else {
        panic!("Shell completion Run must add exactly one runtime message")
    };
    let ProjectedContent::Parts(parts) = &runtime.content else {
        panic!("Shell completion context must be text")
    };
    let [ContentPart::Text { text }] = parts.as_slice() else {
        panic!("Shell completion context must have one text part")
    };
    assert!(text.contains("<system_notification>"));
    assert!(text.contains("kind: shell"));
    assert!(text.contains("status: aborted"));
    assert!(text.contains("task_id: 977679"));
    assert!(text.contains("detail: terminated_by_user"));
    assert!(text.contains("output_path: /tmp/977679.txt"));
    assert!(text.contains(SHELL_FOLLOW_UP));
    assert!(text.starts_with("<timestamp>"));
    assert!(!text.contains("You are still in **Agent Mode**"));
    assert!(text.find("<system_notification>").unwrap() < text.find("<user_query>").unwrap());

    let turn = pb::ConversationTurnStructure::decode(
        blobs
            .get(checkpoint.turns.last().expect("Shell completion Turn"))
            .expect("Shell completion Turn Blob")
            .as_slice(),
    )
    .unwrap();
    let pb::conversation_turn_structure::Turn::AgentConversationTurn(turn) = turn.turn.unwrap()
    else {
        panic!("expected agent conversation Turn")
    };
    let user = pb::UserMessage::decode(
        blobs
            .get(&turn.user_message)
            .expect("simulated Shell UserMessage Blob")
            .as_slice(),
    )
    .unwrap();
    assert_eq!(user.text, *text);
    assert_eq!(user.is_simulated_msg, Some(true));
    assert_eq!(
        user.simulated_msg_reason,
        Some(pb::SimulatedMsgReason::BackgroundTaskCompletion as i32)
    );
    let metadata = user.simulated_message_metadata.unwrap();
    assert_eq!(
        metadata.title.as_deref(),
        Some("Start Python HTTP server on 9000")
    );
    assert_eq!(metadata.task_id.as_deref(), Some("977679"));
}

async fn drive_completion(
    handle: &TransportHandle,
    message: pb::AgentClientMessage,
) -> (pb::ConversationStateStructure, HashMap<Vec<u8>, Vec<u8>>) {
    drive_completion_with_runtime(handle, message, None).await
}

#[tokio::test]
async fn bidi_runtime_background_completion_preserves_active_run_and_deduplicates() {
    let (_directory, store) = fixtures::temp_store().await;
    let provider = fake_provider::FakeProvider::default();
    let gate = provider.push_gated(stop_response("active-model", "foreground result"));
    provider.push(stop_response("continued-model", "handled notification"));
    let assets = PromptAssets::load(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("prompt/cursor")
            .as_path(),
    )
    .unwrap();
    let registry = TransportRegistry::new(
        store.clone(),
        Arc::new(provider.clone()),
        PromptCompiler::new(assets),
    );
    let handle = registry
        .get_or_create("bidi-runtime-background")
        .await
        .unwrap();
    let (send, receive) = tokio::sync::oneshot::channel();
    let run = tokio::spawn(async move {
        drive_completion_with_runtime(
            &handle,
            completion_run(
                "initial-child",
                "bidi-parent",
                pb::ConversationStateStructure::default(),
            ),
            Some(receive),
        )
        .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while provider.requests().is_empty() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let pb::agent_client_message::Message::RunRequest(request) = completion_run(
        "runtime-child",
        "ignored",
        pb::ConversationStateStructure::default(),
    )
    .message
    .unwrap() else {
        unreachable!()
    };
    send.send((
        pb::AgentClientMessage {
            message: Some(pb::agent_client_message::Message::ConversationAction(
                request.action.unwrap(),
            )),
        },
        gate,
    ))
    .unwrap();
    run.await.unwrap();
    let requests = provider.requests();
    assert_eq!(requests.len(), 2);
    let history = serde_json::to_string(&requests[1].history).unwrap();
    assert!(history.contains("foreground result"));
    assert!(history.contains("runtime-child"));
    assert_eq!(
        requests[1]
            .history
            .iter()
            .filter(|message| {
                serde_json::to_string(message)
                    .unwrap()
                    .contains("task_id: runtime-child")
            })
            .count(),
        1,
        "duplicate runtime delivery must append only one notification"
    );
    let statuses: Vec<String> =
        sqlx::query_scalar("SELECT status FROM runs WHERE conversation_id = 'parent-conversation'")
            .fetch_all(store.pool())
            .await
            .unwrap();
    assert_eq!(statuses, ["completed"]);
}

#[tokio::test]
async fn runtime_progress_does_not_interrupt_or_add_a_model_turn() {
    let (_directory, store) = fixtures::temp_store().await;
    let provider = fake_provider::FakeProvider::default();
    let gate = provider.push_gated(stop_response("active-model", "foreground completed"));
    let assets = PromptAssets::load(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("prompt/cursor")
            .as_path(),
    )
    .unwrap();
    let registry = TransportRegistry::new(
        store.clone(),
        Arc::new(provider.clone()),
        PromptCompiler::new(assets),
    );
    let handle = registry
        .get_or_create("progress-only-runtime")
        .await
        .unwrap();
    let (send, receive) = tokio::sync::oneshot::channel();
    let run = tokio::spawn(async move {
        drive_completion_with_runtime(
            &handle,
            completion_run(
                "initial-child",
                "progress-parent",
                pb::ConversationStateStructure::default(),
            ),
            Some(receive),
        )
        .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while provider.requests().is_empty() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    send.send((
        pb::AgentClientMessage {
            message: Some(pb::agent_client_message::Message::ConversationAction(
                pb::ConversationAction {
                    action: Some(
                        pb::conversation_action::Action::BackgroundTaskCompletionAction(
                            pb::BackgroundTaskCompletionAction {
                                completions: vec![pb::BackgroundTaskCompletion {
                                    task_id: "still-running".into(),
                                    reason: pb::BackgroundTaskCompletionReason::TaskProgress as i32,
                                    ..Default::default()
                                }],
                            },
                        ),
                    ),
                    ..Default::default()
                },
            )),
        },
        gate,
    ))
    .unwrap();
    run.await.unwrap();
    assert_eq!(provider.requests().len(), 1);
    let statuses: Vec<String> =
        sqlx::query_scalar("SELECT status FROM runs WHERE conversation_id = 'parent-conversation'")
            .fetch_all(store.pool())
            .await
            .unwrap();
    assert_eq!(statuses, ["completed"]);
}

fn subscription_request(
    users: Vec<pb::UserMessage>,
    state: pb::ConversationStateStructure,
) -> pb::AgentClientMessage {
    let mut message = completion_run("unused", "subscription", state);
    let Some(pb::agent_client_message::Message::RunRequest(request)) = message.message.as_mut()
    else {
        unreachable!()
    };
    request.action = Some(pb::ConversationAction {
        action: Some(
            pb::conversation_action::Action::SubscriptionNotificationAction(
                pb::SubscriptionNotificationAction {
                    notifications: users,
                    request_context: Some(pb::RequestContext {
                        hooks_additional_context: Some("subscription-context-marker".into()),
                        ..Default::default()
                    }),
                    send_to_interaction_listener: Some(true),
                },
            ),
        ),
        ..Default::default()
    });
    message
}

#[tokio::test]
async fn subscription_batch_preserves_each_context_and_skips_replayed_notifications() {
    let (_directory, store) = fixtures::temp_store().await;
    let provider = fake_provider::FakeProvider::default();
    provider.push(stop_response(
        "notification-model",
        "handled two notifications",
    ));
    let assets = PromptAssets::load(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("prompt/cursor")
            .as_path(),
    )
    .unwrap();
    let registry = TransportRegistry::new(
        store.clone(),
        Arc::new(provider.clone()),
        PromptCompiler::new(assets),
    );
    let users = ["first", "second"]
        .into_iter()
        .map(|id| pb::UserMessage {
            message_id: format!("notification-{id}"),
            text: format!("notification-text-{id}"),
            selected_context: Some(pb::SelectedContext {
                extra_context_entries: vec![pb::ExtraContextEntry {
                    data_or_blob_id: Some(pb::extra_context_entry::DataOrBlobId::Data(format!(
                        "selected-context-{id}"
                    ))),
                }],
                ..Default::default()
            }),
            ..Default::default()
        })
        .collect::<Vec<_>>();
    let first = registry.get_or_create("subscription-first").await.unwrap();
    let mut first_events = first.subscribe();
    let (checkpoint, _) = drive_completion(
        &first,
        subscription_request(users.clone(), Default::default()),
    )
    .await;
    let mut appended = Vec::new();
    while let Ok(frame) = first_events.try_recv() {
        for (flags, data) in connect::decode_frames(&frame).unwrap() {
            if flags & connect::END_STREAM_FLAG != 0 {
                continue;
            }
            let server = pb::AgentServerMessage::decode(data).unwrap();
            if let Some(pb::agent_server_message::Message::InteractionUpdate(update)) =
                server.message
            {
                if let Some(pb::interaction_update::Message::UserMessageAppended(event)) =
                    update.message
                {
                    appended.push(event.user_message.unwrap());
                }
            }
        }
    }
    assert_eq!(
        appended, users,
        "Cursor receives each original notification including its selected context"
    );
    let requests = provider.requests();
    assert_eq!(requests.len(), 1);
    let history = serde_json::to_string(&requests[0].history).unwrap();
    for expected in [
        "notification-text-first",
        "notification-text-second",
        "selected-context-first",
        "selected-context-second",
        "subscription-context-marker",
    ] {
        assert!(history.contains(expected), "missing {expected}");
    }
    let retry = registry.get_or_create("subscription-retry").await.unwrap();
    drive_forwarded_completion(
        &retry,
        subscription_request(users.clone(), checkpoint.clone()),
    )
    .await;
    assert_eq!(
        provider.requests().len(),
        1,
        "replay must not call the model again"
    );
    let mut mixed = users;
    mixed.push(pb::UserMessage {
        message_id: "notification-third".into(),
        text: "notification-text-third".into(),
        ..Default::default()
    });
    provider.push(stop_response("third-model", "handled new notification"));
    let third = registry.get_or_create("subscription-mixed").await.unwrap();
    let mut third_events = third.subscribe();
    let mut quiet_request = subscription_request(mixed, checkpoint);
    let Some(pb::agent_client_message::Message::RunRequest(request)) =
        quiet_request.message.as_mut()
    else {
        unreachable!()
    };
    let Some(pb::conversation_action::Action::SubscriptionNotificationAction(action)) = request
        .action
        .as_mut()
        .and_then(|action| action.action.as_mut())
    else {
        unreachable!()
    };
    action.send_to_interaction_listener = Some(false);
    drive_completion(&third, quiet_request).await;
    while let Ok(frame) = third_events.try_recv() {
        for (flags, data) in connect::decode_frames(&frame).unwrap() {
            if flags & connect::END_STREAM_FLAG != 0 {
                continue;
            }
            let server = pb::AgentServerMessage::decode(data).unwrap();
            if let Some(pb::agent_server_message::Message::InteractionUpdate(update)) =
                server.message
            {
                assert!(
                    !matches!(
                        update.message,
                        Some(pb::interaction_update::Message::UserMessageAppended(_))
                    ),
                    "false must suppress appended notifications"
                );
            }
        }
    }
    let messages = store
        .load_current_messages(&cursor_server::model::ConversationId::new(
            "parent-conversation",
        ))
        .await
        .unwrap();
    for id in ["first", "second", "third"] {
        assert_eq!(
            messages
                .iter()
                .filter(|message| message.message_id
                    == format!("runtime:subscription:notification-{id}"))
                .count(),
            1
        );
    }
    assert_eq!(provider.requests().len(), 2);
}

fn progress_request() -> pb::AgentClientMessage {
    let mut message = completion_run(
        "progress-child",
        "progress-request",
        pb::ConversationStateStructure::default(),
    );
    let Some(pb::agent_client_message::Message::RunRequest(request)) = message.message.as_mut()
    else {
        unreachable!()
    };
    let Some(pb::conversation_action::Action::BackgroundTaskCompletionAction(action)) = request
        .action
        .as_mut()
        .and_then(|action| action.action.as_mut())
    else {
        unreachable!()
    };
    action.completions[0].reason = pb::BackgroundTaskCompletionReason::TaskProgress as i32;
    message
}

#[tokio::test]
async fn progress_only_run_request_acknowledges_without_model_or_run() {
    let (_directory, store) = fixtures::temp_store().await;
    let provider = fake_provider::FakeProvider::default();
    let assets = PromptAssets::load(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("prompt/cursor")
            .as_path(),
    )
    .unwrap();
    let registry = TransportRegistry::new(
        store.clone(),
        Arc::new(provider.clone()),
        PromptCompiler::new(assets),
    );
    let handle = registry
        .get_or_create("progress-only-request")
        .await
        .unwrap();
    let mut output = handle.subscribe();
    handle
        .command(TransportCommand::Append {
            seqno: 0,
            message: Box::new(progress_request()),
        })
        .await
        .unwrap();
    let frame = tokio::time::timeout(std::time::Duration::from_secs(5), output.recv())
        .await
        .unwrap()
        .unwrap();
    let (flags, payload) = connect::decode_frames(&frame).unwrap().pop().unwrap();
    assert_ne!(flags & connect::END_STREAM_FLAG, 0);
    assert_eq!(payload.as_ref(), b"{}");
    assert!(provider.requests().is_empty());
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM runs")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn progress_run_request_does_not_replace_an_active_generation() {
    let (_directory, store) = fixtures::temp_store().await;
    let provider = fake_provider::FakeProvider::default();
    let gate = provider.push_gated(stop_response("active-model", "foreground preserved"));
    let assets = PromptAssets::load(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("prompt/cursor")
            .as_path(),
    )
    .unwrap();
    let registry = TransportRegistry::new(
        store.clone(),
        Arc::new(provider.clone()),
        PromptCompiler::new(assets),
    );
    let handle = registry
        .get_or_create("progress-request-during-run")
        .await
        .unwrap();
    let (send, receive) = tokio::sync::oneshot::channel();
    let run = tokio::spawn(async move {
        drive_completion_with_runtime(
            &handle,
            completion_run(
                "initial-child",
                "active-parent",
                pb::ConversationStateStructure::default(),
            ),
            Some(receive),
        )
        .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while provider.requests().is_empty() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    send.send((progress_request(), gate)).unwrap();
    run.await.unwrap();
    assert_eq!(provider.requests().len(), 1);
    let statuses: Vec<String> =
        sqlx::query_scalar("SELECT status FROM runs WHERE conversation_id = 'parent-conversation'")
            .fetch_all(store.pool())
            .await
            .unwrap();
    assert_eq!(statuses, ["completed"]);
}

async fn drive_completion_with_runtime(
    handle: &TransportHandle,
    message: pb::AgentClientMessage,
    mut injection: Option<
        tokio::sync::oneshot::Receiver<(pb::AgentClientMessage, Arc<tokio::sync::Notify>)>,
    >,
) -> (pb::ConversationStateStructure, HashMap<Vec<u8>, Vec<u8>>) {
    let mut output = handle.subscribe();
    handle
        .command(TransportCommand::Append {
            seqno: 0,
            message: Box::new(message),
        })
        .await
        .unwrap();

    let mut append_seqno = 1;
    let mut blobs = HashMap::new();
    let mut final_checkpoint = None;
    loop {
        let frame = tokio::select! {
                value = async { injection.as_mut().unwrap().await }, if injection.is_some() => {
                    let (action, ready) = value.unwrap();
                    // Duplicate delivery must retain one event identity.
                    for _ in 0..2 {
                        handle.command(TransportCommand::Append {
                            seqno: append_seqno,
                            message: Box::new(action.clone()),
                        }).await.unwrap();
                        append_seqno += 1;
                    }
                    injection = None;
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                    ready.notify_one();
                    continue;
                }
                frame = tokio::time::timeout(std::time::Duration::from_secs(5), output.recv()) => frame.unwrap().unwrap(),
        };
        let (flags, payload) = connect::decode_frames(&frame).unwrap().pop().unwrap();
        if flags & connect::END_STREAM_FLAG != 0 {
            break;
        }
        let server = pb::AgentServerMessage::decode(payload).unwrap();
        match server.message {
            Some(pb::agent_server_message::Message::ExecServerMessage(exec)) => {
                assert_eq!(exec.id, 0);
                assert!(matches!(
                    exec.message,
                    Some(pb::exec_server_message::Message::RequestContextArgs(_))
                ));
                handle
                    .command(TransportCommand::Append {
                        seqno: append_seqno,
                        message: Box::new(pb::AgentClientMessage {
                            message: Some(
                                pb::agent_client_message::Message::ExecClientControlMessage(
                                    pb::ExecClientControlMessage {
                                        message: Some(
                                            pb::exec_client_control_message::Message::StreamClose(
                                                pb::ExecClientStreamClose { id: 0 },
                                            ),
                                        ),
                                    },
                                ),
                            ),
                        }),
                    })
                    .await
                    .unwrap();
                append_seqno += 1;
                handle
                    .command(TransportCommand::Append {
                        seqno: append_seqno,
                        message: Box::new(pb::AgentClientMessage {
                            message: Some(pb::agent_client_message::Message::ExecClientMessage(
                                pb::ExecClientMessage {
                                    id: 0,
                                    message: Some(
                                        pb::exec_client_message::Message::RequestContextResult(
                                            pb::RequestContextResult {
                                                result: Some(
                                                    pb::request_context_result::Result::Success(
                                                        pb::RequestContextSuccess {
                                                            request_context: Some(
                                                                pb::RequestContext::default(),
                                                            ),
                                                            ..Default::default()
                                                        },
                                                    ),
                                                ),
                                            },
                                        ),
                                    ),
                                    ..Default::default()
                                },
                            )),
                        }),
                    })
                    .await
                    .unwrap();
                append_seqno += 1;
            }
            Some(pb::agent_server_message::Message::KvServerMessage(kv)) => {
                if let Some(pb::kv_server_message::Message::SetBlobArgs(set)) = &kv.message {
                    blobs.insert(set.blob_id.clone(), set.blob_data.clone());
                }
                handle
                    .command(TransportCommand::Append {
                        seqno: append_seqno,
                        message: Box::new(kv_ack(kv.id)),
                    })
                    .await
                    .unwrap();
                append_seqno += 1;
            }
            Some(pb::agent_server_message::Message::ConversationCheckpointUpdate(state))
                if state.pending_tool_calls.is_empty() =>
            {
                final_checkpoint = Some(state);
            }
            _ => {}
        }
    }
    (
        final_checkpoint.expect("settled completion checkpoint"),
        blobs,
    )
}

async fn drive_forwarded_completion(handle: &TransportHandle, message: pb::AgentClientMessage) {
    let mut output = handle.subscribe();
    handle
        .command(TransportCommand::Append {
            seqno: 0,
            message: Box::new(message),
        })
        .await
        .unwrap();
    let mut append_seqno = 1;
    loop {
        let frame = tokio::time::timeout(std::time::Duration::from_secs(5), output.recv())
            .await
            .unwrap()
            .unwrap();
        let (flags, payload) = connect::decode_frames(&frame).unwrap().pop().unwrap();
        if flags & connect::END_STREAM_FLAG != 0 {
            assert_eq!(payload.as_ref(), b"{}");
            return;
        }
        let server = pb::AgentServerMessage::decode(payload).unwrap();
        match server.message {
            Some(pb::agent_server_message::Message::ExecServerMessage(exec)) => {
                assert_eq!(exec.id, 0);
                handle
                    .command(TransportCommand::Append {
                        seqno: append_seqno,
                        message: Box::new(pb::AgentClientMessage {
                            message: Some(
                                pb::agent_client_message::Message::ExecClientControlMessage(
                                    pb::ExecClientControlMessage {
                                        message: Some(
                                            pb::exec_client_control_message::Message::StreamClose(
                                                pb::ExecClientStreamClose { id: 0 },
                                            ),
                                        ),
                                    },
                                ),
                            ),
                        }),
                    })
                    .await
                    .unwrap();
                append_seqno += 1;
                handle
                    .command(TransportCommand::Append {
                        seqno: append_seqno,
                        message: Box::new(pb::AgentClientMessage {
                            message: Some(pb::agent_client_message::Message::ExecClientMessage(
                                pb::ExecClientMessage {
                                    id: 0,
                                    message: Some(
                                        pb::exec_client_message::Message::RequestContextResult(
                                            pb::RequestContextResult {
                                                result: Some(
                                                    pb::request_context_result::Result::Success(
                                                        pb::RequestContextSuccess {
                                                            request_context: Some(
                                                                pb::RequestContext::default(),
                                                            ),
                                                            ..Default::default()
                                                        },
                                                    ),
                                                ),
                                            },
                                        ),
                                    ),
                                    ..Default::default()
                                },
                            )),
                        }),
                    })
                    .await
                    .unwrap();
                append_seqno += 1;
            }
            Some(pb::agent_server_message::Message::KvServerMessage(kv)) => {
                handle
                    .command(TransportCommand::Append {
                        seqno: append_seqno,
                        message: Box::new(kv_ack(kv.id)),
                    })
                    .await
                    .unwrap();
                append_seqno += 1;
            }
            _ => {}
        }
    }
}

fn completion_run(
    child_id: &str,
    run_id: &str,
    conversation_state: pb::ConversationStateStructure,
) -> pb::AgentClientMessage {
    completion_run_with_detail(child_id, run_id, conversation_state, "child result")
}

fn completion_run_with_detail(
    child_id: &str,
    run_id: &str,
    conversation_state: pb::ConversationStateStructure,
    detail: &str,
) -> pb::AgentClientMessage {
    pb::AgentClientMessage {
        message: Some(pb::agent_client_message::Message::RunRequest(
            pb::AgentRunRequest {
                action: Some(pb::ConversationAction {
                    action: Some(
                        pb::conversation_action::Action::BackgroundTaskCompletionAction(
                            pb::BackgroundTaskCompletionAction {
                                completions: vec![pb::BackgroundTaskCompletion {
                                    task_id: child_id.into(),
                                    kind: pb::BackgroundTaskKind::Subagent as i32,
                                    status: pb::BackgroundTaskStatus::Success as i32,
                                    title: "Inspect protocol".into(),
                                    detail: Some(detail.into()),
                                    output_path: Some("/tmp/child.jsonl".into()),
                                    reason: pb::BackgroundTaskCompletionReason::TaskFinished as i32,
                                    subagent_id: Some(child_id.into()),
                                    tool_call_id: Some("task-call".into()),
                                    ..Default::default()
                                }],
                            },
                        ),
                    ),
                    ..Default::default()
                }),
                conversation_id: Some("parent-conversation".into()),
                requested_model: Some(pb::RequestedModel {
                    model_id: "test-model".into(),
                    ..Default::default()
                }),
                conversation_state: Some(conversation_state),
                run_id: Some(run_id.into()),
                ..Default::default()
            },
        )),
    }
}

fn shell_completion_run(
    conversation_state: pb::ConversationStateStructure,
) -> pb::AgentClientMessage {
    pb::AgentClientMessage {
        message: Some(pb::agent_client_message::Message::RunRequest(
            pb::AgentRunRequest {
                action: Some(pb::ConversationAction {
                    action: Some(
                        pb::conversation_action::Action::BackgroundTaskCompletionAction(
                            pb::BackgroundTaskCompletionAction {
                                completions: vec![pb::BackgroundTaskCompletion {
                                    task_id: "977679".into(),
                                    kind: pb::BackgroundTaskKind::Shell as i32,
                                    status: pb::BackgroundTaskStatus::Aborted as i32,
                                    title: "Start Python HTTP server on 9000".into(),
                                    detail: Some("terminated_by_user".into()),
                                    output_path: Some("/tmp/977679.txt".into()),
                                    reason: pb::BackgroundTaskCompletionReason::TaskFinished as i32,
                                    tool_call_id: Some("shell-call".into()),
                                    ..Default::default()
                                }],
                            },
                        ),
                    ),
                    ..Default::default()
                }),
                conversation_id: Some("parent-conversation".into()),
                requested_model: Some(pb::RequestedModel {
                    model_id: "test-model".into(),
                    ..Default::default()
                }),
                conversation_state: Some(conversation_state),
                run_id: Some("shell-parent-run".into()),
                ..Default::default()
            },
        )),
    }
}

fn stop_response(model_call_id: &str, text: &str) -> Vec<ModelEvent> {
    vec![
        ModelEvent::Start {
            model_call_id: model_call_id.into(),
        },
        ModelEvent::TextStart,
        ModelEvent::TextDelta(text.into()),
        ModelEvent::TextEnd,
        ModelEvent::Done(FinishReason::Stop),
    ]
}

fn kv_ack(id: u32) -> pb::AgentClientMessage {
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
