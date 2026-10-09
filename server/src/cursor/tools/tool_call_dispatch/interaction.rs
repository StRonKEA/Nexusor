//! Dispatches Tool calls that require Cursor user interaction.
//! Interaction query dispatch and approval continuation.

use crate::{
    cursor::{protocol::proto::agent::v1 as pb, tools::codec as interaction},
    model::ToolCall,
    search::{WebFetch, WebSearch},
    Error, Result,
};

use super::{normalized, InteractionContinuation, ToolStart};
use crate::cursor::tools::{
    runtime::{CursorToolRuntime, PendingInteraction},
    tool_call_result::{self as result, ToolResultSender},
};

pub(super) async fn start(runtime: &CursorToolRuntime, call: &ToolCall) -> Result<ToolStart> {
    let id = runtime.reserve_interaction(call).await?;
    Ok(ToolStart {
        messages: vec![interaction::tool_query(id, call)?],
        completion: None,
    })
}

pub(super) async fn resume(
    runtime: &CursorToolRuntime,
    results: &ToolResultSender,
    search: &WebSearch,
    fetch: &WebFetch,
    plugins: Option<&crate::plugin::PluginRegistry>,
    pending: PendingInteraction,
    response: &pb::InteractionResponse,
) -> Result<InteractionContinuation> {
    if let Some(pb::interaction_response::Result::GenerateImageRequestResponse(value)) =
        response.result.as_ref()
    {
        if normalized(&pending.call.name) == "generateimage" {
            if let Some(pb::generate_image_request_response::Result::Approved(approved)) =
                value.result.as_ref()
            {
                let plugins = plugins
                    .ok_or_else(|| Error::Protocol("image generator is unavailable".into()))?
                    .clone();
                let input = &pending.call.arguments;
                let references = input
                    .get("reference_image_paths")
                    .cloned()
                    .unwrap_or_else(|| serde_json::json!([]));
                let mut args = pb::GenerateImageArgs {
                    description: input
                        .get("description")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default()
                        .into(),
                    file_path: input
                        .get("file_path")
                        .or_else(|| input.get("filename"))
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_owned),
                    reference_image_paths: serde_json::from_value(references)?,
                    aspect_ratio: input
                        .get("aspect_ratio")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_owned),
                };
                if !approved.description.trim().is_empty() {
                    args.description = approved.description.clone();
                }
                let cancellation = runtime.local_cancellation.clone();
                let runtime = runtime.clone();
                let results = results.clone();
                tokio::spawn(async move {
                    let outcome = crate::cursor::tools::image_io::generate(
                        &runtime,
                        &results,
                        &pending.call,
                        &plugins,
                        &args,
                        &cancellation,
                    )
                    .await;
                    if !cancellation.is_cancelled() {
                        results.send(result::complete_generated_image(pending, args, outcome));
                    }
                });
                return Ok(InteractionContinuation::Pending);
            }
        }
    }
    if normalized(&pending.call.name) == "websearch"
        && matches!(
            response.result.as_ref(),
            Some(pb::interaction_response::Result::WebSearchRequestResponse(
                pb::WebSearchRequestResponse {
                    result: Some(pb::web_search_request_response::Result::Approved(_)),
                }
            ))
        )
    {
        start_web_search(results.clone(), search.clone(), pending)?;
        return Ok(InteractionContinuation::Pending);
    }
    if normalized(&pending.call.name) == "webfetch"
        && matches!(
            response.result.as_ref(),
            Some(pb::interaction_response::Result::WebFetchRequestResponse(
                pb::WebFetchRequestResponse {
                    result: Some(pb::web_fetch_request_response::Result::Approved(_)),
                }
            ))
        )
    {
        start_web_fetch(results.clone(), fetch.clone(), pending)?;
        return Ok(InteractionContinuation::Pending);
    }
    Ok(InteractionContinuation::Completed(Box::new(
        result::from_interaction(pending, response)?,
    )))
}

fn start_web_fetch(
    results: ToolResultSender,
    fetch: WebFetch,
    pending: PendingInteraction,
) -> Result<()> {
    let url = pending
        .call
        .arguments
        .get("url")
        .and_then(serde_json::Value::as_str)
        .filter(|url| !url.trim().is_empty())
        .ok_or_else(|| Error::Protocol("WebFetch is missing url".into()))?
        .to_string();
    tokio::spawn(async move {
        let outcome = fetch.fetch(&url).await.map_err(|error| error.to_string());
        match result::complete_web_fetch(pending, outcome) {
            Ok(completion) => results.send(completion),
            Err(error) => results.send_error(error),
        }
    });
    Ok(())
}

fn start_web_search(
    results: ToolResultSender,
    search: WebSearch,
    pending: PendingInteraction,
) -> Result<()> {
    // Claude Code 习惯的 query 作为 search_term 的别名兼容。
    let query = ["search_term", "query"]
        .iter()
        .find_map(|name| {
            pending
                .call
                .arguments
                .get(name)
                .and_then(serde_json::Value::as_str)
                .filter(|value| !value.trim().is_empty())
        })
        .ok_or_else(|| Error::Protocol("WebSearch is missing search_term".into()))?
        .to_string();
    tokio::spawn(async move {
        let outcome = search
            .search(&query)
            .await
            .map_err(|error| error.to_string());
        match result::complete_web_search(pending, outcome) {
            Ok(completion) => results.send(completion),
            Err(error) => results.send_error(error),
        }
    });
    Ok(())
}
