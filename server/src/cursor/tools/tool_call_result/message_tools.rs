//! Implements the user-facing message tools: `SendToUser`, `SendMessage` and
//! `SendFinalSummary`.
//!
//! Cursor runs all three inside its agent host rather than over the exec channel:
//! there is no `*Args` entry for them in `ExecServerMessage`. Each one records a
//! message and answers immediately, so Nexusor does the same. The card it renders
//! is the delivery, because Cursor's transcript draws the tool card the model
//! produced.
//!
//! `SendToTask` is deliberately absent: Cursor has no `SendToTaskToolCall`
//! variant in the wire schema, so there is nothing to render.
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

use crate::{
    cursor::protocol::proto::agent::v1 as pb,
    model::{ToolCall, ToolResult},
    Result,
};

use super::{now_ms, ToolCompletion};

fn text(call: &ToolCall, keys: &[&str]) -> String {
    keys.iter()
        .find_map(|key| call.arguments.get(*key))
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_default()
}

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or_default()
}

/// A stable id for the message so a later turn can refer back to it.
fn message_id(call: &ToolCall) -> String {
    format!("msg_{}_{}", now_ms(), call.call_id)
}

pub(crate) async fn send_to_user(call: &ToolCall) -> Result<ToolCompletion> {
    let message = text(call, &["message", "text"]);
    Ok(ToolCompletion::new(
        call,
        now_ms(),
        ToolResult {
            call_id: call.call_id.clone(),
            content: "Message delivered to the user.".to_string(),
            is_error: false,
            image: None,
            images: Vec::new(),
        },
        pb::tool_call::Tool::SendToUserToolCall(pb::SendToUserToolCall {
            args: Some(pb::SendToUserArgs { message }),
            result: Some(pb::SendToUserResult {
                result: Some(pb::send_to_user_result::Result::Success(
                    pb::SendToUserSuccess {},
                )),
            }),
        }),
    ))
}

pub(crate) async fn send_message(call: &ToolCall) -> Result<ToolCompletion> {
    let content = text(call, &["message", "text"]);
    Ok(ToolCompletion::new(
        call,
        now_ms(),
        ToolResult {
            call_id: call.call_id.clone(),
            content: "Message delivered to the user.".to_string(),
            is_error: false,
            image: None,
            images: Vec::new(),
        },
        pb::tool_call::Tool::SendMessageToolCall(pb::SendMessageToolCall {
            args: Some(pb::SendMessageArgs {
                message: Some(pb::send_message_args::Message::Text(pb::SendMessageText {
                    content: content.clone(),
                })),
            }),
            result: Some(pb::SendMessageResult {
                result: Some(pb::send_message_result::Result::Success(
                    pb::SendMessageSuccess {
                        timestamp: unix_ms(),
                        message_id: message_id(call),
                    },
                )),
            }),
        }),
    ))
}

pub(crate) async fn send_final_summary(call: &ToolCall) -> Result<ToolCompletion> {
    let summary = text(call, &["final_summary", "summary", "message"]);
    Ok(ToolCompletion::new(
        call,
        now_ms(),
        ToolResult {
            call_id: call.call_id.clone(),
            content: "Final summary recorded.".to_string(),
            is_error: false,
            image: None,
            images: Vec::new(),
        },
        pb::tool_call::Tool::SendFinalSummaryToolCall(pb::SendFinalSummaryToolCall {
            args: Some(pb::SendFinalSummaryArgs {
                final_summary: Some(summary.clone()),
            }),
            result: Some(pb::SendFinalSummaryResult {
                result: Some(pb::send_final_summary_result::Result::Success(
                    pb::SendFinalSummarySuccess {
                        final_summary: summary,
                    },
                )),
            }),
        }),
    ))
}
