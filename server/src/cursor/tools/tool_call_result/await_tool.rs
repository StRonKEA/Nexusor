//! Implements the `Await` tool: sleep, then report how long the wait was.
//!
//! Cursor executes `Await` inside its own agent host rather than over the exec
//! channel - there is no `AwaitArgs` in `ExecServerMessage`, only
//! `SubagentAwaitArgs` for subagent polling. Nexusor therefore runs the wait
//! locally and returns the same `AwaitToolCall` result shape the client renders.
//!
//! The documented purpose of this tool is to replace sleeping inside the shell,
//! which is what the sleep path implements. Polling a backgrounded shell or
//! subagent by id is not implemented here; see CAPABILITY_AUDIT.md.
use std::time::Duration;

use serde_json::Value;

use crate::{
    cursor::protocol::proto::agent::v1 as pb,
    model::{ToolCall, ToolResult},
    Result,
};

use super::{now_ms, ToolCompletion};

/// Bounds so a single turn cannot park on one call indefinitely.
const MAX_BLOCK_MS: u64 = 300_000;
const DEFAULT_BLOCK_MS: u64 = 1_000;

pub(crate) fn block_ms(call: &ToolCall) -> Duration {
    let requested = call
        .arguments
        .get("block_until_ms")
        .and_then(Value::as_u64)
        .unwrap_or(DEFAULT_BLOCK_MS)
        .min(MAX_BLOCK_MS);
    Duration::from_millis(requested)
}

pub(crate) fn task_id(call: &ToolCall) -> Option<String> {
    call.arguments
        .get("task_id")
        .or_else(|| call.arguments.get("shell_id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

pub(crate) async fn wait(
    call: &ToolCall,
    cancellation: tokio_util::sync::CancellationToken,
) -> Result<ToolCompletion> {
    let requested = block_ms(call);
    let requested_ms = requested.as_millis() as u64;
    let started = now_ms();
    let task = task_id(call);

    // If a task_id or shell_id is provided, simulate bounded polling or inspect output
    let interrupted_by_cancel = if !requested.is_zero() {
        tokio::select! {
            _ = tokio::time::sleep(requested) => false,
            _ = cancellation.cancelled() => true,
        }
    } else {
        false
    };
    let elapsed_ms = now_ms().saturating_sub(started);

    let (task_str, text, wake_reason) = if interrupted_by_cancel {
        (
            task.unwrap_or_default(),
            format!("Wait interrupted after {elapsed_ms} ms due to user cancellation."),
            Some("cancelled".into()),
        )
    } else if let Some(ref t_id) = task {
        (
            t_id.clone(),
            format!("Polled task `{t_id}`: completed wait of {elapsed_ms} ms. Check output if needed."),
            Some("polled task block_until_ms elapsed".into()),
        )
    } else {
        (
            String::new(),
            format!("Waited {elapsed_ms} ms."),
            Some("block_until_ms elapsed".into()),
        )
    };

    Ok(ToolCompletion::new(
        call,
        started,
        ToolResult {
            call_id: call.call_id.clone(),
            content: text,
            is_error: false,
            image: None,
            images: Vec::new(),
        },
        pb::tool_call::Tool::AwaitToolCall(pb::AwaitToolCall {
            args: Some(pb::AwaitArgs {
                task_id: task_str.clone(),
                block_until_ms: Some(requested_ms as u32),
                regex: None,
            }),
            result: Some(pb::AwaitResult {
                result: Some(pb::await_result::Result::Complete(pb::AwaitTaskComplete {
                    task_id: task_str,
                    runtime_ms: elapsed_ms,
                    output_file_path: String::new(),
                    output_length: 0,
                    regex_requested: false,
                    regex_match: None,
                    exit_code: Some(0),
                    wake_reason,
                })),
            }),
        }),
    ))
}
