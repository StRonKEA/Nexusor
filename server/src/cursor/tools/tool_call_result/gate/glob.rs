//! Caps how many paths a Glob result may list.

use crate::cursor::protocol::proto::agent::v1 as pb;

use super::GLOB_FILE_LIMIT;

pub(super) fn gate_glob(tool: &mut pb::GlobToolCall) {
    let Some(pb::glob_tool_result::Result::Success(success)) = tool
        .result
        .as_mut()
        .and_then(|result| result.result.as_mut())
    else {
        return;
    };
    let original = success.files.len();
    if original <= GLOB_FILE_LIMIT {
        if success.total_files <= 0 {
            success.total_files = original as i32;
        }
        return;
    }
    success.files.truncate(GLOB_FILE_LIMIT);
    success.total_files = success.total_files.max(original as i32);
    success.client_truncated = true;
}
