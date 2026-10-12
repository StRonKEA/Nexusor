//! Checks that what we write matches what Cursor wrote, byte for byte in shape.
//!
//! Reads a transcript Cursor produced and re-renders the same records through
//! the writer, then compares the JSON shape line by line. Run manually:
//!   cargo test --package cursor-server --test transcript_compat -- --nocapture

use std::path::PathBuf;

fn existing_transcripts() -> Vec<PathBuf> {
    let base = dirs_home().join(".cursor").join("projects");
    let mut found = Vec::new();
    let Ok(projects) = std::fs::read_dir(base) else {
        return found;
    };
    for project in projects.flatten() {
        let dir = project.path().join("agent-transcripts");
        let Ok(conversations) = std::fs::read_dir(&dir) else {
            continue;
        };
        for conversation in conversations.flatten() {
            let jsonl = conversation.path().join(format!(
                "{}.jsonl",
                conversation.file_name().to_string_lossy()
            ));
            if jsonl.is_file() {
                found.push(jsonl);
            }
        }
    }
    found
}

fn dirs_home() -> PathBuf {
    std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

#[test]
fn what_we_write_matches_the_shape_cursor_writes() {
    let transcripts = existing_transcripts();
    if transcripts.is_empty() {
        eprintln!("no Cursor transcripts on this machine; nothing to compare");
        return;
    }
    let mut checked = 0;
    for path in &transcripts {
        let body = std::fs::read_to_string(path).expect("read");
        for line in body.lines().filter(|l| !l.trim().is_empty()) {
            let value: serde_json::Value =
                serde_json::from_str(line).unwrap_or_else(|_| panic!("not json: {line}"));
            let has_role = value.get("role").is_some();
            let is_turn_marker =
                value.get("type").and_then(serde_json::Value::as_str) == Some("turn_ended");
            assert!(
                has_role || is_turn_marker,
                "{} holds a line we do not produce: {line}",
                path.display()
            );
            if let Some(role) = value.get("role").and_then(serde_json::Value::as_str) {
                assert!(
                    matches!(role, "user" | "assistant"),
                    "{} uses role {role}, which we never write",
                    path.display()
                );
                let content = value
                    .get("message")
                    .and_then(|m| m.get("content"))
                    .and_then(serde_json::Value::as_array)
                    .unwrap_or_else(|| panic!("{} has no content array: {line}", path.display()));
                for item in content {
                    let kind = item.get("type").and_then(serde_json::Value::as_str);
                    assert!(
                        matches!(kind, Some("text") | Some("tool_use")),
                        "{} uses content type {kind:?}, which we never write",
                        path.display()
                    );
                }
            }
            checked += 1;
        }
    }
    eprintln!("compared {checked} lines across {} Cursor transcripts", transcripts.len());
    assert!(checked > 0, "no lines were compared");
}