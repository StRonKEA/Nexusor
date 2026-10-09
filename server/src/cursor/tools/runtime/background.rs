//! Moving a running Shell or Task into the background.
//!
//! A move is expressed as a Cursor `ForceBackground*Args` message. Cursor only accepts
//! that control for a tool it has begun running, so a request that arrives while the
//! call is still reserved is remembered per call id and emitted once the tool starts.
//! Either way the tool runs exactly once — no second process is spawned.
use crate::{cursor::protocol::proto::agent::v1 as pb, model::ToolCall, Result};

use super::CursorToolRuntime;

/// Caps so a chatty run cannot grow either map without bound.
const MAX_PENDING_INTENTS: usize = 128;
const MAX_BACKGROUND_CONTROLS: usize = 128;

impl CursorToolRuntime {
    /// Whether this exec id belongs to a background control rather than the original
    /// Shell stream, which keeps its own result.
    pub(crate) async fn is_background_control(&self, id: u32) -> bool {
        self.background_controls.lock().await.contains_key(&id)
    }

    /// Requests the background move for a Shell call.
    pub(crate) async fn background_shell_request(
        &self,
        call_id: &str,
    ) -> Result<Option<pb::AgentServerMessage>> {
        self.background_request(call_id, false).await
    }

    /// Requests the background move for a call.
    ///
    /// A Task takes the control as soon as its call is reserved, because a subagent has
    /// no start handshake. A Shell only accepts it once its stream has started, so an
    /// earlier request is remembered and emitted by `shell_started`.
    ///
    /// Returns `None` when the request was remembered or already made.
    pub(crate) async fn background_request(
        &self,
        call_id: &str,
        task: bool,
    ) -> Result<Option<pb::AgentServerMessage>> {
        if self.completed.lock().await.values().any(|id| id == call_id) {
            return Ok(None);
        }
        let emit_now = match self.exec_id_of(call_id).await {
            Some(id) => task || self.started_tools.lock().await.contains(&id),
            None => false,
        };
        if !emit_now {
            let mut intents = self.background_intents.lock().await;
            if intents.len() < MAX_PENDING_INTENTS {
                intents.entry(call_id.to_owned()).or_insert(task);
            }
            return Ok(None);
        }
        let mut controls = self.background_controls.lock().await;
        if controls.values().any(|target| target == call_id) {
            return Ok(None);
        }
        if controls.len() >= MAX_BACKGROUND_CONTROLS {
            return Err(crate::Error::Protocol(
                "too many background controls in one run".into(),
            ));
        }
        let id = self.next_id()?;
        controls.insert(id, call_id.to_owned());
        Ok(Some(force_background(id, call_id, task)))
    }

    /// Marks a tool as running and emits any request that arrived before it started.
    ///
    /// Idempotent: a repeated Start event finds no remembered request and returns
    /// `None`.
    pub(crate) async fn shell_started(&self, id: u32) -> Result<Option<pb::AgentServerMessage>> {
        self.started_tools.lock().await.insert(id);
        let Some(call_id) = self
            .execs
            .lock()
            .await
            .get(&id)
            .map(|entry| entry.call.call_id.clone())
        else {
            return Ok(None);
        };
        let Some(task) = self.background_intents.lock().await.remove(&call_id) else {
            return Ok(None);
        };
        self.background_request(&call_id, task).await
    }

    /// Marks a tool as running as its start message goes out, so a background request
    /// arriving from that moment on is emitted instead of remembered.
    pub(crate) async fn mark_started(&self, call: &ToolCall) {
        if let Some(id) = self.exec_id_of(&call.call_id).await {
            self.started_tools.lock().await.insert(id);
        }
    }

    async fn exec_id_of(&self, call_id: &str) -> Option<u32> {
        self.execs
            .lock()
            .await
            .iter()
            .find(|(_, entry)| entry.call.call_id == call_id)
            .map(|(id, _)| *id)
    }
}

/// The Cursor message that tells an already-started tool to move to the background.
fn force_background(id: u32, call_id: &str, task: bool) -> pb::AgentServerMessage {
    pb::AgentServerMessage {
        message: Some(pb::agent_server_message::Message::ExecServerMessage(
            pb::ExecServerMessage {
                id,
                message: Some(if task {
                    pb::exec_server_message::Message::ForceBackgroundSubagentArgs(
                        pb::ForceBackgroundSubagentArgs {
                            tool_call_id: call_id.to_owned(),
                        },
                    )
                } else {
                    pb::exec_server_message::Message::ForceBackgroundShellArgs(
                        pb::ForceBackgroundShellArgs {
                            tool_call_id: call_id.to_owned(),
                        },
                    )
                }),
                ..Default::default()
            },
        )),
        ..Default::default()
    }
}
