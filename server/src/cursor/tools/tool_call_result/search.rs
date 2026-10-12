//! Converts `SemanticSearch` completions into Tool results.

use serde_json::Value;

use crate::{
    cursor::protocol::proto::agent::v1 as pb,
    model::{ToolCall, ToolResult},
    Result,
};

use super::ToolCompletion;

pub(crate) fn complete(
    call: &ToolCall,
    started_at_ms: u64,
    output: std::result::Result<Value, String>,
) -> Result<ToolCompletion> {
    use pb::sem_search_tool_result::Result as SearchResult;

    let description = call
        .arguments
        .get("description")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("Search the codebase")
        .to_owned();
    let (content, is_error, result) = match output {
        Ok(value) => {
            let content = serde_json::to_string_pretty(&value)?;
            (
                content.clone(),
                false,
                SearchResult::Success(pb::SemSearchToolSuccess {
                    results: content,
                    code_results: Vec::new(),
                }),
            )
        }
        Err(error) => (
            error.clone(),
            true,
            SearchResult::Error(pb::SemSearchToolError {
                error_message: error,
            }),
        ),
    };
    Ok(ToolCompletion::new(
        call,
        started_at_ms,
        ToolResult {
            call_id: call.call_id.clone(),
            content,
            is_error,
            image: None,
            images: Vec::new(),
        },
        pb::tool_call::Tool::SemSearchToolCall(pb::SemSearchToolCall {
            args: Some(pb::SemSearchToolArgs {
                query: string(call, "query"),
                target_directories: vec![string(call, "repo")],
                explanation: description,
            }),
            result: Some(pb::SemSearchToolResult {
                result: Some(result),
            }),
        }),
    ))
}

fn string(call: &ToolCall, name: &str) -> String {
    call.arguments
        .get(name)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}
