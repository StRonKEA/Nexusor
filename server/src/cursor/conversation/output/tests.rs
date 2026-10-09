#[test]
fn explicit_user_stop_is_typed_and_not_a_transport_cancellation() {
    use base64::Engine;
    use prost::Message;
    let error = super::user_stop_error();
    assert_eq!(
        error.code,
        crate::cursor::protocol::connect::ConnectCode::InvalidArgument
    );
    let data = super::STANDARD_NO_PAD
        .decode(&error.details[0].value)
        .unwrap();
    let detail =
        crate::cursor::protocol::proto::aiserver::v1::ErrorDetails::decode(data.as_slice())
            .unwrap();
    assert_eq!(detail.error, 21);
    assert_eq!(detail.details.unwrap().is_retryable, Some(false));
}

#[tokio::test]
async fn explicit_stop_changes_only_its_transport_terminal_frame() {
    use crate::cursor::{
        services::observability::CursorTraceService,
        transport::{OutputHub, TransportHandle},
    };
    let store = crate::store::Store::connect("sqlite::memory:")
        .await
        .unwrap();
    for explicit in [false, true] {
        let (commands, _receiver) = tokio::sync::mpsc::channel(2);
        let handle = TransportHandle::new(
            "stop-test".into(),
            commands,
            std::sync::Arc::new(OutputHub::default()),
            CursorTraceService::new(store.clone()).recorder("stop-test"),
        );
        let mut output = handle.subscribe();
        if explicit {
            handle.mark_user_stop();
        }
        super::finish_cancelled(&handle).unwrap();
        let bytes = output.recv().await.unwrap();
        assert_eq!(bytes[0], crate::cursor::protocol::connect::END_STREAM_FLAG);
        let frame: serde_json::Value = serde_json::from_slice(&bytes[5..]).unwrap();
        assert_eq!(
            frame["error"]["code"],
            if explicit {
                "invalid_argument"
            } else {
                "canceled"
            }
        );
        assert_eq!(frame["error"]["details"].is_array(), explicit);
        assert!(output.recv().await.is_none());
    }
}
use super::{accept_tool_completion, checkpoint_context_tokens};
use crate::{cursor::protocol::proto::agent::v1 as pb, run::CommandResult, Error};

#[test]
fn compacted_checkpoint_replaces_the_in_memory_context_usage() {
    let compacted = pb::ConversationStateStructure {
        token_details: Some(pb::ConversationTokenDetails {
            used_tokens: 20_000,
            ..Default::default()
        }),
        ..Default::default()
    };

    assert_eq!(checkpoint_context_tokens(&compacted), Some(20_000));
    assert_eq!(
        checkpoint_context_tokens(&pb::ConversationStateStructure::default()),
        None
    );
}

#[test]
fn closing_and_ended_runs_ignore_known_tool_completions() {
    for delivery in [CommandResult::RunClosing, CommandResult::RunEnded] {
        assert!(!accept_tool_completion(delivery, "request", "run", "call").unwrap());
    }
}

#[test]
fn stale_target_remains_an_error() {
    assert!(matches!(
        accept_tool_completion(CommandResult::StaleTarget, "request", "run", "call"),
        Err(Error::RunNotFound(request_id)) if request_id == "request"
    ));
}
