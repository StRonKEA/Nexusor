//! Dispatches `Await` tool calls.
//!
//! `Await` has no exec-channel message in Cursor's protocol, so the wait runs
//! here and reports back through the ordinary completion path.
use crate::{model::ToolCall, Result};

use crate::cursor::tools::{
    runtime::CursorToolRuntime,
    tool_call_result::{self as result, ToolResultSender},
};

use super::ToolStart;

pub(super) fn start(
    runtime: &CursorToolRuntime,
    results: &ToolResultSender,
    call: &ToolCall,
) -> Result<ToolStart> {
    let call = call.clone();
    let results = results.clone();
    let cancellation = runtime.local_cancellation.clone();
    tokio::spawn(async move {
        match result::await_wait(&call, cancellation).await {
            Ok(completion) => results.send(completion),
            Err(error) => results.send_error(error),
        }
    });
    Ok(ToolStart {
        messages: Vec::new(),
        completion: None,
    })
}
