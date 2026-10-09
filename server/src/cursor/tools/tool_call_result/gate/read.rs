//! Trims a Read result: text by characters, binary down to a marker and its size.

use crate::cursor::protocol::proto::agent::v1 as pb;

use super::truncation::truncate_text;
use super::truncation::truncation_notice;
use super::*;

pub(super) fn gate_read(tool: &mut pb::ReadToolCall) {
    let Some(pb::read_tool_result::Result::Success(success)) = tool
        .result
        .as_mut()
        .and_then(|result| result.result.as_mut())
    else {
        return;
    };
    // Media bytes must reach the bounded decoder before model-visible projection.
    let binary_limit = match success.output.as_ref() {
        Some(pb::read_tool_success::Output::Data(data)) => {
            match crate::media::kind(data, "", &success.path) {
                Some("pdf") => 32 * 1024 * KIB,
                Some("video") => 128 * 1024 * KIB,
                _ => READ_BINARY_LIMIT,
            }
        }
        _ => READ_BINARY_LIMIT,
    };
    let Some(output) = success.output.as_mut() else {
        return;
    };
    match output {
        pb::read_tool_success::Output::Content(value) => {
            let next = truncate_text("Read", value, READ_CONTENT_LIMIT);
            if next != *value {
                *value = next;
                success.exceeded_limit = true;
            }
        }
        pb::read_tool_success::Output::Data(value) if value.len() > binary_limit => {
            let notice = truncation_notice("Read binary data", binary_limit, 0, value.len());
            success.output = Some(pb::read_tool_success::Output::Content(notice));
            success.exceeded_limit = true;
        }
        _ => {}
    }
}
