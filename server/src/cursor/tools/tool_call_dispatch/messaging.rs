//! Dispatches the user-facing message tools and `Reflect`.
//!
//! `SendToUser`, `SendMessage`, `SendFinalSummary` and `Reflect` have no
//! exec-channel message in Cursor's protocol, so they complete here and draw
//! their own card.
use crate::{model::ToolCall, Result};

use crate::cursor::tools::tool_call_result::{self as result, ToolResultSender};

use super::ToolStart;

pub(super) fn start(results: &ToolResultSender, call: &ToolCall) -> Result<ToolStart> {
    let call = call.clone();
    let results = results.clone();
    tokio::spawn(async move {
        let outcome = match call.name.to_ascii_lowercase().as_str() {
            name if name.contains("creategoal") => result::create_goal(&call).await,
            name if name.contains("updategoal") => result::update_goal(&call).await,
            name if name.contains("reflect") => result::reflect(&call).await,
            name if name.contains("sendto") => result::send_to_user(&call).await,
            name if name.contains("finalsummary") => result::send_final_summary(&call).await,
            _ => result::send_message(&call).await,
        };
        match outcome {
            Ok(completion) => results.send(completion),
            Err(error) => results.send_error(error),
        }
    });
    Ok(ToolStart {
        messages: Vec::new(),
        completion: None,
    })
}
