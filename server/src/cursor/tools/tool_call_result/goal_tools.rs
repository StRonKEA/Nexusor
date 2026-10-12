//! Implements `CreateGoal` and `UpdateGoal`, the long-running goal loop.
//!
//! Cursor drives the continuation itself: the agent host restores `goal_state`
//! from the conversation state at the start of a turn, and
//! `willContinueGoalAfterTurn` in the glass bundle opens the next turn while an
//! active goal exists. Nexusor therefore only records the goal - it never starts
//! a turn on its own and never issues a model request the user did not ask for.
//!
//! `GoalStatus` numbering matches Cursor's `agent.v1.GoalStatus` exactly
//! (UNSPECIFIED=0, ACTIVE=1, PAUSED=2, COMPLETE=3, CLEARED=4), and Cursor
//! validates the goal id as a UUID.
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
        .unwrap_or_default()
        .trim()
        .to_string()
}

/// Maps the model's `active`/`complete` names onto Cursor's enum. Anything else
/// is rejected rather than silently treated as `active`, which would keep a
/// finished goal running.
fn status(name: &str) -> Option<i32> {
    match name {
        "active" => Some(pb::GoalStatus::Active as i32),
        "complete" => Some(pb::GoalStatus::Complete as i32),
        _ => None,
    }
}

pub(crate) async fn create_goal(call: &ToolCall) -> Result<ToolCompletion> {
    let objective = text(call, &["objective"]);
    if objective.is_empty() {
        return Err(crate::Error::Protocol(
            "CreateGoal requires a non-empty objective".into(),
        ));
    }
    Ok(ToolCompletion::new(
        call,
        now_ms(),
        ToolResult {
            call_id: call.call_id.clone(),
            content: format!("Goal created: {objective}"),
            is_error: false,
            image: None,
            images: Vec::new(),
        },
        pb::tool_call::Tool::CreateGoalToolCall(pb::CreateGoalToolCall {
            args: Some(pb::CreateGoalArgs {
                objective: objective.clone(),
            }),
            result: Some(pb::CreateGoalResult {
                result: Some(pb::create_goal_result::Result::Success(
                    pb::CreateGoalSuccess {},
                )),
            }),
        }),
    ))
}

pub(crate) async fn update_goal(call: &ToolCall) -> Result<ToolCompletion> {
    let requested = text(call, &["status"]);
    let Some(resolved) = status(&requested) else {
        return Err(crate::Error::Protocol(format!(
            "UpdateGoal status must be `active` or `complete`, got `{requested}`"
        )));
    };
    let label = if resolved == pb::GoalStatus::Complete as i32 {
        "complete"
    } else {
        "active"
    };
    Ok(ToolCompletion::new(
        call,
        now_ms(),
        ToolResult {
            call_id: call.call_id.clone(),
            content: format!("Goal is now {label}"),
            is_error: false,
            image: None,
            images: Vec::new(),
        },
        pb::tool_call::Tool::UpdateGoalToolCall(pb::UpdateGoalToolCall {
            args: Some(pb::UpdateGoalArgs { status: resolved }),
            result: Some(pb::UpdateGoalResult {
                result: Some(pb::update_goal_result::Result::Success(
                    pb::UpdateGoalSuccess { status: resolved },
                )),
            }),
        }),
    ))
}
