//! Trims a Grep result: per-file and total match budgets, a byte cap on the joined
//! content, and notices left in place of the matches that were dropped.

use crate::cursor::protocol::proto::agent::v1 as pb;

use super::truncation::truncate_text;
use super::truncation::truncation_notice;
use super::*;

pub(super) fn gate_grep(tool: &mut pb::GrepToolCall) {
    let Some(pb::grep_result::Result::Success(success)) = tool
        .result
        .as_mut()
        .and_then(|result| result.result.as_mut())
    else {
        return;
    };
    let mut budget = GrepBudget {
        content_bytes: GREP_CONTENT_LIMIT,
        matches: GREP_TOTAL_MATCHES,
    };
    let mut workspace_names = success
        .workspace_results
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    workspace_names.sort_unstable();
    for name in workspace_names {
        if let Some(result) = success.workspace_results.get_mut(&name) {
            gate_grep_union(result, &mut budget);
        }
    }
    if let Some(result) = success.active_editor_result.as_mut() {
        gate_grep_union(result, &mut budget);
    }
}

pub(super) struct GrepBudget {
    content_bytes: usize,
    matches: usize,
}

pub(super) fn gate_grep_union(result: &mut pb::GrepUnionResult, budget: &mut GrepBudget) {
    use pb::grep_union_result::Result;
    match result.result.as_mut() {
        Some(Result::Content(content)) => gate_grep_content(content, budget),
        Some(Result::Files(files)) => {
            let original = files.files.len();
            if original > GREP_LIST_LIMIT {
                files.files.truncate(GREP_LIST_LIMIT);
                files.client_truncated = true;
            }
            if files.total_files <= 0 {
                files.total_files = original as i32;
            }
        }
        Some(Result::Count(counts)) => {
            let original = counts.counts.len();
            if original > GREP_LIST_LIMIT {
                counts.counts.truncate(GREP_LIST_LIMIT);
                counts.client_truncated = true;
            }
            if counts.total_files <= 0 {
                counts.total_files = original as i32;
            }
        }
        None => {}
    }
}

pub(super) fn gate_grep_content(content: &mut pb::GrepContentResult, budget: &mut GrepBudget) {
    if content
        .matches
        .iter()
        .flat_map(|file| &file.matches)
        .any(is_grep_notice)
    {
        return;
    }
    let original_bytes = grep_content_bytes(&content.matches);
    let original_files = content.matches.len();
    let mut truncated = false;
    let mut files = Vec::with_capacity(original_files);

    for file in &content.matches {
        if budget.matches == 0 || budget.content_bytes == 0 {
            truncated = true;
            break;
        }
        let mut next = pb::GrepFileMatch {
            file: file.file.clone(),
            matches: Vec::new(),
        };
        for matched in &file.matches {
            if is_grep_notice(matched) {
                next.matches.push(matched.clone());
                continue;
            }
            if next.matches.len() >= GREP_MATCHES_PER_FILE
                || budget.matches == 0
                || budget.content_bytes == 0
            {
                truncated = true;
                break;
            }
            let mut next_match = matched.clone();
            let original = next_match.content.clone();
            next_match.content = truncate_text("Grep match", &original, GREP_MATCH_LIMIT);
            if next_match.content != original {
                next_match.content_truncated = true;
                truncated = true;
            }
            if next_match.content.len() > budget.content_bytes {
                next_match.content =
                    truncate_text("Grep", &next_match.content, budget.content_bytes);
                next_match.content_truncated = true;
                truncated = true;
            }
            if next_match.content.trim().is_empty() {
                truncated = true;
                break;
            }
            budget.content_bytes = budget
                .content_bytes
                .saturating_sub(next_match.content.len());
            budget.matches -= 1;
            next.matches.push(next_match);
        }
        if next.matches.len() < file.matches.len() {
            truncated = true;
        }
        if !next.matches.is_empty() {
            files.push(next);
        }
    }
    if files.len() < original_files {
        truncated = true;
    }
    if truncated {
        content.client_truncated = true;
        add_grep_notice(&mut files, original_bytes);
    }
    content.matches = files;
}

pub(super) fn add_grep_notice(files: &mut Vec<pb::GrepFileMatch>, original_bytes: usize) {
    if files
        .iter()
        .flat_map(|file| &file.matches)
        .any(is_grep_notice)
    {
        return;
    }
    loop {
        let used = grep_content_bytes(files);
        let notice = truncation_notice("Grep", GREP_CONTENT_LIMIT, used, original_bytes);
        if used.saturating_add(notice.len()) <= GREP_CONTENT_LIMIT {
            let matched = pb::GrepContentMatch {
                line_number: 0,
                content: notice,
                content_truncated: true,
                is_context_line: true,
            };
            if let Some(file) = files.last_mut() {
                file.matches.push(matched);
            } else {
                files.push(pb::GrepFileMatch {
                    file: "[truncated]".into(),
                    matches: vec![matched],
                });
            }
            return;
        }
        let Some(file) = files.last_mut() else {
            return;
        };
        file.matches.pop();
        if file.matches.is_empty() {
            files.pop();
        }
    }
}

pub(super) fn is_grep_notice(matched: &pb::GrepContentMatch) -> bool {
    matched.line_number == 0
        && matched.content_truncated
        && matched
            .content
            .starts_with("[truncated: Grep result exceeded")
}

pub(super) fn grep_content_bytes(files: &[pb::GrepFileMatch]) -> usize {
    files
        .iter()
        .flat_map(|file| &file.matches)
        .map(|matched| matched.content.len())
        .sum()
}
