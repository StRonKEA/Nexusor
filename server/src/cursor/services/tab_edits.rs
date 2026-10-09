//! Validates model-proposed edits against the supplied buffer before encoding them.
use crate::{cursor::protocol::proto::tab as pb, Error, Result};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Proposal {
    edits: Vec<Edit>,
    #[serde(default)]
    next: Option<Next>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Edit {
    start_line: i32,
    end_line: i32,
    text: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Next {
    path: String,
    line: i32,
}

fn invalid(message: &str) -> Error {
    Error::Protocol(format!("invalid Tab proposal: {message}"))
}

/// None preserves compatibility with models returning an ordinary insertion.
pub(super) fn frames(
    text: &str,
    input: &pb::Request,
    first: i32,
    last: i32,
) -> Result<Option<Vec<pb::Response>>> {
    let trimmed = text.trim();
    if !trimmed.starts_with('{') || !trimmed.contains("\"edits\"") {
        return Ok(None);
    }
    let proposal: Proposal = serde_json::from_str(trimmed).map_err(|e| invalid(&e.to_string()))?;
    if proposal.edits.len() > 8 {
        return Err(invalid("too many edits"));
    }
    let file = input
        .current_file
        .as_ref()
        .ok_or_else(|| invalid("missing buffer"))?;
    let mut previous_end = 0;
    let mut shift = 0i32;
    let mut output = vec![pb::Response {
        model_info: Some(pb::ModelInfo {
            is_fused_cursor_prediction_model: true,
            is_multidiff_model: true,
        }),
        ..Default::default()
    }];
    let mut count = 0;
    let mut prediction_shift = 0;
    let mut prediction_edited = false;
    for edit in proposal.edits {
        if edit.start_line < first
            || edit.end_line > last
            || edit.end_line < edit.start_line
            || edit.start_line <= previous_end
        {
            return Err(invalid(
                "edits must be ordered, non-overlapping and inside supplied context",
            ));
        }
        previous_end = edit.end_line;
        let original = file
            .contents
            .split('\n')
            .skip((edit.start_line - file.contents_start_at_line - 1) as usize)
            .take((edit.end_line - edit.start_line + 1) as usize)
            .map(|line| line.trim_end_matches('\r'))
            .collect::<Vec<_>>()
            .join("\n");
        if original == edit.text.replace("\r\n", "\n") {
            continue;
        }
        let start = edit
            .start_line
            .checked_add(shift)
            .filter(|line| *line >= 1)
            .ok_or_else(|| invalid("line overflow"))?;
        let end = edit
            .end_line
            .checked_add(shift)
            .ok_or_else(|| invalid("line overflow"))?;
        let text = edit.text.replace("\r\n", "\n");
        if count > 0 {
            output.push(pb::Response {
                begin_edit: Some(true),
                ..Default::default()
            });
        }
        output.push(pb::Response {
            range_to_replace: Some(pb::Range {
                start_line_number: start,
                end_line_number_inclusive: end,
            }),
            should_remove_leading_eol: Some(true),
            ..Default::default()
        });
        // Multidiff removes this sentinel EOL while preserving the preceding line.
        output.push(pb::Response {
            text: format!("\n{text}"),
            ..Default::default()
        });
        output.push(pb::Response {
            done_edit: Some(true),
            ..Default::default()
        });
        let line_delta = text.split('\n').count() as i32 - (edit.end_line - edit.start_line + 1);
        shift += line_delta;
        if let Some(next) = &proposal.next {
            if next.path == file.relative_workspace_path {
                if next.line > edit.end_line {
                    prediction_shift += line_delta;
                } else if next.line >= edit.start_line {
                    prediction_edited = true;
                }
            }
        }
        count += 1;
    }
    if let Some(next) = proposal.next.filter(|_| input.supports_cpt == Some(true)) {
        if next.path.starts_with(['/', '\\'])
            || next.path.contains(':')
            || next.path.split(['/', '\\']).any(|part| part == "..")
            || next.line < 1
        {
            return Err(invalid("prediction path/line is invalid"));
        }
        let expected = if next.path == file.relative_workspace_path {
            next.line
                .checked_sub(file.contents_start_at_line + 1)
                .and_then(|line| usize::try_from(line).ok())
                .and_then(|line| file.contents.split('\n').nth(line))
                .map(str::to_owned)
        } else {
            input
                .additional_files
                .iter()
                .filter(|other| other.relative_workspace_path == next.path)
                .flat_map(|other| {
                    other
                        .visible_range_content
                        .iter()
                        .zip(&other.start_line_number_one_indexed)
                })
                .find_map(|(content, start)| {
                    next.line
                        .checked_sub(*start)
                        .and_then(|line| usize::try_from(line).ok())
                        .and_then(|line| content.split('\n').nth(line))
                        .map(str::to_owned)
                })
        }
        .ok_or_else(|| invalid("prediction target is not in supplied file context"))?;
        // Cursor's multidiff parser sets shouldAdjustLineNumber=false. The
        // target follows the last edit and must use post-edit coordinates.
        if !prediction_edited {
            output.push(pb::Response {
                cursor_prediction_target: Some(pb::CursorPrediction {
                    relative_path: next.path,
                    line_number_one_indexed: next.line + prediction_shift,
                    expected_content: expected.trim_end_matches('\r').to_owned(),
                    should_retrigger_cpp: true,
                }),
                ..Default::default()
            });
        }
    }
    Ok(Some(output))
}
