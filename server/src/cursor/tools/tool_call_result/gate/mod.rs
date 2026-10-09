//! Gates each tool result to a size the model can actually read.
//!
//! `tool_completion` is the entry point: it hands the call to the module for that
//! tool family, then applies the shared text cap. A family with nothing to trim is
//! left untouched.

mod edit;
mod glob;
mod grep;
mod image;
mod mcp;
mod read;
mod shell;
#[cfg(test)]
mod tests;
mod truncation;
mod web;

use crate::{cursor::protocol::proto::agent::v1 as pb, model::limit_tool_result_text};

use edit::gate_edit;
use glob::gate_glob;
use grep::gate_grep;
use image::gate_generate_image;
use mcp::{gate_mcp, gate_mcp_resource, gate_mcp_resources, gate_mcp_tools};
use read::gate_read;
use shell::{gate_shell, gate_shell_result};
use web::{gate_web_fetch, gate_web_search};

pub(super) const KIB: usize = 1024;
pub(super) const READ_CONTENT_LIMIT: usize = 64 * KIB;
pub(super) const READ_BINARY_LIMIT: usize = 32 * KIB;
pub(super) const SHELL_STREAM_LIMIT: usize = 16 * KIB;
pub(super) const SHELL_INTERLEAVED_LIMIT: usize = 32 * KIB;
pub(super) const GREP_CONTENT_LIMIT: usize = 32 * KIB;
pub(super) const GREP_MATCH_LIMIT: usize = 2 * KIB;
pub(super) const GREP_MATCHES_PER_FILE: usize = 100;
pub(super) const GREP_TOTAL_MATCHES: usize = 300;
pub(super) const GREP_LIST_LIMIT: usize = 300;
pub(super) const GLOB_FILE_LIMIT: usize = 200;
pub(super) const EDIT_RESULT_LIMIT: usize = 32 * KIB;
pub(super) const PATCH_EDIT_RESULT_LIMIT: usize = 4 * KIB;
pub(super) const MCP_TEXT_LIMIT: usize = 32 * KIB;
pub(super) const MCP_CONTENT_ITEM_LIMIT: usize = 20;
pub(super) const MCP_STRUCTURED_LIMIT: usize = 32 * KIB;
pub(super) const MCP_BINARY_LIMIT: usize = 32 * KIB;
pub(super) const MCP_RESOURCE_LIMIT: usize = 200;
pub(super) const MCP_RESOURCE_DESCRIPTION_LIMIT: usize = KIB;
pub(super) const WEB_FETCH_LIMIT: usize = 32 * KIB;
pub(super) const WEB_SEARCH_LIMIT: usize = 16 * KIB;
pub(super) const WEB_SEARCH_TITLE_LIMIT: usize = 512;
pub(super) const WEB_SEARCH_SNIPPET_LIMIT: usize = 2 * KIB;

pub(super) fn tool_completion(
    tool_name: &str,
    tool: &mut pb::tool_call::Tool,
    content: &mut String,
) {
    use pb::tool_call::Tool;

    match tool {
        Tool::ShellToolCall(tool) => gate_shell(tool),
        Tool::GrepToolCall(tool) => gate_grep(tool),
        Tool::GlobToolCall(tool) => gate_glob(tool),
        Tool::ReadToolCall(tool) => gate_read(tool),
        Tool::EditToolCall(tool) => gate_edit(tool_name, tool),
        Tool::McpToolCall(tool) => gate_mcp(tool),
        Tool::ListMcpResourcesToolCall(tool) => gate_mcp_resources(tool),
        Tool::ReadMcpResourceToolCall(tool) => gate_mcp_resource(tool),
        Tool::GetMcpToolsToolCall(tool) => gate_mcp_tools(tool),
        Tool::WebFetchToolCall(tool) => gate_web_fetch(tool),
        Tool::WebSearchToolCall(tool) => gate_web_search(tool),
        Tool::GenerateImageToolCall(tool) => gate_generate_image(tool),
        _ => {}
    }
    *content = limit_tool_result_text(tool_name, content);
}

pub(super) fn exec_message(message: &mut pb::exec_client_message::Message) {
    use pb::exec_client_message::Message;
    match message {
        Message::ShellResult(result) | Message::MiniSweAgentBashResult(result) => {
            gate_shell_result(result)
        }
        _ => {}
    }
}
