//! Runs the tool calls Cursor dispatches, and tracks their lifecycle.
//!
//! `CursorToolRuntime` owns every in-flight exec and interaction. One submodule per
//! concern: reserving a call, MCP discovery, buffering executor output, interruption
//! bookkeeping, and moving a running tool into the background.

mod background;
mod interruption;
mod mcp;
mod output;
mod reservation;
#[cfg(test)]
mod tests;

use std::{
    collections::{HashMap, HashSet},
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc,
    },
};

use tokio::sync::{oneshot, Mutex};
use tokio_util::sync::CancellationToken;

use crate::{cursor::protocol::proto::agent::v1 as pb, model::ToolCall, Error, Result};

use super::edit::EditWrite;

/// How a reserved exec will be executed once its result arrives.
#[derive(Clone, Debug)]
pub(crate) enum ExecStage {
    Direct,
    DynamicMcp(pb::McpToolDefinition),
    EditRead,
    EditWrite(EditWrite),
}

/// An exec awaiting its result from Cursor's executor.
#[derive(Clone, Debug)]
pub(crate) struct PendingExec {
    pub call: ToolCall,
    pub context: ExecContext,
    pub started_at_ms: u64,
    pub stdout: String,
    pub stderr: String,
    pub hook_contexts: Vec<pb::HookAdditionalContext>,
    pub stage: ExecStage,
}

/// An interaction awaiting the user's answer.
#[derive(Clone, Debug)]
pub(crate) struct PendingInteraction {
    pub call: ToolCall,
    pub started_at_ms: u64,
}

/// An in-flight image Read/Write awaiting Cursor's response.
struct ImageIoPending {
    /// The exec id Cursor echoes back on the client message.
    pub id: u32,
    pub reply: Option<oneshot::Sender<Result<pb::exec_client_message::Message>>>,
}

/// A tool call the local task-control panel can still act on.
pub struct ControllableTool {
    pub call_id: String,
    pub name: String,
}

/// The MCP tools a server exposed, keyed by server then tool name.
#[derive(Clone, Debug, Default)]
pub(crate) struct McpDiscovery {
    pub servers: HashMap<String, HashMap<String, McpRoute>>,
    /// Set when a discovery run covered every server, so an absent route is final.
    pub complete: bool,
}

/// An MCP tool as Cursor needs to see it.
#[derive(Clone, Debug, Default)]
pub struct McpRoute {
    pub name: String,
    pub provider_identifier: String,
    pub tool_name: String,
    pub description: String,
}

/// Which model a subagent run should use.
#[derive(Clone, Debug, PartialEq)]
pub enum SubagentModel {
    Model(String),
    Disabled,
}

/// Everything an exec needs that is not part of the call itself.
#[derive(Clone, Debug, Default)]
pub struct ExecContext {
    pub conversation_id: String,
    pub root_conversation_id: String,
    pub default_subagent_model: String,
    pub subagent_model: Option<SubagentModel>,
    pub subagent_models: HashMap<String, SubagentModel>,
    pub allow_subagents: bool,
    pub subagents_disabled: bool,
    pub subagent_write_access: bool,
    pub terminals_folder: String,
    pub admin_command_denylist: Vec<String>,
    pub mcp_routes: HashMap<(String, String), McpRoute>,
}

impl ExecContext {
    /// Fills in a Task's `model` from this run's routing.
    ///
    /// The model is chosen per subagent type first, then the run-wide default. An
    /// explicit model in the call always wins, and `Disabled` is reported separately
    /// so the caller can render the disabled state rather than failing.
    pub fn prepare_call(&self, call: &ToolCall) -> Result<ToolCall> {
        let mut call = call.clone();
        if !call.name.eq_ignore_ascii_case("Task") {
            return Ok(call);
        }
        let requested = call
            .arguments
            .get("model")
            .and_then(serde_json::Value::as_str)
            .filter(|model| !model.trim().is_empty())
            .map(str::to_owned);
        if requested.is_some() {
            return Ok(call);
        }
        let subagent_type = call
            .arguments
            .get("subagent_type")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        let selection = if subagent_type.is_empty() {
            self.subagent_model.clone()
        } else {
            self.subagent_models.get(subagent_type).cloned()
        };
        let model = match selection {
            Some(SubagentModel::Model(model)) => model,
            // No route for this subagent type: the run's default applies, which is the
            // parent model when the user configured no subagent route at all.
            None => self.default_subagent_model.clone(),
            // Left empty so `task_disabled` reports it and Cursor renders the state.
            Some(SubagentModel::Disabled) => String::new(),
        };
        let object = call
            .arguments
            .as_object_mut()
            .ok_or_else(|| Error::Protocol("Task arguments must be an object".into()))?;
        object.insert("model".into(), serde_json::Value::String(model));
        Ok(call)
    }

    /// Whether this run's routing disables the Task tool entirely.
    pub fn task_disabled(&self, call: &ToolCall) -> bool {
        if self.subagents_disabled {
            return true;
        }
        if !call.name.eq_ignore_ascii_case("Task") {
            return false;
        }
        let subagent_type = call
            .arguments
            .get("subagent_type")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        let selection = if subagent_type.is_empty() {
            self.subagent_model.clone()
        } else {
            self.subagent_models.get(subagent_type).cloned()
        };
        matches!(selection, Some(SubagentModel::Disabled))
    }
}

#[derive(Clone, Default)]
pub struct CursorToolRuntime {
    /// Shared with `next_run` so a replacement run keeps interrupted-exec history.
    next_id: Arc<AtomicU32>,
    execs: Arc<Mutex<HashMap<u32, PendingExec>>>,
    interactions: Arc<Mutex<HashMap<u32, PendingInteraction>>>,
    completed: Arc<Mutex<HashMap<u32, String>>>,
    interrupted: Arc<Mutex<HashSet<u32>>>,
    /// Exec id -> the call it moves to the background.
    background_controls: Arc<Mutex<HashMap<u32, String>>>,
    /// call_id -> whether it is a Task. Remembered for a request that arrives before
    /// the tool starts.
    background_intents: Arc<Mutex<HashMap<String, bool>>>,
    /// Exec ids whose start message has gone out to Cursor.
    started_tools: Arc<Mutex<HashSet<u32>>>,
    discovered_mcp: Arc<Mutex<McpDiscovery>>,
    /// call_id -> the image Read/Write exchanges in flight for it.
    image_operations: Arc<Mutex<HashMap<String, Vec<ImageIoPending>>>>,
    /// call_id -> the scope token, so an interrupt during the generation gap (before
    /// any exchange is issued) can still cancel it.
    image_scopes: Arc<Mutex<HashMap<String, CancellationToken>>>,
    /// Cancelled when the run is replaced, so orphaned work stops.
    pub(crate) local_cancellation: CancellationToken,
}

impl CursorToolRuntime {
    /// Tools this run can still act on: a Task while it runs, a Shell while it is
    /// still executing.
    pub async fn controllable_tools(&self) -> Vec<ControllableTool> {
        let execs = self.execs.lock().await;
        execs
            .values()
            .filter(|entry| {
                entry.call.name.eq_ignore_ascii_case("Task") || is_shell(&entry.call.name)
            })
            .map(|entry| ControllableTool {
                call_id: entry.call.call_id.clone(),
                name: entry.call.name.clone(),
            })
            .collect()
    }

    /// The controllable tools, shaped for the console API.
    pub async fn active_local_tools(
        &self,
    ) -> Vec<crate::cursor::conversation::local_control::ActiveLocalTool> {
        self.controllable_tools()
            .await
            .into_iter()
            .map(
                |tool| crate::cursor::conversation::local_control::ActiveLocalTool {
                    call_id: tool.call_id,
                    name: tool.name,
                },
            )
            .collect()
    }

    /// The exec id a tool call is currently running under, if any.
    pub async fn controllable_exec_id(&self, call_id: &str) -> Option<u32> {
        let execs = self.execs.lock().await;
        execs
            .iter()
            .find(|(_, entry)| entry.call.call_id == call_id)
            .map(|(id, _)| *id)
    }

    pub async fn image_exec_is_active(&self, id: u32, call_id: &str) -> bool {
        let operations = self.image_operations.lock().await;
        operations
            .get(call_id)
            .is_some_and(|entries| entries.iter().any(|entry| entry.id == id))
    }

    /// Begins a cancellable image scope, distinct from `local_cancellation` so a new
    /// message can stop image I/O without cancelling the whole run.
    pub async fn begin_image_operation(&self, call_id: &str) -> ImageOperation {
        let operation = ImageOperation {
            token: CancellationToken::new(),
        };
        self.image_scopes
            .lock()
            .await
            .insert(call_id.to_owned(), operation.token.clone());
        self.image_operations
            .lock()
            .await
            .insert(call_id.to_owned(), Vec::new());
        operation
    }

    pub async fn finish_image_operation(&self, call_id: &str) {
        self.image_scopes.lock().await.remove(call_id);
        self.image_operations.lock().await.remove(call_id);
    }

    /// Cancels every image scope without touching `local_cancellation`: a new message
    /// must stop image I/O but leave the rest of the run alone.
    async fn cancel_image_operations(&self) {
        for token in self.image_scopes.lock().await.values() {
            token.cancel();
        }
        // Cancelling the scope is enough: every exchange awaits either a response or
        // its token, so clearing the map leaves no waiter behind.
        self.image_scopes.lock().await.clear();
        self.image_operations.lock().await.clear();
    }

    /// Aborts in-flight image execs so the client stops waiting on them.
    pub async fn abort_image_io(&self, call_id: &str) -> Vec<u32> {
        self.image_operations
            .lock()
            .await
            .remove(call_id)
            .map(|entries| {
                entries
                    .into_iter()
                    .map(|entry| entry.id)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    }

    /// Issues one Read/Write exchange on Cursor's executor and awaits its result.
    ///
    /// The exchange is registered before the request is sent so an abort can always
    /// find it.
    pub async fn image_io(
        &self,
        results: &super::tool_call_result::ToolResultSender,
        call: &ToolCall,
        message: pb::exec_server_message::Message,
        cancellation: &CancellationToken,
    ) -> Result<pb::exec_client_message::Message> {
        let id = self.next_id()?;
        let exec_id = format!("{}:image-io:{id}", call.call_id);
        let (reply, wait) = oneshot::channel();
        self.image_operations
            .lock()
            .await
            .entry(call.call_id.clone())
            .or_default()
            .push(ImageIoPending {
                id,
                reply: Some(reply),
            });
        results.send_exec(
            call.call_id.clone(),
            pb::AgentServerMessage {
                message: Some(pb::agent_server_message::Message::ExecServerMessage(
                    pb::ExecServerMessage {
                        id,
                        exec_id,
                        message: Some(message),
                        ..Default::default()
                    },
                )),
                ..Default::default()
            },
        )?;
        match tokio::select! {
            biased;
            () = cancellation.cancelled() => Err(Error::Cancelled),
            response = wait => {
                response.map_err(|_| Error::Protocol("image exchange channel closed".into()))?
            }
        } {
            Ok(message) => {
                self.forget_image_io(&call.call_id, id).await;
                Ok(message)
            }
            Err(error) => {
                self.forget_image_io(&call.call_id, id).await;
                // Tell Cursor to stop watching this exec; no result is coming.
                let _ = results.send_exec(call.call_id.clone(), super::codec::abort(id));
                Err(error)
            }
        }
    }

    /// Drops a finished exchange so a late response has nowhere to go and an abort
    /// cannot resurrect it.
    async fn forget_image_io(&self, call_id: &str, id: u32) {
        let mut operations = self.image_operations.lock().await;
        let Some(entries) = operations.get_mut(call_id) else {
            return;
        };
        entries.retain(|entry| entry.id != id);
        if entries.is_empty() {
            operations.remove(call_id);
        }
    }

    /// Delivers an executor response to whichever image exchange is waiting for it.
    pub async fn deliver_image_io(
        &self,
        exec_id: u32,
        message: Option<pb::exec_client_message::Message>,
    ) -> bool {
        let Some(message) = message else {
            return false;
        };
        let pending = {
            let mut operations = self.image_operations.lock().await;
            let mut found = None;
            operations.retain(
                |_, entries| match entries.iter().position(|e| e.id == exec_id) {
                    Some(index) => {
                        found = Some(entries.remove(index));
                        false
                    }
                    None => true,
                },
            );
            found
        };
        match pending.and_then(|entry| entry.reply) {
            Some(reply) => reply.send(Ok(message)).is_ok(),
            None => false,
        }
    }

    fn next_id(&self) -> Result<u32> {
        self.next_id
            .fetch_add(1, Ordering::Relaxed)
            .checked_add(1)
            .ok_or_else(|| Error::Protocol("Cursor message id space exhausted".into()))
    }
}

/// A cancellable scope for one image read/write exchange.
///
/// Derefs to its token so callers that need a `&CancellationToken` can pass it
/// directly; the guard cancels on drop, which also covers early returns.
#[derive(Clone)]
pub struct ImageOperation {
    token: CancellationToken,
}

impl std::ops::Deref for ImageOperation {
    type Target = CancellationToken;

    fn deref(&self) -> &Self::Target {
        &self.token
    }
}

impl ImageOperation {
    pub fn drop_guard(self) -> ImageOperationGuard {
        ImageOperationGuard { inner: Some(self) }
    }
}

pub struct ImageOperationGuard {
    inner: Option<ImageOperation>,
}

impl Drop for ImageOperationGuard {
    fn drop(&mut self) {
        if let Some(inner) = self.inner.take() {
            inner.token.cancel();
        }
    }
}

fn is_shell(name: &str) -> bool {
    name.eq_ignore_ascii_case("Shell") || name.eq_ignore_ascii_case("Bash")
}

pub(crate) fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or_default()
}
