//! Writes the on-disk chat transcript that Cursor resolves a conversation link
//! against.
//!
//! Cursor does not look a conversation up in a database when a chat link is
//! followed. Its link handler tries exactly two things
//! (`workbench.desktop.main.js`, `Eqy`): whether the composer is already open,
//! and whether this file exists:
//!
//! ```text
//! <transcriptsDir>/<conversationId>/<conversationId>.jsonl
//! ```
//!
//! `transcriptsDir` is the `agent_transcripts_folder` the client sends in
//! `EnvironmentDetails`. When neither route succeeds the link reports
//! "Chat not found" — which is what happened to a conversation that existed in
//! the store, appeared in search results with a correct id, and still would not
//! open.
//!
//! The layout and record shape below were read from transcripts Cursor wrote
//! itself, not guessed: one JSON object per line, `{"role", "message"}` for
//! messages and `{"type":"turn_ended","status"}` to close a turn.
//!
//! This is the user's own data directory, written with the same shape Cursor
//! uses. No Cursor program file is touched, and deleting the folder undoes it.

use std::path::{Path, PathBuf};

use crate::model::message::{CanonicalMessage, ContentPart, MessageContent, Role};

/// Builds the transcript path Cursor looks for.
///
/// The nesting matters: Cursor probes `<id>/<id>.jsonl`, not `<id>.jsonl` at
/// the top level.
pub fn transcript_path(folder: &str, conversation_id: &str) -> PathBuf {
    Path::new(folder)
        .join(conversation_id)
        .join(format!("{conversation_id}.jsonl"))
}

/// Renders a conversation as transcript lines.
///
/// Only what Cursor's own transcripts contain is emitted. Tool results have no
/// counterpart in any of the five files Cursor wrote locally, so they are left
/// out rather than shaped by guesswork; the conversation is still fully
/// navigable without them.
pub fn render(messages: &[CanonicalMessage]) -> Vec<String> {
    let mut lines = Vec::new();
    for message in messages {
        match (&message.role, &message.content) {
            (Role::User, MessageContent::Parts { parts }) => {
                let text = readable_text(parts);
                if !text.is_empty() {
                    lines.push(record(
                        "user",
                        vec![serde_json::json!({ "type": "text", "text": text })],
                    ));
                }
            }
            (
                Role::Assistant,
                MessageContent::Assistant {
                    text, tool_calls, ..
                },
            ) => {
                let mut blocks = Vec::new();
                if !text.trim().is_empty() {
                    blocks.push(serde_json::json!({ "type": "text", "text": text.trim() }));
                }
                for call in tool_calls {
                    blocks.push(serde_json::json!({
                        "type": "tool_use",
                        "name": call.name,
                        "input": call.arguments,
                    }));
                }
                if !blocks.is_empty() {
                    lines.push(record("assistant", blocks));
                }
            }
            _ => {}
        }
    }
    lines.push(serde_json::json!({ "type": "turn_ended", "status": "success" }).to_string());
    lines
}

/// The text of a user turn, with Cursor's injected wrappers removed.
///
/// A stored user turn is mostly scaffolding — environment, rules, skills, a
/// tool manifest — and the request sits at the end of it inside
/// `<user_query>`. Writing the raw turn would bury the conversation under tens
/// of thousands of characters of context it does not describe.
fn readable_text(parts: &[ContentPart]) -> String {
    let raw = parts
        .iter()
        .filter_map(|part| match part {
            ContentPart::Text { text } => Some(text.clone()),
            ContentPart::Image { .. } => None,
        })
        .collect::<Vec<_>>()
        .join("\n");

    let queries = crate::cursor::text::blocks_content(&raw, "user_query");
    let mut text = crate::cursor::text::strip_all_tags(&raw);
    for query in &queries {
        text.push('\n');
        text.push_str(query);
    }
    text.trim().to_string()
}

fn record(role: &str, content: Vec<serde_json::Value>) -> String {
    serde_json::json!({
        "role": role,
        "message": { "content": content },
    })
    .to_string()
}

/// Writes the transcript, replacing any previous one.
///
/// The write is staged and renamed so a reader never sees a half-written file,
/// and a failure leaves the old transcript intact.
pub fn write(
    folder: &str,
    conversation_id: &str,
    messages: &[CanonicalMessage],
) -> crate::Result<PathBuf> {
    let path = transcript_path(folder, conversation_id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let body = if messages.is_empty() {
        String::new()
    } else {
        let mut body = render(messages).join("\n");
        body.push('\n');
        body
    };
    let staged = path.with_extension("jsonl.partial");
    std::fs::write(&staged, body)?;
    std::fs::rename(&staged, &path)?;
    Ok(path)
}

/// Whether a conversation can be opened from a link right now.
pub fn exists(folder: &str, conversation_id: &str) -> bool {
    transcript_path(folder, conversation_id).is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(text: &str) -> CanonicalMessage {
        CanonicalMessage::text("m1", Role::User, crate::model::message::Origin::User, text)
    }

    #[test]
    fn the_path_is_nested_the_way_cursor_probes_it() {
        let path = transcript_path("C:/p/agent-transcripts", "abc");
        assert_eq!(path, PathBuf::from("C:/p/agent-transcripts/abc/abc.jsonl"));
    }

    #[test]
    fn a_turn_is_written_without_the_injected_wrappers() {
        let turn = user(
            "<user_info>\nOS Version: win32\n</user_info>\
                        \n<user_query>\nraporu yaz\n</user_query>",
        );
        let lines = render(std::slice::from_ref(&turn));
        assert_eq!(lines.len(), 2, "message plus turn_ended: {lines:?}");
        assert!(lines[0].contains("raporu yaz"), "{}", lines[0]);
        assert!(!lines[0].contains("user_info"), "{}", lines[0]);
    }

    #[test]
    fn every_line_is_one_json_object_with_a_role_or_a_turn_marker() {
        let turn = user("merhaba");
        for line in render(std::slice::from_ref(&turn)) {
            let value: serde_json::Value = serde_json::from_str(&line).expect("valid json line");
            let has_role = value.get("role").is_some();
            let has_turn =
                value.get("type").and_then(serde_json::Value::as_str) == Some("turn_ended");
            assert!(has_role || has_turn, "unrecognised line: {line}");
        }
    }

    #[test]
    fn a_turn_always_ends_with_the_marker_cursor_writes() {
        let lines = render(&[]);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("\"turn_ended\""));
        assert!(lines[0].contains("\"success\""));
    }

    #[test]
    fn writing_leaves_a_readable_file_and_reports_its_path() {
        let dir = std::env::temp_dir().join(format!("nexusor-transcript-{}", std::process::id()));
        let folder = dir.to_string_lossy().to_string();
        let conversation = "11111111-2222-3333-4444-555555555555";
        let path = write(&folder, conversation, &[user("merhaba")]).expect("write");
        assert!(path.is_file(), "missing {}", path.display());
        assert!(exists(&folder, conversation));
        let body = std::fs::read_to_string(&path).expect("read");
        assert_eq!(body.lines().count(), 2, "{body}");
        assert!(
            !body.contains(".partial"),
            "staged file left behind: {body}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_transcript_is_reported_as_missing() {
        assert!(!exists(
            "C:/p/agent-transcripts",
            "nobody-wrote-this-conversation"
        ));
    }
}
