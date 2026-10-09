//! Runtime lifecycle: reservation, output buffering, MCP discovery and interruption.
use super::*;

use crate::cursor::protocol::proto::agent::v1 as pb;
use crate::cursor::tools::tool_call_result::ToolWorkEvent;
use crate::model::ToolCall;

fn call(id: &str, name: &str) -> ToolCall {
    ToolCall {
        call_id: id.into(),
        name: name.into(),
        model_call_id: "m".into(),
        index: 0,
        arguments: serde_json::json!({}),
        arguments_text: "{}".into(),
        argument_error: None,
    }
}

fn context() -> ExecContext {
    ExecContext::default()
}

fn hook_context(event: &str) -> pb::HookAdditionalContext {
    pb::HookAdditionalContext {
        hook_event_name: event.to_owned(),
        content: "recorded".into(),
    }
}

/// A discovery run reporting one server with one tool.
fn mcp_tool(server: &str, tool: &str) -> pb::McpStateExecResult {
    pb::McpStateExecResult {
        result: Some(pb::mcp_state_exec_result::Result::Success(
            pb::McpStateSuccess {
                servers: vec![pb::McpStateServer {
                    server_identifier: server.into(),
                    tools: vec![pb::McpToolDefinition {
                        name: format!("{server}.{tool}"),
                        provider_identifier: server.into(),
                        tool_name: tool.into(),
                        description: "does a thing".into(),
                        ..Default::default()
                    }],
                    ..Default::default()
                }],
            },
        )),
    }
}

/// A discovery run that failed, leaving whatever was already known in place.
fn mcp_error() -> pb::McpStateExecResult {
    pb::McpStateExecResult {
        result: Some(pb::mcp_state_exec_result::Result::Error(
            pb::McpStateError {
                error: "server unreachable".into(),
            },
        )),
    }
}

#[tokio::test]
async fn reserve_assigns_unique_ids_and_keeps_the_call() {
    let runtime = CursorToolRuntime::default();
    let first = runtime
        .reserve_exec(&call("a", "Shell"), &context())
        .await
        .unwrap();
    let second = runtime
        .reserve_exec(&call("b", "Shell"), &context())
        .await
        .unwrap();
    assert_ne!(first, second);
    assert_eq!(runtime.exec_call(first).await.unwrap().call_id, "a");
    assert_eq!(runtime.exec_call(second).await.unwrap().call_id, "b");
    assert!(runtime.exec_call(first + 100).await.is_none());
}

#[tokio::test]
async fn output_buffers_until_the_exec_is_taken() {
    let runtime = CursorToolRuntime::default();
    let id = runtime
        .reserve_exec(&call("a", "Shell"), &context())
        .await
        .unwrap();
    assert!(runtime.append_stdout(id, "out").await);
    assert!(runtime.append_stdout(id, "-more").await);
    assert!(runtime.append_stderr(id, "err").await);
    // An exec that has already been taken no longer accepts output.
    let pending = runtime.take_exec(id).await.unwrap();
    assert_eq!(pending.stdout, "out-more");
    assert_eq!(pending.stderr, "err");
    assert!(!runtime.append_stdout(id, "late").await);
    assert_eq!(runtime.completed_call(id).await.as_deref(), Some("a"));
}

#[tokio::test]
async fn hook_contexts_are_appended_once_each() {
    let runtime = CursorToolRuntime::default();
    let id = runtime
        .reserve_exec(&call("a", "Shell"), &context())
        .await
        .unwrap();
    let first = hook_context("one");
    runtime
        .append_hook_contexts(id, &[first.clone(), hook_context("two")])
        .await;
    runtime
        .append_hook_contexts(id, std::slice::from_ref(&first))
        .await;
    let pending = runtime.take_exec(id).await.unwrap();
    assert_eq!(pending.hook_contexts, vec![first, hook_context("two")]);
}

#[tokio::test]
async fn interactions_complete_like_execs() {
    let runtime = CursorToolRuntime::default();
    let id = runtime
        .reserve_interaction(&call("ask", "ask"))
        .await
        .unwrap();
    assert!(runtime.running_exec_ids().await.is_empty());
    let pending = runtime.take_interaction(id).await.unwrap();
    assert_eq!(pending.call.call_id, "ask");
    assert_eq!(runtime.completed_call(id).await.as_deref(), Some("ask"));
    runtime.discard_interaction(id).await;
    runtime.clear_completed().await;
    assert!(runtime.completed_call(id).await.is_none());
}

#[tokio::test]
async fn a_message_interrupt_keeps_subagents_running() {
    let runtime = CursorToolRuntime::default();
    let shell = runtime
        .reserve_exec(&call("s", "Shell"), &context())
        .await
        .unwrap();
    let task = runtime
        .reserve_exec(&call("t", "Task"), &context())
        .await
        .unwrap();
    let aborts = runtime.interrupt_for_message().await;
    assert_eq!(aborts, vec![shell]);
    assert_eq!(runtime.running_exec_ids().await, vec![task]);
    assert!(runtime.is_interrupted(shell).await);
    assert!(!runtime.is_interrupted(task).await);
    // A late response for the aborted exec is discarded, not treated as a result.
    runtime.discard_exec(shell).await;
    assert!(runtime.running_task_exec_id("t").await.is_some());
    assert!(runtime.running_task_exec_id("s").await.is_none());
}

#[tokio::test]
async fn replacing_a_run_stops_everything_and_cancels_the_local_token() {
    let runtime = CursorToolRuntime::default();
    let shell = runtime
        .reserve_exec(&call("s", "Shell"), &context())
        .await
        .unwrap();
    let interaction = runtime
        .reserve_interaction(&call("ask", "ask"))
        .await
        .unwrap();
    let aborts = runtime.interrupt_for_run_replacement().await;
    assert_eq!(aborts, vec![shell]);
    assert!(runtime.local_cancellation.is_cancelled());
    assert!(runtime.is_interrupted(shell).await);
    assert!(runtime.is_interrupted(interaction).await);
    assert!(runtime.running_exec_ids().await.is_empty());
}

#[tokio::test]
async fn draining_a_run_forgets_every_record() {
    let runtime = CursorToolRuntime::default();
    runtime
        .reserve_exec(&call("s", "Shell"), &context())
        .await
        .unwrap();
    let id = runtime
        .reserve_interaction(&call("ask", "ask"))
        .await
        .unwrap();
    runtime
        .reserve_exec(&call("t", "Task"), &context())
        .await
        .unwrap();
    let drained = runtime.drain_running().await;
    assert_eq!(drained.len(), 2);
    assert!(runtime.local_cancellation.is_cancelled());
    assert!(!runtime.is_interrupted(id).await);
    assert!(runtime.completed_call(id).await.is_none());
    assert!(runtime.running_exec_ids().await.is_empty());
}

#[tokio::test]
async fn a_replacement_run_keeps_interrupted_history_but_not_running_work() {
    let runtime = CursorToolRuntime::default();
    let id = runtime
        .reserve_exec(&call("s", "Shell"), &context())
        .await
        .unwrap();
    runtime.interrupt_for_run_replacement().await;
    let replacement = runtime.next_run();
    assert!(replacement.running_exec_ids().await.is_empty());
    assert!(replacement.is_interrupted(id).await);
    assert!(!replacement.local_cancellation.is_cancelled());
    // The id sequence is shared, so a replacement run cannot reuse an exec id.
    assert_ne!(
        replacement
            .reserve_exec(&call("s", "Shell"), &context())
            .await
            .unwrap(),
        id
    );
}

#[tokio::test]
async fn partial_output_is_reported_once_with_a_truncated_tail() {
    let runtime = CursorToolRuntime::default();
    let shell = runtime
        .reserve_exec(&call("s", "Shell"), &context())
        .await
        .unwrap();
    let quiet = runtime
        .reserve_exec(&call("q", "Read"), &context())
        .await
        .unwrap();
    assert!(runtime.interrupted_output("run-1").await.is_none());
    runtime.append_stdout(shell, "partial").await;
    let message = runtime.interrupted_output("run-1").await.unwrap();
    let crate::model::MessageContent::Parts { parts } = message.content else {
        panic!("expected text parts")
    };
    let crate::model::ContentPart::Text { text } = &parts[0] else {
        panic!("expected text")
    };
    assert_eq!(message.message_id, "interrupted-output:run-1");
    assert!(text.contains("Shell"));
    assert!(text.contains("partial"));
    // An exec that produced nothing is not worth reporting.
    assert!(!text.contains(&quiet.to_string()));
}

#[tokio::test]
async fn discovered_mcp_routes_win_over_the_request_metadata() {
    let runtime = CursorToolRuntime::default();
    let mut context = context();
    context.mcp_routes.insert(
        ("docs".into(), "search".into()),
        McpRoute {
            name: "docs.search".into(),
            provider_identifier: "docs".into(),
            tool_name: "search".into(),
            description: "from metadata".into(),
        },
    );
    let discovery = mcp_tool("docs", "search");
    runtime
        .discover_mcp(&call("get", "GetMcpTools"), &discovery)
        .await;
    let route = runtime.mcp_route(&context, "docs", "search").await.unwrap();
    assert_eq!(route.description, "does a thing");
    // A server the discovery covered has no route beyond what it reported.
    assert!(runtime
        .mcp_route(&context, "docs", "missing")
        .await
        .is_none());
}

#[tokio::test]
async fn a_full_discovery_run_stops_falling_back_to_metadata() {
    let runtime = CursorToolRuntime::default();
    let mut context = context();
    context.mcp_routes.insert(
        ("docs".into(), "search".into()),
        McpRoute {
            name: "docs.search".into(),
            provider_identifier: "docs".into(),
            tool_name: "search".into(),
            description: "from metadata".into(),
        },
    );
    runtime
        .discover_mcp(&call("get", "GetMcpTools"), &mcp_tool("other", "ping"))
        .await;
    assert!(runtime
        .mcp_route(&context, "docs", "search")
        .await
        .is_none());
}

#[tokio::test]
async fn a_server_scoped_discovery_leaves_others_to_metadata() {
    let runtime = CursorToolRuntime::default();
    let mut context = context();
    context.mcp_routes.insert(
        ("docs".into(), "search".into()),
        McpRoute {
            name: "docs.search".into(),
            provider_identifier: "docs".into(),
            tool_name: "search".into(),
            description: "from metadata".into(),
        },
    );
    let mut scoped = call("get", "GetMcpTools");
    scoped.arguments = serde_json::json!({"server": "other"});
    runtime
        .discover_mcp(&scoped, &mcp_tool("other", "ping"))
        .await;
    let route = runtime.mcp_route(&context, "docs", "search").await.unwrap();
    assert_eq!(route.description, "from metadata");
    assert!(runtime
        .mcp_route(&context, "other", "missing")
        .await
        .is_none());
}

#[tokio::test]
async fn a_failed_discovery_leaves_the_previous_routes_intact() {
    let runtime = CursorToolRuntime::default();
    let context = context();
    runtime
        .discover_mcp(&call("get", "GetMcpTools"), &mcp_tool("docs", "search"))
        .await;
    runtime
        .discover_mcp(&call("get", "GetMcpTools"), &mcp_error())
        .await;
    assert!(runtime
        .mcp_route(&context, "docs", "search")
        .await
        .is_some());
}

#[tokio::test]
async fn an_image_scope_ends_when_the_generation_finishes() {
    let runtime = CursorToolRuntime::default();
    let scope = runtime.begin_image_operation("image-1").await;
    assert!(!scope.is_cancelled());
    runtime.finish_image_operation("image-1").await;
    // A later interrupt has no scope left to cancel.
    runtime.interrupt_for_message().await;
    assert!(!scope.is_cancelled());
}

#[tokio::test]
async fn a_dropped_guard_cancels_the_image_scope() {
    let runtime = CursorToolRuntime::default();
    let scope = runtime.begin_image_operation("image-1").await;
    {
        let _guard = scope.clone().drop_guard();
        assert!(!scope.is_cancelled());
    }
    assert!(scope.is_cancelled());
    assert!(!runtime.local_cancellation.is_cancelled());
}

#[tokio::test]
async fn aborting_an_image_call_reports_the_waiting_exec() {
    let runtime = CursorToolRuntime::default();
    let scope = runtime.begin_image_operation("image-1").await;
    let guard = scope.clone().drop_guard();
    let child = scope.clone();
    let (sender, mut receiver) = super::super::tool_call_result::tool_result_channel();
    let call = call("image-1", "GenerateImage");
    let worker = runtime.clone();
    let exchange = tokio::spawn(async move {
        worker
            .image_io(
                &sender,
                &call,
                pb::exec_server_message::Message::ReadArgs(pb::ReadArgs {
                    path: "out.png".into(),
                    tool_call_id: "image-1".into(),
                    ..Default::default()
                }),
                &child,
            )
            .await
    });
    let ToolWorkEvent::Exec { message, .. } = receiver.recv().await.unwrap().unwrap() else {
        panic!("expected exec")
    };
    let Some(pb::agent_server_message::Message::ExecServerMessage(exec)) = message.message else {
        panic!("expected exec")
    };
    assert_eq!(exec.exec_id, format!("image-1:image-io:{}", exec.id));
    assert!(runtime.image_exec_is_active(exec.id, "image-1").await);

    // Dropping the guard is how the caller stops image I/O: the exchange gives up and
    // is unregistered, and Cursor is told to stop watching the exec.
    drop(guard);
    assert!(matches!(exchange.await.unwrap(), Err(Error::Cancelled)));
    assert!(!runtime.image_exec_is_active(exec.id, "image-1").await);
    assert!(!runtime.local_cancellation.is_cancelled());
    let ToolWorkEvent::Exec { message, .. } = receiver.recv().await.unwrap().unwrap() else {
        panic!("expected abort")
    };
    assert_eq!(*message, super::super::codec::abort(exec.id));
    assert!(runtime.abort_image_io("image-1").await.is_empty());
}

#[tokio::test]
async fn a_task_model_is_filled_from_the_run_routing() {
    let mut context = context();
    context.default_subagent_model = "fallback".into();
    context.subagent_model = Some(SubagentModel::Model("run-wide".into()));
    context
        .subagent_models
        .insert("Explore".into(), SubagentModel::Model("scout".into()));

    let general = call("t", "Task");
    assert_eq!(
        context.prepare_call(&general).unwrap().arguments["model"],
        "run-wide"
    );

    let mut named = call("t", "Task");
    named.arguments = serde_json::json!({"subagent_type": "Explore"});
    assert_eq!(
        context.prepare_call(&named).unwrap().arguments["model"],
        "scout"
    );

    // An explicit model in the call is the user's choice and is left alone.
    let mut explicit = call("t", "Task");
    explicit.arguments = serde_json::json!({"model": "mine"});
    assert_eq!(
        context.prepare_call(&explicit).unwrap().arguments["model"],
        "mine"
    );

    // A non-Task call is untouched.
    let shell = call("s", "Shell");
    assert_eq!(
        context.prepare_call(&shell).unwrap().arguments,
        shell.arguments
    );
}

#[tokio::test]
async fn an_unrouted_task_falls_back_to_the_run_default() {
    // No subagent route configured at all: the Task must inherit the parent model,
    // not be sent to Cursor with an empty model id.
    let mut context = context();
    context.default_subagent_model = "parent-model".into();
    let task = call("t", "Task");
    assert_eq!(
        context.prepare_call(&task).unwrap().arguments["model"],
        "parent-model"
    );
    assert!(!context.task_disabled(&task));

    // A subagent type with no route of its own also falls back to the default.
    let mut named = call("t", "Task");
    named.arguments = serde_json::json!({"subagent_type": "Explore"});
    assert_eq!(
        context.prepare_call(&named).unwrap().arguments["model"],
        "parent-model"
    );
    assert!(!context.task_disabled(&named));
}

#[tokio::test]
async fn a_disabled_task_is_left_empty_rather_than_failing_the_call() {
    let mut context = context();
    context.subagent_model = Some(SubagentModel::Disabled);
    let task = call("t", "Task");
    assert!(context.task_disabled(&task));
    assert_eq!(context.prepare_call(&task).unwrap().arguments["model"], "");
    // A disabled default does not disable a subagent that was routed explicitly.
    let mut named = call("t", "Task");
    named.arguments = serde_json::json!({"subagent_type": "Explore"});
    assert!(!context.task_disabled(&named));
    assert!(!context.task_disabled(&call("s", "Shell")));
}

#[tokio::test]
async fn disabling_subagents_disables_the_task_tool_entirely() {
    let mut context = context();
    context.subagents_disabled = true;
    context
        .subagent_models
        .insert("Explore".into(), SubagentModel::Model("scout".into()));
    assert!(context.task_disabled(&call("t", "Task")));
}

#[tokio::test]
async fn only_task_and_shell_are_controllable() {
    let runtime = CursorToolRuntime::default();
    let context = context();
    runtime
        .reserve_exec(&call("t", "Task"), &context)
        .await
        .unwrap();
    runtime
        .reserve_exec(&call("s", "Shell"), &context)
        .await
        .unwrap();
    runtime
        .reserve_exec(&call("r", "Read"), &context)
        .await
        .unwrap();
    let mut names = runtime
        .controllable_tools()
        .await
        .into_iter()
        .map(|tool| tool.name)
        .collect::<Vec<_>>();
    names.sort();
    assert_eq!(names, vec!["Shell", "Task"]);
}
