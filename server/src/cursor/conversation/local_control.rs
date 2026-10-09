//! Nexusor controls existing Cursor executions without changing Cursor program files.
use crate::{
    cursor::{
        tools::{codec, runtime::CursorToolRuntime},
        transport::TransportHandle,
    },
    Error, Result,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize)]
pub struct ControlRequest {
    pub run_id: String,
    pub action: ControlAction,
    #[serde(default)]
    pub tool_call_id: String,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlAction {
    StopTool,
    BackgroundTool,
    StopRun,
}

#[derive(Clone, Debug, Serialize)]
pub struct ActiveLocalRun {
    pub request_id: String,
    pub run_id: String,
    pub conversation_id: String,
    pub parent_request_id: Option<String>,
    pub tools: Vec<ActiveLocalTool>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        cursor::{
            protocol::{connect, proto::agent::v1 as pb},
            services::observability::CursorTraceService,
            tools::runtime::ExecContext,
            transport::OutputHub,
        },
        model::ToolCall,
        store::Store,
    };
    use std::sync::Arc;

    fn tool(id: &str, name: &str) -> ToolCall {
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

    async fn handle() -> TransportHandle {
        let store = Store::connect("sqlite::memory:").await.unwrap();
        let (tx, _rx) = tokio::sync::mpsc::channel(4);
        TransportHandle::new(
            "request".into(),
            tx,
            Arc::new(OutputHub::default()),
            CursorTraceService::new(store).recorder("request"),
        )
    }

    #[tokio::test]
    async fn local_controls_target_existing_exec_and_preserve_siblings() {
        let runtime = CursorToolRuntime::default();
        let handle = handle().await;
        let mut wire = handle.subscribe();
        let context = ExecContext::default();
        let a = runtime
            .reserve_exec(&tool("a", "Task"), &context)
            .await
            .unwrap();
        let b = runtime
            .reserve_exec(&tool("b", "Task"), &context)
            .await
            .unwrap();
        let mut request = ControlRequest {
            run_id: "run".into(),
            tool_call_id: "a".into(),
            action: ControlAction::StopTool,
        };
        assert_eq!(
            apply(&runtime, &handle, &request).await.unwrap().status,
            "stop_requested"
        );
        let msg: pb::AgentServerMessage =
            connect::decode_unary(&wire.recv().await.unwrap()).unwrap();
        assert_eq!(msg, codec::abort(a));
        assert!(
            runtime.exec_call(a).await.is_some(),
            "await actual client result"
        );
        assert!(runtime.exec_call(b).await.is_some());
        request.action = ControlAction::BackgroundTool;
        assert_eq!(
            apply(&runtime, &handle, &request).await.unwrap().status,
            "background_requested"
        );
        let msg: pb::AgentServerMessage =
            connect::decode_unary(&wire.recv().await.unwrap()).unwrap();
        assert!(
            matches!(msg.message,Some(pb::agent_server_message::Message::ExecServerMessage(pb::ExecServerMessage{
            message:Some(pb::exec_server_message::Message::ForceBackgroundSubagentArgs(args)),..
        })) if args.tool_call_id=="a")
        );
        assert_eq!(
            apply(&runtime, &handle, &request).await.unwrap().status,
            "background_queued_or_already_requested"
        );
        assert!(wire.try_recv().is_err());
        runtime.discard_exec(a).await;
        assert!(apply(&runtime, &handle, &request).await.is_err());
        assert!(wire.try_recv().is_err());
    }

    #[tokio::test]
    async fn local_controls_reject_reads_and_queue_only_existing_early_shell() {
        let runtime = CursorToolRuntime::default();
        let handle = handle().await;
        let mut wire = handle.subscribe();
        let context = ExecContext::default();
        runtime
            .reserve_exec(&tool("read", "Read"), &context)
            .await
            .unwrap();
        let shell = runtime
            .reserve_exec(&tool("shell", "Shell"), &context)
            .await
            .unwrap();
        let mut request = ControlRequest {
            run_id: "run".into(),
            tool_call_id: "read".into(),
            action: ControlAction::StopTool,
        };
        assert!(apply(&runtime, &handle, &request).await.is_err());
        request.tool_call_id = "unknown".into();
        request.action = ControlAction::BackgroundTool;
        assert!(apply(&runtime, &handle, &request).await.is_err());
        request.tool_call_id = "shell".into();
        assert_eq!(
            apply(&runtime, &handle, &request).await.unwrap().status,
            "background_queued_or_already_requested"
        );
        assert!(wire.try_recv().is_err());
        let msg = runtime.shell_started(shell).await.unwrap().unwrap();
        assert!(
            matches!(msg.message,Some(pb::agent_server_message::Message::ExecServerMessage(pb::ExecServerMessage{
            message:Some(pb::exec_server_message::Message::ForceBackgroundShellArgs(args)),..
        })) if args.tool_call_id=="shell")
        );
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct ActiveLocalTool {
    pub call_id: String,
    pub name: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct ControlResponse {
    pub status: &'static str,
}

pub(crate) async fn apply(
    runtime: &CursorToolRuntime,
    handle: &TransportHandle,
    request: &ControlRequest,
) -> Result<ControlResponse> {
    if request.tool_call_id.is_empty() || request.tool_call_id.len() > 512 {
        return Err(Error::Protocol("A current tool_call_id is required".into()));
    }
    let tools = runtime.controllable_tools().await;
    let tool = tools
        .iter()
        .find(|tool| tool.call_id == request.tool_call_id)
        .ok_or_else(|| {
            Error::RunNotFound("Tool has ended or is not controlled by this active run".into())
        })?;
    match request.action {
        ControlAction::StopRun => Err(Error::Protocol(
            "Run controls must be handled by its active runtime".into(),
        )),
        ControlAction::StopTool => {
            let id = runtime
                .controllable_exec_id(&tool.call_id)
                .await
                .ok_or_else(|| Error::RunNotFound("Tool ended before stop".into()))?;
            handle.emit(&codec::abort(id))?;
            Ok(ControlResponse {
                status: "stop_requested",
            })
        }
        ControlAction::BackgroundTool => {
            let task = tool.name.eq_ignore_ascii_case("Task");
            match runtime.background_request(&tool.call_id, task).await? {
                Some(message) => {
                    handle.emit(&message)?;
                    Ok(ControlResponse {
                        status: "background_requested",
                    })
                }
                None => Ok(ControlResponse {
                    status: "background_queued_or_already_requested",
                }),
            }
        }
    }
}
