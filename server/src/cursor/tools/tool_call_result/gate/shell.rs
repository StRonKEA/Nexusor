//! Trims a Shell result: one stream per delta, bounded in total, and interleaved
//! stdout/stderr marked so the model can tell them apart.

use crate::cursor::protocol::proto::agent::v1 as pb;

use super::truncation::truncate_edges;
use super::*;

pub(super) fn gate_shell(tool: &mut pb::ShellToolCall) {
    if let Some(result) = tool.result.as_mut() {
        gate_shell_result(result);
    }
}

pub(super) fn gate_shell_result(result: &mut pb::ShellResult) {
    use pb::shell_result::Result;
    match result.result.as_mut() {
        Some(Result::Success(success)) => {
            success.stdout = truncate_edges("Shell stdout", &success.stdout, SHELL_STREAM_LIMIT);
            success.stderr = truncate_edges("Shell stderr", &success.stderr, SHELL_STREAM_LIMIT);
            if let Some(interleaved) = success.interleaved_output.as_mut() {
                *interleaved = truncate_edges(
                    "Shell interleaved output",
                    interleaved,
                    SHELL_INTERLEAVED_LIMIT,
                );
            }
        }
        Some(Result::Failure(failure)) => {
            failure.stdout = truncate_edges("Shell stdout", &failure.stdout, SHELL_STREAM_LIMIT);
            failure.stderr = truncate_edges("Shell stderr", &failure.stderr, SHELL_STREAM_LIMIT);
            if let Some(interleaved) = failure.interleaved_output.as_mut() {
                *interleaved = truncate_edges(
                    "Shell interleaved output",
                    interleaved,
                    SHELL_INTERLEAVED_LIMIT,
                );
            }
        }
        _ => {}
    }
}
