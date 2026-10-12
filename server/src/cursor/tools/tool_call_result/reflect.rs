//! Implements `Reflect`, the structured self-review tool.
//!
//! Cursor's `Reflect` records five model-authored fields and returns immediately;
//! it has no exec-channel message and no external dependency. Nexusor stores the
//! same fields and returns the same empty success.
use serde_json::Value;

use crate::{
    cursor::protocol::proto::agent::v1 as pb,
    model::{ToolCall, ToolResult},
    Result,
};

use super::{now_ms, ToolCompletion};

const FIELDS: [&str; 5] = [
    "unexpected_action_outcomes",
    "relevant_instructions",
    "scenario_analysis",
    "critical_synthesis",
    "next_steps",
];

fn field(call: &ToolCall, name: &str) -> String {
    call.arguments
        .get(name)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

pub(crate) async fn reflect(call: &ToolCall) -> Result<ToolCompletion> {
    let unexpected = field(call, FIELDS[0]);
    let instructions = field(call, FIELDS[1]);
    let analysis = field(call, FIELDS[2]);
    let synthesis = field(call, FIELDS[3]);
    let next = field(call, FIELDS[4]);

    let mut recorded = Vec::new();
    if !unexpected.is_empty() {
        recorded.push("unexpected action outcomes");
    }
    if !instructions.is_empty() {
        recorded.push("relevant instructions");
    }
    if !analysis.is_empty() {
        recorded.push("scenario analysis");
    }
    if !synthesis.is_empty() {
        recorded.push("critical synthesis");
    }
    if !next.is_empty() {
        recorded.push("next steps");
    }
    let summary = if recorded.is_empty() {
        "Reflection recorded, but it contained no fields.".to_string()
    } else {
        format!("Reflection recorded: {}.", recorded.join(", "))
    };

    Ok(ToolCompletion::new(
        call,
        now_ms(),
        ToolResult {
            call_id: call.call_id.clone(),
            content: summary,
            is_error: false,
            image: None,
            images: Vec::new(),
        },
        pb::tool_call::Tool::ReflectToolCall(pb::ReflectToolCall {
            args: Some(pb::ReflectArgs {
                unexpected_action_outcomes: unexpected,
                relevant_instructions: instructions,
                scenario_analysis: analysis,
                critical_synthesis: synthesis,
                next_steps: next,
                tool_call_id: call.call_id.clone(),
            }),
            result: Some(pb::ReflectResult {
                result: Some(pb::reflect_result::Result::Success(pb::ReflectSuccess {})),
            }),
        }),
    ))
}
