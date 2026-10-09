//! Owns the current Run and coordinates the Conversation lifecycle.
use std::sync::Arc;

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::{
    cursor::{
        checkpoint::CheckpointBuilder,
        compile,
        protocol::proto::agent::v1 as pb,
        services::{blob_sync::BlobSynchronizer, context_sync::RequestContextSynchronizer},
        tools::{
            codec, compat,
            runtime::CursorToolRuntime,
            tool_call_result::{tool_result_channel, ToolResultReceiver, ToolResultSender},
            ClientToolEvent, ToolDispatcher,
        },
        transport::{OrderedInbox, TransportHandle},
    },
    run::{CommandResult, RunEngine, RunHandle, RunPhase},
};

use super::{
    CompiledMessages, ConversationDependencies, ConversationOutput, ConversationOutputDependencies,
    ConversationRegistry, MessageDelivery, RunFinish, TransportCommand, TransportFinish,
};

pub struct ConversationRuntime;

const CONTINUATION_IDLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

#[derive(Clone)]
struct RunGeneration {
    id: u64,
    request: pb::AgentRunRequest,
    superseded: CancellationToken,
    finished: CancellationToken,
    run: Arc<parking_lot::Mutex<Option<RunHandle>>>,
    results: ToolResultSender,
    runtime_actions: mpsc::UnboundedSender<compile::RuntimeAction>,
    tool_runtime: CursorToolRuntime,
    tools: ToolDispatcher,
}

struct FinishGeneration(CancellationToken);

async fn preserve_interrupted_output(
    dependencies: &ConversationDependencies,
    generation: &RunGeneration,
    request_id: &str,
) {
    let run = generation.run.lock().clone();
    let Some(run) = run else {
        return;
    };
    let Some(message) = generation
        .tool_runtime
        .interrupted_output(run.run_id().as_str())
        .await
    else {
        return;
    };
    let conversation_id = crate::model::ConversationId::new(
        generation
            .request
            .conversation_id
            .clone()
            .unwrap_or_else(|| request_id.to_owned()),
    );
    let source_user = generation
        .request
        .action
        .as_ref()
        .and_then(|action| action.action.as_ref())
        .and_then(|action| match action {
            pb::conversation_action::Action::UserMessageAction(action) => {
                action.user_message.as_ref()
            }
            pb::conversation_action::Action::StartPlanAction(action) => {
                action.user_message.as_ref()
            }
            _ => None,
        });
    let Some(user) = source_user else {
        return;
    };
    if let Err(error) = dependencies
        .store
        .save_interrupted_output(
            &conversation_id,
            &message,
            run.run_id(),
            &user.message_id,
            &user.text,
        )
        .await
    {
        tracing::error!(%error, "failed to preserve interrupted tool output");
    }
}

struct TransportActorGuard(TransportHandle);

impl Drop for TransportActorGuard {
    fn drop(&mut self) {
        self.0.close_transport();
    }
}

impl Drop for FinishGeneration {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

impl ConversationRuntime {
    pub(crate) fn spawn(
        registry: ConversationRegistry,
        handle: TransportHandle,
        mut receiver: mpsc::Receiver<TransportCommand>,
    ) {
        tokio::spawn(async move {
            let _actor_guard = TransportActorGuard(handle.clone());
            let dependencies = registry.dependencies().clone();
            let blob_sync = BlobSynchronizer::new(
                handle.request_id().into(),
                dependencies.store.clone(),
                handle.clone(),
            );
            let mut inbox = OrderedInbox::starting_at(0);
            let tool_runtime_factory = CursorToolRuntime::default();
            let context_sync =
                RequestContextSynchronizer::new(handle.clone(), dependencies.store.clone());
            let mut current = None::<RunGeneration>;
            let mut next_generation = 1_u64;
            let mut pending_finish = None::<(u64, TransportFinish)>;
            let mut draining = false;
            let mut waiting_for_action = false;
            loop {
                let command = if draining {
                    if !handle.admissions_drained() {
                        tokio::select! {
                            command = receiver.recv() => match command {
                                Some(command) => command,
                                None => {
                                    finish_pending(&handle, &current, pending_finish.take());
                                    break;
                                }
                            },
                            _ = handle.wait_admissions_drained() => continue,
                        }
                    } else {
                        handle.mark_draining();
                        match receiver.try_recv() {
                            Ok(command) => command,
                            Err(mpsc::error::TryRecvError::Empty)
                            | Err(mpsc::error::TryRecvError::Disconnected) => {
                                finish_pending(&handle, &current, pending_finish.take());
                                break;
                            }
                        }
                    }
                } else if waiting_for_action {
                    tokio::select! {
                        command = receiver.recv() => match command {
                            Some(command) => command,
                            None => {
                                handle.mark_disconnected();
                                super::output::finish_success(&handle);
                                break;
                            }
                        },
                        _ = tokio::time::sleep(CONTINUATION_IDLE_TIMEOUT) => {
                            let Some(generation) = current.as_ref() else {
                                super::output::finish_success(&handle);
                                break;
                            };
                            handle.begin_close();
                            pending_finish = Some((generation.id, TransportFinish::Success));
                            draining = true;
                            waiting_for_action = false;
                            continue;
                        }
                    }
                } else {
                    match receiver.recv().await {
                        Some(command) => command,
                        None => {
                            handle.mark_disconnected();
                            if let Some(generation) = current.as_ref() {
                                preserve_interrupted_output(
                                    &dependencies,
                                    generation,
                                    handle.request_id(),
                                )
                                .await;
                                generation.superseded.cancel();
                                if let Some(run) = generation.run.lock().clone() {
                                    run.cancel();
                                }
                            }
                            super::output::finish_cancelled(&handle).ok();
                            break;
                        }
                    }
                };
                match command {
                    TransportCommand::InspectLocal { reply } => {
                        let snapshot = if let Some(generation) = current
                            .as_ref()
                            .filter(|g| !g.finished.is_cancelled() && !g.superseded.is_cancelled())
                        {
                            let run_id = generation
                                .run
                                .lock()
                                .as_ref()
                                .map(|r| r.run_id().to_string());
                            match run_id {
                                Some(run_id) => Some(super::local_control::ActiveLocalRun {
                                    request_id: handle.request_id().into(),
                                    run_id,
                                    conversation_id: handle
                                        .conversation_id()
                                        .unwrap_or_default()
                                        .into(),
                                    parent_request_id: handle
                                        .parent()
                                        .map(|parent| parent.request_id.clone()),
                                    tools: generation.tool_runtime.active_local_tools().await,
                                }),
                                None => None,
                            }
                        } else {
                            None
                        };
                        let _ = reply.send(snapshot);
                    }
                    TransportCommand::LocalControl { request, reply } => {
                        if reply.is_closed() {
                            continue;
                        }
                        let active = current
                            .as_ref()
                            .filter(|generation| {
                                !generation.finished.is_cancelled()
                                    && !generation.superseded.is_cancelled()
                            })
                            .filter(|generation| {
                                generation
                                    .run
                                    .lock()
                                    .as_ref()
                                    .is_some_and(|run| run.run_id().as_str() == request.run_id)
                            });
                        let result = match active {
                            Some(generation)
                                if matches!(
                                    request.action,
                                    super::local_control::ControlAction::StopRun
                                ) =>
                            {
                                handle.mark_user_stop();
                                preserve_interrupted_output(
                                    &dependencies,
                                    generation,
                                    handle.request_id(),
                                )
                                .await;
                                generation.superseded.cancel();
                                if let Some(run) = generation.run.lock().clone() {
                                    run.cancel();
                                }
                                for id in generation.tool_runtime.drain_running().await {
                                    let _ = handle.emit(&codec::abort(id));
                                }
                                Ok(super::local_control::ControlResponse {
                                    status: "run_stop_requested",
                                })
                            }
                            Some(generation) => {
                                super::local_control::apply(
                                    &generation.tool_runtime,
                                    &handle,
                                    &request,
                                )
                                .await
                            }
                            None => Err(crate::Error::RunNotFound(
                                "Control target is stale or has ended".into(),
                            )),
                        };
                        let _ = reply.send(result);
                    }
                    TransportCommand::Disconnect => {
                        handle.mark_disconnected();
                        if let Some(generation) = current.as_ref() {
                            preserve_interrupted_output(
                                &dependencies,
                                generation,
                                handle.request_id(),
                            )
                            .await;
                            generation.superseded.cancel();
                            if let Some(run) = generation.run.lock().clone() {
                                run.cancel();
                            }
                            for id in generation.tool_runtime.drain_running().await {
                                let _ = handle.emit(&codec::abort(id));
                            }
                        }
                        let turn_completed = waiting_for_action
                            || current.as_ref().is_some_and(|generation| {
                                generation
                                    .run
                                    .lock()
                                    .as_ref()
                                    .is_none_or(|run| run.phase() != RunPhase::Running)
                            });
                        if turn_completed {
                            super::output::finish_success(&handle);
                        } else {
                            super::output::finish_cancelled(&handle).ok();
                        }
                        break;
                    }
                    TransportCommand::RunFinished { generation, finish } => {
                        if current
                            .as_ref()
                            .is_none_or(|current| current.id != generation)
                        {
                            continue;
                        }
                        match finish {
                            RunFinish::TurnCompleted => {
                                pending_finish = None;
                                draining = false;
                                waiting_for_action = true;
                            }
                            RunFinish::Transport(finish) => {
                                waiting_for_action = false;
                                handle.begin_close();
                                pending_finish = Some((generation, finish));
                                draining = true;
                            }
                        }
                    }
                    TransportCommand::Append { seqno, message } => {
                        for (_seqno, message) in inbox.push(seqno, *message) {
                            {
                                match message.message {
                                    Some(pb::agent_client_message::Message::RunRequest(
                                        request,
                                    )) => {
                                        if matches!(request.action.as_ref().and_then(|action| action.action.as_ref()),
                                            Some(pb::conversation_action::Action::BackgroundTaskCompletionAction(action))
                                                if background_progress_only(action))
                                        {
                                            if let Some(id) = request.conversation_id.as_deref() {
                                                if let Err(error) = handle.set_conversation_id(id) {
                                                    let _ = super::output::finish_failed(&handle, &error);
                                                    return;
                                                }
                                            }
                                            // A progress-only wakeup has no turn to run. On an
                                            // existing stream preserve its generation and closing
                                            // state; on a fresh stream acknowledge without a model.
                                            if current.is_none() {
                                                super::output::finish_success(&handle);
                                            }
                                            continue;
                                        }
                                        waiting_for_action = false;
                                        if draining {
                                            handle.reopen();
                                            draining = false;
                                            pending_finish = None;
                                        }
                                        if let Some(conversation_id) =
                                            request.conversation_id.as_deref()
                                        {
                                            if let Err(error) =
                                                handle.set_conversation_id(conversation_id)
                                            {
                                                tracing::error!(
                                                    request_id = handle.request_id(),
                                                    %error,
                                                    "invalid Cursor conversation id"
                                                );
                                                let _ = super::output::finish_failed(&handle, &error);
                                                return;
                                            }
                                        }
                                        start_generation(
                                            &registry,
                                            &handle,
                                            &dependencies,
                                            &blob_sync,
                                            &context_sync,
                                            &tool_runtime_factory,
                                            &mut current,
                                            &mut next_generation,
                                            request,
                                        )
                                        .await;
                                    }
                                    Some(pb::agent_client_message::Message::ExecClientMessage(
                                        message,
                                    )) => {
                                        if context_sync.handle_client(&message).await {
                                            continue;
                                        }
                                        let Some(generation) = current.as_ref() else {
                                            continue;
                                        };
                                        match codec::client_event(
                                            &message,
                                            &generation.tool_runtime,
                                        )
                                        .await
                                        {
                                            Ok(codec::ClientExecEvent::Delta(message)) => {
                                                let _ = handle.emit(&message);
                                            }
                                            Ok(codec::ClientExecEvent::Message(message)) => {
                                                let _ = handle.emit(&message);
                                            }
                                            Ok(codec::ClientExecEvent::Completed(result)) => {
                                                generation.results.send(*result)
                                            }
                                            Ok(codec::ClientExecEvent::Pending) => {}
                                            Err(error) => generation.results.send_error(error),
                                        }
                                    }
                                    Some(
                                        pb::agent_client_message::Message::ExecClientControlMessage(
                                            message,
                                        ),
                                    ) => {
                                        use pb::exec_client_control_message::Message;
                                        match message.message {
                                            Some(Message::StreamClose(close)) => {
                                                if context_sync.handle_stream_close(close.id).await
                                                {
                                                    continue;
                                                }
                                                let Some(generation) = current.as_ref() else {
                                                    continue;
                                                };
                                                match codec::stream_closed(
                                                    close.id,
                                                    &generation.tool_runtime,
                                                )
                                                .await
                                                {
                                                    Ok(Some(completion)) => {
                                                        generation.results.send(completion)
                                                    }
                                                    Ok(None) => {}
                                                    Err(error) => {
                                                        generation.results.send_error(error)
                                                    }
                                                }
                                            }
                                            Some(Message::Throw(throw)) => {
                                                if context_sync
                                                    .handle_throw(
                                                        throw.id,
                                                        format!(
                                                            "Cursor request context failed: {}",
                                                            throw.error
                                                        ),
                                                    )
                                                    .await
                                                {
                                                    continue;
                                                }
                                                let Some(generation) = current.as_ref() else {
                                                    continue;
                                                };
                                                if generation.tool_runtime.is_background_control(throw.id).await {
                                                    tracing::warn!(id = throw.id, error = %throw.error, "Cursor background Shell control failed; original Shell remains active");
                                                    continue;
                                                }
                                                if generation
                                                    .tool_runtime
                                                    .is_interrupted(throw.id)
                                                    .await
                                                {
                                                    generation
                                                        .tool_runtime
                                                        .discard_exec(throw.id)
                                                        .await;
                                                    continue;
                                                }
                                                match generation
                                                    .tool_runtime
                                                    .take_exec(throw.id)
                                                    .await
                                                {
                                                    Some(pending) => generation.results.send(
                                                        compat::failure_with_message(
                                                            &pending.call,
                                                            format!(
                                                                "Exec {} failed: {}",
                                                                pending.call.call_id, throw.error
                                                            ),
                                                        ),
                                                    ),
                                                    None => tracing::warn!(
                                                        id = throw.id,
                                                        "ignoring failure for unknown tool execution"
                                                    ),
                                                }
                                            }
                                            Some(Message::Heartbeat(_)) | None => {}
                                        }
                                    }
                                    Some(
                                        pb::agent_client_message::Message::InteractionResponse(
                                            message,
                                        ),
                                    ) => {
                                        let Some(generation) = current.as_ref() else {
                                            continue;
                                        };
                                        match generation.tools.interaction_response(&message).await
                                        {
                                            Ok(ClientToolEvent::Completed(completion)) => {
                                                generation.results.send(*completion)
                                            }
                                            Ok(ClientToolEvent::Pending) => {}
                                            Err(error) => generation.results.send_error(error),
                                        }
                                    }
                                    Some(pb::agent_client_message::Message::KvClientMessage(
                                        message,
                                    )) => {
                                        let _ = blob_sync.handle_client(message).await;
                                    }
                                    // ConversationAction has two delivery paths that must not be
                                    // conflated:
                                    //    it: UserMessage, BackgroundTaskCompletion, Summarize, ExecutePlan,
                                    //    StartPlan, AsyncAskQuestionCompletion, GoalContinuation, Resume and
                                    //    SubscriptionNotification.
                                    // 2. AgentClientMessage::ConversationAction arrives on an already active
                                    //    Bidi Run and is dispatched below. An action being projected in path 1
                                    //    does not mean this path supports it.
                                    //
                                    // Cursor 3.16 sends a queued follow-up as InjectContextAction, targeting
                                    // expected_run_id. The session owns that path because an interruption must
                                    // abort active execs and publish a recoverable checkpoint before the old
                                    // Run ends; it is not handle.cancel().
                                    //
                                    // Variants whose wire behaviour is not yet captured are logged rather than
                                    // given invented semantics. Logging one is not evidence that the requested
                                    // operation ran.
                                    Some(
                                        pb::agent_client_message::Message::ConversationAction(
                                            conversation_action,
                                        ),
                                    ) => match conversation_action.action.clone() {
                                        Some(
                                            pb::conversation_action::Action::UserMessageAction(
                                                action,
                                            ),
                                        ) => {
                                            let delivered_to_active_run =
                                                current.as_ref().is_some_and(|generation| {
                                                    generation.run.lock().as_ref().is_some_and(
                                                        |run| run.phase() == RunPhase::Running,
                                                    ) && generation
                                                        .runtime_actions
                                                        .send(compile::RuntimeAction::UserMessage(
                                                            action.clone(),
                                                        ))
                                                        .is_ok()
                                                });
                                            if delivered_to_active_run {
                                                continue;
                                            }
                                            let Some(previous) = current.as_ref() else {
                                                continue;
                                            };
                                            let mut request = previous.request.clone();
                                            request.action = Some(conversation_action);
                                            request.conversation_state = None;
                                            request.pre_fetched_blobs.clear();
                                            waiting_for_action = false;
                                            start_generation(
                                                &registry,
                                                &handle,
                                                &dependencies,
                                                &blob_sync,
                                                &context_sync,
                                                &tool_runtime_factory,
                                                &mut current,
                                                &mut next_generation,
                                                request,
                                            )
                                            .await;
                                        }
                                        Some(pb::conversation_action::Action::BackgroundTaskCompletionAction(ref action))
                                            if background_progress_only(action) => {
                                            // Progress is informational, even when the previous
                                            // run has ended. It must not start a new generation.
                                        }
                                        Some(pb::conversation_action::Action::BackgroundTaskCompletionAction(ref action))
                                            if current.as_ref().is_some_and(|generation| {
                                                generation.run.lock().as_ref().is_some_and(|run| run.phase() == RunPhase::Running)
                                            }) => {
                                            if let Some(generation) = current.as_ref() {
                                                if generation.runtime_actions.send(compile::RuntimeAction::BackgroundCompletion(action.clone())).is_err() {
                                                    generation.results.send_error(crate::Error::Protocol(
                                                        "background completion arrived without an active runtime listener".into()
                                                    ));
                                                }
                                            }
                                        }
                                        Some(
                                            pb::conversation_action::Action::StartPlanAction(_)
                                            | pb::conversation_action::Action::ExecutePlanAction(_)
                                            | pb::conversation_action::Action::BackgroundTaskCompletionAction(_)
                                            | pb::conversation_action::Action::AsyncAskQuestionCompletionAction(_)
                                            | pb::conversation_action::Action::GoalContinuationAction(_),
                                        ) => {
                                            let Some(previous) = current.as_ref() else {
                                                continue;
                                            };
                                            let mut request = previous.request.clone();
                                            request.action = Some(conversation_action);
                                            request.conversation_state = None;
                                            request.pre_fetched_blobs.clear();
                                            waiting_for_action = false;
                                            start_generation(
                                                &registry,
                                                &handle,
                                                &dependencies,
                                                &blob_sync,
                                                &context_sync,
                                                &tool_runtime_factory,
                                                &mut current,
                                                &mut next_generation,
                                                request,
                                            )
                                            .await;
                                        }
                                        Some(pb::conversation_action::Action::ResumeAction(_)) => {
                                            if waiting_for_action {
                                                if let Some(previous) = current.as_ref() {
                                                    let mut request = previous.request.clone();
                                                    request.action = Some(conversation_action);
                                                    request.conversation_state = None;
                                                    request.pre_fetched_blobs.clear();
                                                    waiting_for_action = false;
                                                    start_generation(
                                                        &registry,
                                                        &handle,
                                                        &dependencies,
                                                        &blob_sync,
                                                        &context_sync,
                                                        &tool_runtime_factory,
                                                        &mut current,
                                                        &mut next_generation,
                                                        request,
                                                    )
                                                    .await;
                                                }
                                            }
                                        }
                                        Some(pb::conversation_action::Action::CancelAction(_)) => {
                                            if let Some(generation) = current.as_ref() {
                                                preserve_interrupted_output(&dependencies, generation, handle.request_id()).await;
                                                if let Some(run) = generation.run.lock().clone() {
                                                    run.cancel();
                                                }
                                                for id in
                                                    generation.tool_runtime.drain_running().await
                                                {
                                                    let _ = handle.emit(&codec::abort(id));
                                                }
                                            }
                                        }
                                        Some(
                                            pb::conversation_action::Action::InjectContextAction(
                                                action,
                                            ),
                                        ) => {
                                            let Some(generation) = current.as_ref() else {
                                                continue;
                                            };
                                            if generation
                                                .runtime_actions
                                                .send(compile::RuntimeAction::Inject(action))
                                                .is_err()
                                            {
                                                generation.results.send_error(crate::Error::Protocol(
                                                    "InjectContextAction arrived without an active Run"
                                                        .into(),
                                                ));
                                            }
                                        }
                                        Some(
                                            pb::conversation_action::Action::CancelSubagentAction(
                                                action,
                                            ),
                                        ) => {
                                            if let Some(generation) = current.as_ref() {
                                                if let Some(id) = generation
                                                    .tool_runtime
                                                    .running_task_exec_id(&action.subagent_id)
                                                    .await
                                                {
                                                    let _ = handle.emit(&codec::abort(id));
                                                }
                                            }
                                        }
                                        Some(pb::conversation_action::Action::BackgroundShellAction(action)) => {
                                            if let Some(generation) = current.as_ref() {
                                                match generation.tool_runtime.background_shell_request(&action.tool_call_id).await {
                                                    Ok(Some(message)) => { let _ = handle.emit(&message); }
                                                    Ok(None) => tracing::debug!(tool_call_id = %action.tool_call_id, "background Shell target absent or already requested"),
                                                    Err(error) => tracing::warn!(%error, "background Shell control rejected"),
                                                }
                                            }
                                        }
                                        Some(pb::conversation_action::Action::BackgroundSubagentAction(action)) => {
                                            if let Some(generation) = current.as_ref() {
                                                match generation.tool_runtime.background_request(&action.tool_call_id, true).await {
                                                    Ok(Some(message)) => { let _ = handle.emit(&message); }
                                                    Ok(None) => tracing::debug!(tool_call_id = %action.tool_call_id, "background Task queued or already requested"),
                                                    Err(error) => tracing::warn!(%error, "background Task control rejected"),
                                                }
                                            }
                                        }
                                        Some(action) => {
                                            tracing::warn!(
                                                request_id = handle.request_id(),
                                                action = runtime_action_name(&action),
                                                "ignoring unsupported runtime ConversationAction"
                                            );
                                        }
                                        None => {
                                            if let Some(generation) = current.as_ref() {
                                                generation.results.send_error(
                                                    crate::Error::Protocol(
                                                        "runtime ConversationAction has no action"
                                                            .into(),
                                                    ),
                                                );
                                            }
                                        }
                                    },
                                    _ => {}
                                }
                            }
                        }
                    }
                }
            }
        });
    }
}

#[allow(clippy::too_many_arguments)]
async fn start_generation(
    registry: &ConversationRegistry,
    handle: &TransportHandle,
    dependencies: &ConversationDependencies,
    blob_sync: &BlobSynchronizer,
    context_sync: &RequestContextSynchronizer,
    tool_runtime_factory: &CursorToolRuntime,
    current: &mut Option<RunGeneration>,
    next_generation: &mut u64,
    mut request: pb::AgentRunRequest,
) {
    if handle.parent().is_some() {
        normalize_subagent_message_id(handle.request_id(), &mut request);
    }
    let previous_finished = if let Some(previous) = current.take() {
        previous.superseded.cancel();
        if let Some(run) = previous.run.lock().clone() {
            run.cancel();
        }
        for id in previous.tool_runtime.interrupt_for_run_replacement().await {
            let _ = handle.emit(&codec::abort(id));
        }
        Some(previous.finished.clone())
    } else {
        None
    };
    let (results, result_receiver) = tool_result_channel();
    let (runtime_actions, runtime_action_receiver) =
        mpsc::unbounded_channel::<compile::RuntimeAction>();
    let tool_runtime = tool_runtime_factory.next_run();
    let tools = ToolDispatcher::with_results(
        tool_runtime.clone(),
        results.clone(),
        dependencies.store.clone(),
        dependencies.web_cache.clone(),
    )
    .with_image_generator(dependencies.plugins.clone());
    let generation = RunGeneration {
        id: *next_generation,
        request: request.clone(),
        superseded: CancellationToken::new(),
        finished: CancellationToken::new(),
        run: Arc::new(parking_lot::Mutex::new(None)),
        results,
        runtime_actions,
        tool_runtime,
        tools,
    };
    *next_generation = next_generation.saturating_add(1);
    *current = Some(generation.clone());
    spawn_run_request(
        registry.clone(),
        handle.clone(),
        request,
        dependencies.clone(),
        blob_sync.clone(),
        context_sync.clone(),
        generation,
        previous_finished,
        result_receiver,
        runtime_action_receiver,
    );
}

fn background_progress_only(action: &pb::BackgroundTaskCompletionAction) -> bool {
    !action.completions.is_empty()
        && action.completions.iter().all(|completion| {
            completion.reason == pb::BackgroundTaskCompletionReason::TaskProgress as i32
        })
}

fn normalize_subagent_message_id(request_id: &str, request: &mut pb::AgentRunRequest) {
    use prost::Message;
    use sha2::{Digest, Sha256};
    let Some(pb::conversation_action::Action::UserMessageAction(action)) = request
        .action
        .as_mut()
        .and_then(|action| action.action.as_mut())
    else {
        return;
    };
    if let Some(user) = action
        .user_message
        .as_mut()
        .filter(|user| user.message_id.is_empty())
    {
        // Cursor creates subagent seed messages without an ID. Stable across
        // transport retries, distinct for different prompts on the same request.
        let digest = Sha256::digest(user.encode_to_vec());
        user.message_id = format!("subagent:{request_id}:{}", hex::encode(&digest[..16]));
    }
}

fn finish_pending(
    handle: &TransportHandle,
    current: &Option<RunGeneration>,
    pending: Option<(u64, TransportFinish)>,
) {
    let Some((generation, finish)) = pending else {
        return;
    };
    if current
        .as_ref()
        .is_some_and(|current| current.id == generation)
    {
        finish_transport(handle, finish);
    }
}

fn finish_transport(handle: &TransportHandle, finish: TransportFinish) {
    match finish {
        TransportFinish::Success => super::output::finish_success(handle),
        TransportFinish::Failed(error) => {
            let _ = super::output::finish_failed(handle, &error);
        }
        TransportFinish::Cancelled => {
            let _ = super::output::finish_cancelled(handle);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn spawn_run_request(
    registry: ConversationRegistry,
    handle: TransportHandle,
    request: pb::AgentRunRequest,
    dependencies: ConversationDependencies,
    blob_sync: BlobSynchronizer,
    context_sync: RequestContextSynchronizer,
    generation: RunGeneration,
    previous_finished: Option<CancellationToken>,
    results: ToolResultReceiver,
    runtime_actions: mpsc::UnboundedReceiver<compile::RuntimeAction>,
) {
    tokio::spawn(async move {
        let _finished = FinishGeneration(generation.finished.clone());
        if let Some(previous_finished) = previous_finished {
            tokio::select! {
                biased;
                _ = generation.superseded.cancelled() => return,
                _ = previous_finished.cancelled() => {}
            }
        }
        if generation.superseded.is_cancelled() {
            return;
        }

        let mut checkpoint = CheckpointBuilder::new(
            dependencies.store.clone(),
            blob_sync.clone(),
            handle
                .parent()
                .map(|parent| parent.tool_call_id.clone())
                .filter(|id| !id.is_empty()),
            request.conversation_state.clone(),
        );
        let prepared = tokio::select! {
            biased;
            _ = generation.superseded.cancelled() => return,
            prepared = compile::prepare(
                handle.request_id(),
                &request,
                compile::PrepareDependencies {
                    plugins: dependencies.plugins.as_ref(),
                    compiler: &dependencies.compiler,
                    store: &dependencies.store,
                    checkpoint: &checkpoint,
                    blob_sync: &blob_sync,
                    context_sync: &context_sync,
                    local_rules_dir: dependencies.local_rules_dir.as_deref(),
                },
            ) => prepared,
        };
        let (mut prepared, context) = match prepared {
            Ok(prepared) => prepared,
            Err(error) => {
                if generation.superseded.is_cancelled() {
                    return;
                }
                tracing::error!(
                    request_id = handle.request_id(),
                    %error,
                    "failed to prepare Cursor Run"
                );
                let _ = handle
                    .command(TransportCommand::RunFinished {
                        generation: generation.id,
                        finish: RunFinish::Transport(TransportFinish::Failed(error)),
                    })
                    .await;
                return;
            }
        };
        if matches!(
            request
                .action
                .as_ref()
                .and_then(|action| action.action.as_ref()),
            Some(pb::conversation_action::Action::SubscriptionNotificationAction(_))
        ) && prepared.initial_messages.is_empty()
        {
            let _ = handle
                .command(TransportCommand::RunFinished {
                    generation: generation.id,
                    finish: RunFinish::Transport(TransportFinish::Success),
                })
                .await;
            return;
        }
        checkpoint.configure(
            prepared.model.model_id.clone(),
            prepared.model.context_window_tokens,
            context.checkpoint_prompt.instructions.clone(),
            context.checkpoint_prompt.tools.clone(),
            context.dynamic_tools.keys().cloned().collect(),
            context.turn_user.clone(),
        );
        if context.background_completion {
            let event_id = prepared
                .initial_messages
                .first()
                .and_then(|message| message.runtime_event_id.clone())
                .unwrap_or_else(|| format!("background:{}", prepared.run_id));
            match registry
                .deliver(
                    &prepared.conversation_id,
                    CompiledMessages {
                        event_id,
                        target_run_id: None,
                        messages: prepared.initial_messages.clone(),
                        delivery: MessageDelivery::InsertMessages,
                    },
                )
                .await
            {
                CommandResult::Applied | CommandResult::Duplicate => {
                    if !generation.superseded.is_cancelled() {
                        let _ = handle
                            .command(TransportCommand::RunFinished {
                                generation: generation.id,
                                finish: RunFinish::Transport(TransportFinish::Success),
                            })
                            .await;
                    }
                    return;
                }
                CommandResult::RunClosing => {
                    prepared.initial_messages.clear();
                    tokio::select! {
                        biased;
                        _ = generation.superseded.cancelled() => return,
                        _ = registry.wait_until_idle(&prepared.conversation_id) => {}
                    }
                    if let Ok(checkpoint) = dependencies
                        .store
                        .ensure_conversation(&prepared.conversation_id)
                        .await
                    {
                        prepared.base_checkpoint_id = checkpoint;
                    }
                }
                CommandResult::RunEnded => {
                    prepared.initial_messages.clear();
                }
                CommandResult::StaleTarget => {
                    if !generation.superseded.is_cancelled() {
                        let _ = handle
                            .command(TransportCommand::RunFinished {
                                generation: generation.id,
                                finish: RunFinish::Transport(TransportFinish::Success),
                            })
                            .await;
                    }
                    return;
                }
            }
        }
        let pending = registry.take_pending(&prepared.conversation_id).await;
        if !pending.is_empty() {
            let mut messages = pending
                .into_iter()
                .flat_map(|pending| pending.messages)
                .collect::<Vec<_>>();
            messages.extend(prepared.initial_messages);
            prepared.initial_messages = messages;
            if let Ok(checkpoint) = dependencies
                .store
                .ensure_conversation(&prepared.conversation_id)
                .await
            {
                prepared.base_checkpoint_id = checkpoint;
            }
        }
        if generation.superseded.is_cancelled() {
            return;
        }

        let run_id = prepared.run_id.clone();
        let conversation_id = prepared.conversation_id.clone();
        let (port, core, run_handle) = crate::run::channel(run_id.clone(), 256);
        *generation.run.lock() = Some(run_handle.clone());
        if generation.superseded.is_cancelled() {
            run_handle.cancel();
            *generation.run.lock() = None;
            return;
        }
        registry
            .activate(conversation_id.clone(), run_id.clone(), run_handle.clone())
            .await;
        let cancellation = run_handle.cancellation();
        let engine = RunEngine::new(dependencies.store.clone(), dependencies.provider.clone());
        let core_run = tokio::spawn(async move { engine.run(prepared, port, cancellation).await });
        let output = ConversationOutput::new(
            handle.clone(),
            dependencies.store.clone(),
            context,
            core,
            run_handle,
            registry.clone(),
            ConversationOutputDependencies {
                superseded: generation.superseded.clone(),
                tools: generation.tools.clone(),
                results,
                runtime_actions,
                compiler: dependencies.compiler.clone(),
                blob_sync,
                checkpoint,
                tool_runtime: generation.tool_runtime.clone(),
            },
        );
        let finish = match output.run().await {
            Ok(finish) => finish,
            Err(error) => {
                if generation.superseded.is_cancelled() {
                    RunFinish::Transport(TransportFinish::Cancelled)
                } else {
                    tracing::error!(
                        request_id = handle.request_id(),
                        %error,
                        "Cursor session failed"
                    );
                    RunFinish::Transport(TransportFinish::Failed(error))
                }
            }
        };
        let _ = core_run.await;
        registry.release(&conversation_id, &run_id).await;
        if generation
            .run
            .lock()
            .as_ref()
            .is_some_and(|run| run.run_id() == &run_id)
        {
            *generation.run.lock() = None;
        }
        // An explicit Nexusor stop has no replacement generation to own stream
        // closure. It must still deliver the terminal cancellation to Cursor.
        if !generation.superseded.is_cancelled() || handle.user_stopped() {
            let _ = handle
                .command(TransportCommand::RunFinished {
                    generation: generation.id,
                    finish,
                })
                .await;
        }
    });
}

fn runtime_action_name(action: &pb::conversation_action::Action) -> &'static str {
    use pb::conversation_action::Action;

    match action {
        Action::UserMessageAction(_) => "UserMessageAction",
        Action::ResumeAction(_) => "ResumeAction",
        Action::CancelAction(_) => "CancelAction",
        Action::SummarizeAction(_) => "SummarizeAction",
        Action::ShellCommandAction(_) => "ShellCommandAction",
        Action::StartPlanAction(_) => "StartPlanAction",
        Action::ExecutePlanAction(_) => "ExecutePlanAction",
        Action::AsyncAskQuestionCompletionAction(_) => "AsyncAskQuestionCompletionAction",
        Action::CancelSubagentAction(_) => "CancelSubagentAction",
        Action::BackgroundTaskCompletionAction(_) => "BackgroundTaskCompletionAction",
        Action::BackgroundShellAction(_) => "BackgroundShellAction",
        Action::BackgroundSubagentAction(_) => "BackgroundSubagentAction",
        Action::SubscriptionNotificationAction(_) => "SubscriptionNotificationAction",
        Action::GoalContinuationAction(_) => "GoalContinuationAction",
        Action::InjectContextAction(_) => "InjectContextAction",
    }
}

#[cfg(test)]
mod subagent_message_tests {
    use super::*;
    #[test]
    fn only_explicit_progress_batches_skip_generation() {
        let progress = pb::BackgroundTaskCompletion {
            reason: pb::BackgroundTaskCompletionReason::TaskProgress as i32,
            ..Default::default()
        };
        let mut batch = pb::BackgroundTaskCompletionAction {
            completions: vec![progress],
        };
        assert!(background_progress_only(&batch));
        batch.completions.push(pb::BackgroundTaskCompletion {
            reason: pb::BackgroundTaskCompletionReason::TaskFinished as i32,
            ..Default::default()
        });
        assert!(!background_progress_only(&batch));
        assert!(!background_progress_only(
            &pb::BackgroundTaskCompletionAction::default()
        ));
        batch.completions = vec![pb::BackgroundTaskCompletion::default()];
        assert!(!background_progress_only(&batch));
    }
    #[test]
    fn missing_subagent_seed_id_is_stable_and_existing_ids_are_preserved() {
        let mut request = pb::AgentRunRequest {
            action: Some(pb::ConversationAction {
                action: Some(pb::conversation_action::Action::UserMessageAction(
                    pb::UserMessageAction {
                        user_message: Some(pb::UserMessage {
                            text: "inspect".into(),
                            ..Default::default()
                        }),
                        ..Default::default()
                    },
                )),
                ..Default::default()
            }),
            ..Default::default()
        };
        let mut retry = request.clone();
        normalize_subagent_message_id("child", &mut request);
        normalize_subagent_message_id("child", &mut retry);
        assert_eq!(request, retry);
        normalize_subagent_message_id("other", &mut retry);
        assert_eq!(request, retry);
        let pb::conversation_action::Action::UserMessageAction(action) =
            request.action.unwrap().action.unwrap()
        else {
            panic!()
        };
        assert!(action
            .user_message
            .unwrap()
            .message_id
            .starts_with("subagent:child:"));
    }
}
