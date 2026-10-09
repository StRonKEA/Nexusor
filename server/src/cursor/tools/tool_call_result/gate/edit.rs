//! Trims an Edit result: a plain edit keeps room for the diff, a patch edit much
//! less, because the patch itself is already the whole answer.

use crate::cursor::protocol::proto::agent::v1 as pb;

use super::truncation::truncate_text;
use super::*;

pub(super) fn gate_edit(tool_name: &str, tool: &mut pb::EditToolCall) {
    let Some(pb::edit_result::Result::Success(success)) = tool
        .result
        .as_mut()
        .and_then(|result| result.result.as_mut())
    else {
        return;
    };
    let limit = match tool_name.trim() {
        "PatchEdit" | "PatchEditLines" | "PatchEditSpan" | "StrReplace" => PATCH_EDIT_RESULT_LIMIT,
        _ => EDIT_RESULT_LIMIT,
    };
    if let Some(diff) = success.diff_string.as_mut() {
        *diff = truncate_text(tool_name, diff, limit);
        success.before_full_file_content = None;
        success.after_full_file_content.clear();
    } else {
        success.before_full_file_content = None;
        success.after_full_file_content =
            truncate_text(tool_name, &success.after_full_file_content, limit);
    }
}
