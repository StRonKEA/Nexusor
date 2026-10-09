//! Keeps oversized current text available locally without rewriting canonical history.
use sha2::{Digest, Sha256};

use crate::model::{ContentPart, PreparedRun, ProjectedContent, ProjectedMessage, Role};

pub(super) async fn prepare(
    prepared: &PreparedRun,
    mut history: Vec<ProjectedMessage>,
    provider_rejected: bool,
) -> crate::Result<Vec<ProjectedMessage>> {
    let Some(budget) = super::compaction::input_budget(prepared) else {
        return Ok(history);
    };
    // Current input must leave space for tools, results and the previous summary.
    // Use bytes here: character/token heuristics underestimate some Unicode.
    let threshold = usize::try_from(budget / 2).unwrap_or(usize::MAX).max(4096);
    let current = crate::model::project_messages(&prepared.initial_messages)?;
    if !provider_rejected
        && crate::model::estimate_context_tokens(&prepared.prompt, &current) <= budget
    {
        return Ok(history);
    }
    for message in &mut history {
        if message.role != Role::User
            || !prepared
                .initial_messages
                .iter()
                .any(|m| m.message_id == message.message_id)
        {
            continue;
        }
        let ProjectedContent::Parts(parts) = &mut message.content else {
            continue;
        };
        for part in parts {
            let ContentPart::Text { text } = part else {
                continue;
            };
            if text.len() <= threshold {
                continue;
            }
            if !prepared
                .prompt
                .tools
                .iter()
                .any(|tool| matches!(tool.name.as_str(), "Read" | "Shell"))
            {
                return Err(crate::Error::Protocol(format!(
                    "Current input exceeds the {budget}-token input budget and this mode has no Read/Shell tool. Original input remains in conversation history. Split the input, attach a local file in Agent mode, or select a larger-context model."
                )));
            }
            let directory = crate::config::managed_data_dir()?.join("large-inputs");
            *text = archive_text(&directory, text).await?;
        }
    }
    Ok(history)
}

/// Archives older files once the directory grows past the budget.
///
/// Archives are addressed by content digest, so dropping the oldest ones cannot
/// invalidate a notice that still points at an existing file.
fn prune_archives(directory: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    let mut files: Vec<(std::time::SystemTime, std::path::PathBuf, u64)> = Vec::new();
    let mut total = 0u64;
    for entry in entries.flatten() {
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if !metadata.is_file() {
            continue;
        }
        total += metadata.len();
        files.push((
            metadata.modified().unwrap_or(std::time::UNIX_EPOCH),
            entry.path(),
            metadata.len(),
        ));
    }
    if total <= ARCHIVE_MAX_BYTES {
        return;
    }
    files.sort_by_key(|(modified, _, _)| *modified);
    for (_, path, size) in &files {
        if total <= ARCHIVE_MAX_BYTES {
            break;
        }
        if std::fs::remove_file(path).is_ok() {
            total = total.saturating_sub(*size);
        }
    }
}

const ARCHIVE_MAX_BYTES: u64 = 256 * 1024 * 1024;

async fn archive_text(directory: &std::path::Path, text: &str) -> crate::Result<String> {
    use tokio::io::AsyncWriteExt;
    tokio::fs::create_dir_all(directory).await?;
    prune_archives(directory);
    let digest = format!("{:x}", Sha256::digest(text.as_bytes()));
    let path = directory.join(format!("{digest}.txt"));
    match tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .await
    {
        Ok(mut file) => {
            file.write_all(text.as_bytes()).await?;
            file.flush().await?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            // Never silently trust a partial or externally modified archive.
            if tokio::fs::read(&path).await? != text.as_bytes() {
                return Err(crate::Error::Protocol("Large-input archive verification failed; original conversation input is preserved".into()));
            }
        }
        Err(error) => return Err(error.into()),
    }
    let mut head = text.len().min(1024);
    while !text.is_char_boundary(head) {
        head -= 1;
    }
    let mut tail = text.len().saturating_sub(1024).max(head);
    while !text.is_char_boundary(tail) {
        tail += 1;
    }
    Ok(format!(
        "<large_user_input>\nThe current user input is too large to include inline. Its complete original UTF-8 text is preserved locally at: {}\nBytes: {}. SHA-256: {digest}.\nOnly a head/tail preview follows; the omitted middle has NOT been read or summarized. Use Read with bounded offsets/limits, or Shell for bounded extraction/search (especially for one long line), to inspect the task instructions and relevant content before acting. Do not read the entire file in one tool result. If the task needs complete coverage, process it in chunks and track coverage; never claim unseen content was processed. Explain this file-based handling and any remaining coverage limitations to the user.\n<preview_head>\n{}\n</preview_head>\n[... unread middle omitted ...]\n<preview_tail>\n{}\n</preview_tail>\n</large_user_input>",
        path.display(), text.len(), &text[..head], &text[tail..]
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn archive_preserves_unicode_middle_and_detects_modified_files() {
        let directory = tempfile::tempdir().unwrap();
        let text = format!(
            "START{}MIDDLE_SECRET{}END",
            "ğ🦀".repeat(3000),
            "ç🦀".repeat(3000)
        );
        let notice = archive_text(directory.path(), &text).await.unwrap();
        assert!(notice.contains("START") && notice.contains("END"));
        assert!(!notice.contains("MIDDLE_SECRET"));
        assert!(notice.contains("NOT been read") && notice.contains("bounded"));
        let path = directory
            .path()
            .join(format!("{:x}.txt", Sha256::digest(text.as_bytes())));
        assert_eq!(tokio::fs::read_to_string(&path).await.unwrap(), text);
        assert_eq!(archive_text(directory.path(), &text).await.unwrap(), notice);
        tokio::fs::write(&path, "modified").await.unwrap();
        assert!(archive_text(directory.path(), &text).await.is_err());
    }

    #[tokio::test]
    async fn fitting_input_is_unchanged_and_toolless_overflow_is_actionable() {
        use crate::model::{
            CanonicalMessage, CheckpointId, ConversationId, ModelSpec, Origin, PromptSpec,
            RunAction, RunId, RunKind,
        };
        let mut model = ModelSpec::new("model");
        model.context_window_tokens = Some(8000);
        let mut prepared = PreparedRun {
            run_id: RunId::new("test"),
            cursor_request_id: None,
            conversation_id: ConversationId::new("test"),
            kind: RunKind::Root,
            model,
            prompt: PromptSpec {
                instructions: String::new(),
                tools: vec![],
            },
            initial_messages: vec![CanonicalMessage::text(
                "input",
                Role::User,
                Origin::User,
                "fitting input",
            )],
            action: RunAction::Start,
            base_checkpoint_id: CheckpointId(1),
        };
        let history = crate::model::project_messages(&prepared.initial_messages).unwrap();
        assert_eq!(
            prepare(&prepared, history.clone(), false).await.unwrap(),
            history
        );
        prepared.initial_messages[0] =
            CanonicalMessage::text("input", Role::User, Origin::User, "ğ".repeat(40000));
        let history = crate::model::project_messages(&prepared.initial_messages).unwrap();
        let error = prepare(&prepared, history, false)
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("no Read/Shell") && error.contains("Split the input"));
    }
}
