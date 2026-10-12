//! Implements `SearchConversations` over the conversations Nexusor already stores.
//!
//! Cursor's own tool runs inside its agent host against Cursor's local index.
//! Nexusor keeps the canonical history for every run in `messages`, so the same
//! question is answered from the local store instead: no index to maintain, and
//! results match what the agent actually saw.
//!
//! The wire shape matches Cursor's: `ConversationSearchArgs{query, limit}` and a
//! `ConversationSearchHit` per matching conversation.
use serde_json::Value;

use crate::{
    cursor::protocol::proto::agent::v1 as pb,
    model::{ToolCall, ToolResult},
    Result,
};

use super::{now_ms, ToolCompletion};

/// Cursor's own default; kept so the tool behaves the same on both sides.
const DEFAULT_LIMIT: usize = 20;
/// Cursor's own schema caps this at 100.
const MAX_LIMIT: usize = 100;
const SNIPPET_CHARS: usize = 240;

/// Every hit here comes from the local store, so Cursor's `source` label is
/// always `local` (`LOCAL = 1` in `agent.v1`; `0` would read as `unknown`).
const SOURCE_LOCAL: i32 = 1;
const SOURCE_LABEL: &str = "local";

struct Hit {
    conversation_id: String,
    title: String,
    updated_at_ms: i64,
    snippet: String,
}

/// Folds Turkish diacritics so a query typed without them still matches stored
/// text: `guvenlik` has to find `güvenlik`. This mirrors how a person types.
fn fold(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            'ç' | 'Ç' => out.push('c'),
            'ğ' | 'Ğ' => out.push('g'),
            'ı' | 'İ' | 'i' | 'I' => out.push('i'),
            'ö' | 'Ö' => out.push('o'),
            'ş' | 'Ş' => out.push('s'),
            'ü' | 'Ü' => out.push('u'),
            other => out.extend(other.to_lowercase()),
        }
    }
    out
}

/// Pulls the readable text out of a stored message.
///
/// A canonical message stores its body under `content`, which is either a plain
/// string or `{"kind":"parts","parts":[{"text":...}]}`. Walking every value of
/// that object would also pick up structural members such as the `"parts"`
/// discriminator itself, which then became the conversation title, so only the
/// content-bearing keys are followed.
fn body_of(payload: &Value, out: &mut String) {
    match payload.get("content") {
        Some(Value::String(text)) => out.push_str(text),
        Some(content) => collect_text(content, out),
        None => {
            if let Some(text) = payload.get("text").and_then(Value::as_str) {
                out.push_str(text);
            }
        }
    }
}

fn collect_text(value: &Value, out: &mut String) {
    match value {
        Value::String(text) => out.push_str(text),
        Value::Array(items) => {
            for item in items {
                collect_text(item, out);
            }
        }
        Value::Object(map) => {
            if let Some(text) = map.get("text").and_then(Value::as_str) {
                out.push_str(text);
                out.push('\n');
            } else if let Some(parts) = map.get("parts") {
                // `kind`, `type` and friends describe the shape, not the content.
                collect_text(parts, out);
            }
        }
        _ => {}
    }
}

/// The part of a stored turn a person would recognise, and whether it came from
/// something they typed.
///
/// A turn is one long text: environment, rules, skills, tool manifest, then the
/// request. Both the title and the snippet are drawn from this, so neither can
/// quote `</timestamp>` or `<user_info>` the way the raw body made them.
struct Readable {
    text: String,
    from_query: bool,
}

fn readable(payload: &Value) -> Readable {
    let mut body = String::new();
    body_of(payload, &mut body);

    let queries = crate::cursor::text::blocks_content(&body, "user_query");
    let mut text = crate::cursor::text::strip_all_tags(&body);
    for query in &queries {
        text.push('\n');
        text.push_str(query);
    }
    let free = crate::cursor::text::collapse_blank_lines(&text);

    Readable {
        from_query: !queries.is_empty(),
        text: if free.trim().is_empty() {
            queries.join("\n")
        } else {
            free
        },
    }
}

/// Where a title came from.
///
/// The distinction matters: every conversation opens on a turn that is entirely
/// injected context, and the first real request only appears on a later turn.
/// A title lifted from that opening turn describes the environment, not the work.
enum Title {
    /// Text the person actually typed, inside `<user_query>`.
    Query(String),
    /// Whichever text survived once the wrappers were removed.
    Other(String),
    /// Nothing usable: the turn held nothing but injected context.
    None,
}

/// Renders a short, human-readable title from one stored user turn.
fn title_from(payload: &Value) -> Title {
    let read = readable(payload);
    match first_line(&read.text) {
        Some(line) if read.from_query => Title::Query(shorten(line)),
        Some(line) => Title::Other(shorten(line)),
        None => Title::None,
    }
}

fn first_line(text: &str) -> Option<&str> {
    text.lines().map(str::trim).find(|line| !line.is_empty())
}

fn shorten(line: &str) -> String {
    let mut chars = line.chars();
    let short: String = chars.by_ref().take(80).collect();
    if chars.next().is_some() {
        format!("{}…", short.trim_end())
    } else {
        short
    }
}

/// Short form of a UUID, used only when no turn yielded a usable title.
fn short_id(conversation_id: &str) -> String {
    conversation_id
        .split('-')
        .next()
        .unwrap_or(conversation_id)
        .to_string()
}

/// Case-insensitive keyword match.
///
/// Cursor states the rule plainly: every unquoted keyword must match the same
/// conversation, so a long query returns nothing. Splitting on whitespace and
/// requiring all terms keeps that behaviour instead of silently OR-ing them.
fn matches(haystack: &str, terms: &[String]) -> bool {
    let lowered = fold(haystack);
    terms.iter().all(|term| lowered.contains(term))
}

fn snippet_for(body: &str, terms: &[String]) -> String {
    let chars: Vec<char> = body.chars().collect();
    let lowered: Vec<char> = fold(body).chars().collect();
    let position = terms
        .iter()
        .find_map(|term| {
            let needle: Vec<char> = term.chars().collect();
            lowered
                .windows(needle.len())
                .position(|window| window == needle.as_slice())
        })
        .unwrap_or(0);
    let start = position.saturating_sub(SNIPPET_CHARS / 3);
    let end = (start + SNIPPET_CHARS).min(chars.len());
    let slice: String = chars[start..end].iter().collect();
    let prefix = if start > 0 { "…" } else { "" };
    let suffix = if end < chars.len() { "…" } else { "" };
    format!("{prefix}{}{suffix}", slice.trim())
}

async fn search(call: &ToolCall, store: &crate::store::Store) -> Result<(Vec<Hit>, bool)> {
    let query = call
        .arguments
        .get("query")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    let limit = call
        .arguments
        .get("limit")
        .and_then(Value::as_u64)
        .map(|value| (value as usize).clamp(1, MAX_LIMIT))
        .unwrap_or(DEFAULT_LIMIT);
    if query.is_empty() {
        return Ok((Vec::new(), false));
    }
    let terms: Vec<String> = query
        .split_whitespace()
        .map(|term| fold(term.trim_matches('"')))
        .filter(|term| !term.is_empty())
        .collect();
    if terms.is_empty() {
        return Ok((Vec::new(), false));
    }

    let rows: Vec<(String, String, Value, i64)> = sqlx::query_as(
        "SELECT conversation_id, role, payload_json, created_at_ms
         FROM messages ORDER BY created_at_ms DESC LIMIT 5000",
    )
    .fetch_all(&store.pool)
    .await
    .map_err(|error| crate::Error::Store(format!("conversation search: {error}")))?;

    // Rows arrive newest first, so a title is only kept when it comes from an
    // earlier turn than the one already held. Two maps are needed because a
    // conversation opens on a fully injected turn: keeping the earliest title of
    // any single kind would prefer that noise over the first real query below it.
    let mut query_titles: std::collections::HashMap<String, (i64, String)> =
        std::collections::HashMap::new();
    let mut spare_titles: std::collections::HashMap<String, (i64, String)> =
        std::collections::HashMap::new();
    let mut best: std::collections::HashMap<String, (i64, String)> =
        std::collections::HashMap::new();
    for (conversation_id, role, payload, created_at_ms) in rows {
        if role == "user" {
            match title_from(&payload) {
                Title::Query(title) => {
                    let slot = query_titles
                        .entry(conversation_id.clone())
                        .or_insert((created_at_ms, title.clone()));
                    if created_at_ms < slot.0 {
                        *slot = (created_at_ms, title);
                    }
                }
                Title::Other(title) => {
                    let slot = spare_titles
                        .entry(conversation_id.clone())
                        .or_insert((created_at_ms, title.clone()));
                    if created_at_ms < slot.0 {
                        *slot = (created_at_ms, title);
                    }
                }
                Title::None => {}
            }
        }
        // Matching runs on the readable text, not the raw body: 22 000
        // characters of environment, rule and tool markup sit in front of the
        // sentence a person wrote, so a match there points at scaffolding.
        let read = readable(&payload);
        if !matches(&read.text, &terms) {
            continue;
        }
        let entry = best.entry(conversation_id).or_insert((0, String::new()));
        if created_at_ms > entry.0 {
            entry.0 = created_at_ms;
            entry.1 = snippet_for(&read.text, &terms);
        }
    }

    let mut hits: Vec<Hit> = best
        .into_iter()
        .map(|(conversation_id, (updated_at_ms, snippet))| {
            // A typed query always outranks leftover prose, however early the
            // leftover sits, because only the query describes the work.
            let title = query_titles
                .get(&conversation_id)
                .or_else(|| spare_titles.get(&conversation_id))
                .map(|(_, title)| title.clone())
                .unwrap_or_else(|| {
                    format!("(untitled conversation {})", short_id(&conversation_id))
                });
            Hit {
                conversation_id,
                title,
                updated_at_ms,
                snippet,
            }
        })
        .collect();
    hits.sort_by_key(|hit| std::cmp::Reverse(hit.updated_at_ms));
    let truncated = hits.len() > limit;
    hits.truncate(limit);
    Ok((hits, truncated))
}

/// Reads the transcript folder back out of a stored turn.
///
/// The client sends `agent_transcripts_folder` in its environment details and
/// the compiled turn quotes the path verbatim, so the same folder the writer
/// used can be recovered per conversation instead of reconstructed.
fn transcripts_folder_of(payload: &Value) -> Option<String> {
    let mut body = String::new();
    body_of(payload, &mut body);
    let after = body.split(FOLDER_SENTENCE).nth(1)?;
    // The path runs to the first whitespace, which is the period ending
    // "…/agent-transcripts. They have names…". Cutting at the first `.` would
    // be wrong: a Windows path legitimately contains `.cursor`.
    let word: String = after
        .trim_start()
        .chars()
        .take_while(|c| !c.is_whitespace())
        .collect();
    let folder = word.trim_end_matches('.');
    (!folder.is_empty()).then(|| folder.to_string())
}

/// The literal the client-sent folder is quoted with, from the turn Nexusor
/// compiles. Reading it back is what lets a stored conversation be matched to
/// the folder its writer used, without reconstructing a workspace path.
const FOLDER_SENTENCE: &str = "Agent transcripts (past chats) live in";

/// Whether a conversation can be opened when the model cites it.
///
/// Cursor resolves a chat link by opening the transcript file, and reports
/// "Chat not found" when there is none. A hit that cannot be opened is worse
/// than a missing hit, because the model presents it as usable.
fn openable(hit: &Hit, folders: &[(String, String)]) -> bool {
    folders.iter().any(|(conversation_id, folder)| {
        conversation_id == &hit.conversation_id
            && crate::cursor::conversation::transcript::exists(folder, &hit.conversation_id)
    })
}

/// Renders hits exactly the way Cursor's own `SearchConversations` does.
///
/// The shape was read out of `cursor-agent-host/dist/main.js` rather than
/// invented here, because the model reads this text as its only source of
/// conversation ids and a different layout is one it rewrites from memory — it
/// produced links that opened "chat not found".
fn render_summary(hits: &[Hit], truncated: bool) -> String {
    if hits.is_empty() {
        return "No matching conversations found.".to_string();
    }
    let mut lines = vec![format!(
        "Found {} conversation{}:",
        hits.len(),
        if hits.len() == 1 { "" } else { "s" }
    )];
    for (index, hit) in hits.iter().enumerate() {
        lines.push(format!(
            "{}. [{}] \"{}\" (id: {}, updated: {})",
            index + 1,
            SOURCE_LABEL,
            hit.title,
            hit.conversation_id,
            iso_or_unknown(hit.updated_at_ms)
        ));
        if !hit.snippet.is_empty() {
            lines.push(format!("   snippet: {}", hit.snippet));
        }
    }
    if truncated {
        lines.push("Additional matches were omitted because the result limit was reached.".into());
    }
    lines.join("\n")
}

/// `new Date(ms).toISOString()`, or `unknown` for a zero timestamp.
fn iso_or_unknown(updated_at_ms: i64) -> String {
    if updated_at_ms == 0 {
        return "unknown".to_string();
    }
    match chrono::DateTime::from_timestamp_millis(updated_at_ms) {
        Some(moment) => moment.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        None => "unknown".to_string(),
    }
}

pub(crate) async fn search_conversations(
    call: &ToolCall,
    store: Option<&crate::store::Store>,
) -> Result<ToolCompletion> {
    let Some(store) = store else {
        return Err(crate::Error::Config(
            "SearchConversations requires the conversation store".into(),
        ));
    };
    let (mut hits, truncated) = search(call, store).await?;

    // Only offer conversations that will actually open. The folder a
    // conversation lives in is the one its own turns reported, so it is read
    // back out of the stored messages rather than guessed from a path.
    let rows: Vec<(String, Value)> =
        sqlx::query_as("SELECT conversation_id, payload_json FROM messages WHERE role = 'user'")
            .fetch_all(&store.pool)
            .await
            .map_err(|error| crate::Error::Store(format!("conversation folders: {error}")))?;
    let mut folders: Vec<(String, String)> = Vec::new();
    for (conversation_id, payload) in rows {
        if let Some(folder) = transcripts_folder_of(&payload) {
            if !folders
                .iter()
                .any(|(known, _): &(String, String)| known == &conversation_id)
            {
                folders.push((conversation_id, folder));
            }
        }
    }
    let total = hits.len();
    hits.retain(|hit| openable(hit, &folders));
    let truncated = truncated || hits.len() < total;
    let summary = render_summary(&hits, truncated);

    Ok(ToolCompletion::new(
        call,
        now_ms(),
        ToolResult {
            call_id: call.call_id.clone(),
            content: summary,
            is_error: false,
            image: None,
            images: Vec::new(),
        },
        pb::tool_call::Tool::SearchConversationsToolCall(pb::SearchConversationsToolCall {
            args: Some(pb::ConversationSearchArgs {
                query: call
                    .arguments
                    .get("query")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                tool_call_id: call.call_id.clone(),
                limit: call
                    .arguments
                    .get("limit")
                    .and_then(Value::as_u64)
                    .map(|value| value as i32),
            }),
            result: Some(pb::ConversationSearchResult {
                result: Some(pb::conversation_search_result::Result::Success(
                    pb::ConversationSearchSuccess {
                        hits: hits
                            .iter()
                            .map(|hit| pb::ConversationSearchHit {
                                conversation_id: hit.conversation_id.clone(),
                                title: hit.title.clone(),
                                source: SOURCE_LOCAL,
                                updated_at_ms: hit.updated_at_ms,
                                snippet: (!hit.snippet.is_empty()).then(|| hit.snippet.clone()),
                            })
                            .collect(),
                        truncated,
                        partial: false,
                        rebuilding: false,
                    },
                )),
            }),
        }),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turkish_diacritics_fold_so_plain_keywords_still_match() {
        assert_eq!(fold("Güvenlik İncelemesi"), "guvenlik incelemesi");
        assert!(matches(
            "güvenlik incelemesi hazırlandı",
            &[fold("guvenlik"), fold("inceleme")]
        ));
    }

    #[test]
    fn every_keyword_must_match_the_same_conversation() {
        let terms = vec![fold("guvenlik"), fold("inceleme")];
        assert!(matches("guvenlik incelemesi raporu", &terms));
        assert!(!matches("guvenlik raporu", &terms));
    }

    #[test]
    fn a_partshaped_payload_yields_text_not_json() {
        let payload = serde_json::json!({
            "content": {
                "kind": "parts",
                "parts": [
                    { "type": "text", "text": "gercek kullanici sorusu" },
                    { "type": "text", "text": "ikinci parca" }
                ]
            }
        });
        let mut body = String::new();
        body_of(&payload, &mut body);
        assert!(body.contains("gercek kullanici sorusu"));
        assert!(!body.contains("kind"), "raw json leaked: {body}");
    }

    /// A turn shaped like the real ones: injected context only, no query.
    /// It must yield no title at all rather than a wrapper name.
    #[test]
    fn a_fully_injected_turn_yields_no_title() {
        let payload = serde_json::json!({
            "content": { "kind": "parts", "parts": [{
                "type": "text",
                "text": "<user_info>\nOS Version: win32\n</user_info>\
                         \n<user_rule>\nkural metni\n</user_rule>\
                         \n<rules>\n- kural iki\n</rules>\
                         \n<tools>\nLS, Read, Edit\n</tools>"
            }]}
        });
        assert!(matches!(title_from(&payload), Title::None));
    }

    /// This is the shape that actually opens every conversation: injected
    /// context, then the typed request.
    #[test]
    fn a_real_shaped_turn_is_titled_from_the_typed_query() {
        let payload = serde_json::json!({
            "content": { "kind": "parts", "parts": [{
                "type": "text",
                "text": "<user_info>\nOS Version: win32\nWorkspace Path: C:\\x\n</user_info>\
                         \n<system_reminder>\nmod bilgisi\n</system_reminder>\
                         \n<user_query>\nSearch my earlier conversations for: guvenlik inceleme\n</user_query>"
            }]}
        });
        match title_from(&payload) {
            Title::Query(title) => {
                assert_eq!(
                    title,
                    "Search my earlier conversations for: guvenlik inceleme"
                )
            }
            _ => panic!("the typed query should win"),
        }
    }

    /// Free text outside every known wrapper is still better than nothing.
    #[test]
    fn a_turn_without_a_query_falls_back_past_the_wrappers() {
        let payload = serde_json::json!({
            "content": { "kind": "parts", "parts": [{
                "type": "text",
                "text": "<user_info>\nOS Version: win32\n</user_info>\n\
                         <user_rule>\nkural metni\n</user_rule>\n\
                         <rules>\n- kural iki\n</rules>\n\
                         dosyalari tara ve rapor yaz"
            }]}
        });
        match title_from(&payload) {
            Title::Other(title) => assert_eq!(title, "dosyalari tara ve rapor yaz"),
            _ => panic!("leftover prose should be usable"),
        }
    }

    /// A wrapper this list has never heard of must not become the title.
    #[test]
    fn an_unknown_wrapper_is_not_offered_as_a_title() {
        let payload = serde_json::json!({
            "content": { "kind": "parts", "parts": [{
                "type": "text",
                "text": "<git_status>\nmodified: a.rs\n</git_status>\n\
                         <branch_status>\nmain\n</branch_status>"
            }]}
        });
        assert!(matches!(title_from(&payload), Title::None));
    }

    /// An unterminated block must not leak its opening tag into a title.
    #[test]
    fn an_unterminated_injected_block_is_dropped_not_shown() {
        let stripped = crate::cursor::text::strip_all_tags("<user_info>\nOS Version: win32\n");
        assert!(!stripped.contains("OS Version"), "leaked: {stripped}");
        assert!(!stripped.contains("user_info"), "leaked: {stripped}");
    }

    /// Prose with angle brackets in it is not markup and must survive intact.
    #[test]
    fn angle_brackets_in_prose_are_not_treated_as_tags() {
        let text = "if a < b and c > d then compare";
        assert_eq!(crate::cursor::text::strip_all_tags(text), text);
    }

    /// The exact shape that reached the model: an unbalanced `</timestamp>` in
    /// front of the sentence. Dropping it must not swallow the sentence with it.
    #[test]
    fn a_stray_closing_tag_is_dropped_without_truncating() {
        let payload = serde_json::json!({
            "content": { "kind": "parts", "parts": [{
                "type": "text",
                "text": "10:12 AM (UTC+3)</timestamp>\n\
                         <user_query>\nraporu yaz\n</user_query>"
            }]}
        });
        let read = readable(&payload);
        assert!(!read.text.contains("timestamp"), "leaked: {}", read.text);
        assert!(read.text.contains("raporu yaz"), "truncated: {}", read.text);
    }

    /// The reported defect: a snippet drawn from the raw body quoted the
    /// scaffolding instead of the request.
    #[test]
    fn a_snippet_never_shows_injected_markup() {
        let payload = serde_json::json!({
            "content": { "kind": "parts", "parts": [{
                "type": "text",
                "text": "<user_info>\nOS Version: win32\n</user_info>\
                         \n<timestamp>\nMonday\n</timestamp>\
                         \n<user_query>\nSearch my earlier conversations for: \
                         guvenlik inceleme\n</user_query>"
            }]}
        });
        let read = readable(&payload);
        assert!(read.from_query);
        let snippet = snippet_for(&read.text, &[fold("guvenlik")]);
        assert!(
            !snippet.contains('<') && !snippet.contains('>'),
            "markup leaked: {snippet}"
        );
        assert!(
            snippet.contains("guvenlik inceleme"),
            "lost the match: {snippet}"
        );
    }

    /// Wrappers are removed even when the name is one this code never saw.
    #[test]
    fn readable_text_keeps_the_request_and_drops_the_scaffolding() {
        let payload = serde_json::json!({
            "content": { "kind": "parts", "parts": [{
                "type": "text",
                "text": "<git_status>\nmodified: a.rs\n</git_status>\
                         \n<user_query>\nraporu yaz\n</user_query>"
            }]}
        });
        let read = readable(&payload);
        assert_eq!(read.text.trim(), "raporu yaz");
    }

    #[test]
    fn a_long_title_is_truncated_with_an_ellipsis() {
        let long = "a".repeat(200);
        let titled = shorten(&long);
        assert_eq!(titled.chars().count(), 81);
        assert!(titled.ends_with('…'));
    }

    #[test]
    fn an_untitled_conversation_still_names_itself_by_id() {
        assert_eq!(short_id("495fe91c-d7e0-4898-839b-70f9d1ae0dd4"), "495fe91c");
    }

    /// The layout below was read out of `cursor-agent-host/dist/main.js`. If it
    /// drifts, the model starts rewriting ids and produces dead links again.
    #[test]
    fn the_layout_matches_cursors_own_renderer() {
        let hit = Hit {
            conversation_id: "495fe91c-d7e0-4898-839b-70f9d1ae0dd4".into(),
            title: "Guvenlik incelemesi".into(),
            updated_at_ms: 0,
            snippet: "SQL injection tespit edildi".into(),
        };
        let rendered = render_summary(std::slice::from_ref(&hit), false);
        assert_eq!(
            rendered,
            "Found 1 conversation:\n\
             1. [local] \"Guvenlik incelemesi\" \
             (id: 495fe91c-d7e0-4898-839b-70f9d1ae0dd4, updated: unknown)\n   \
             snippet: SQL injection tespit edildi"
        );
    }

    #[test]
    fn several_hits_are_numbered_and_the_plural_is_used() {
        let hits: Vec<Hit> = ["a", "b"]
            .iter()
            .map(|id| Hit {
                conversation_id: format!("{id}-id"),
                title: format!("{id} basligi"),
                updated_at_ms: 1_700_000_000_000,
                snippet: String::new(),
            })
            .collect();
        let rendered = render_summary(&hits, false);
        assert!(rendered.starts_with("Found 2 conversations:"), "{rendered}");
        assert!(rendered.contains("1. [local] \"a basligi\" (id: a-id, updated: "));
        assert!(rendered.contains("2. [local] \"b basligi\" (id: b-id, updated: "));
        // Cursor writes an ISO stamp with millisecond precision.
        assert!(rendered.contains("2023-11-14T"), "{rendered}");
        assert!(!rendered.contains("snippet:"), "empty snippets are skipped");
    }

    #[test]
    fn no_hits_says_so_in_cursors_words() {
        assert_eq!(
            render_summary(&[], false),
            "No matching conversations found."
        );
    }

    #[test]
    fn a_dropped_result_set_is_reported_rather_than_hidden() {
        let hits = vec![Hit {
            conversation_id: "a-id".into(),
            title: "a".into(),
            updated_at_ms: 0,
            snippet: String::new(),
        }];
        assert!(render_summary(&hits, true)
            .ends_with("Additional matches were omitted because the result limit was reached."));
    }

    #[test]
    fn every_hit_is_claimed_to_come_from_the_local_store() {
        assert_eq!(SOURCE_LOCAL, 1, "agent.v1: LOCAL = 1, UNSPECIFIED = 0");
    }

    /// The folder is quoted verbatim in the compiled turn, so it can be read
    /// back instead of reconstructed from a workspace path.
    #[test]
    fn the_transcript_folder_is_read_back_out_of_the_turn() {
        let payload = serde_json::json!({
            "content": { "kind": "parts", "parts": [{
                "type": "text",
                "text": "<agent_transcripts>\nAgent transcripts (past chats) live in \
                         C:\\Users\\x\\.cursor\\projects\\p/agent-transcripts. \
                         They have names like <uuid>.jsonl\n</agent_transcripts>"
            }]}
        });
        assert_eq!(
            transcripts_folder_of(&payload).as_deref(),
            Some(r"C:\Users\x\.cursor\projects\p/agent-transcripts")
        );
    }

    #[test]
    fn a_turn_without_a_transcript_folder_yields_none() {
        let payload = serde_json::json!({
            "content": { "kind": "parts", "parts": [{ "type": "text", "text": "merhaba" }]}
        });
        assert_eq!(transcripts_folder_of(&payload), None);
    }

    /// A hit whose transcript is missing must not be offered: the model cites it
    /// as usable and the link then reports "Chat not found".
    #[test]
    fn a_hit_is_only_offered_when_its_transcript_exists() {
        let hit = Hit {
            conversation_id: "cid".into(),
            title: "t".into(),
            updated_at_ms: 0,
            snippet: String::new(),
        };
        let dir = std::env::temp_dir().join(format!("nexusor-openable-{}", std::process::id()));
        let folder = dir.to_string_lossy().to_string();
        crate::cursor::conversation::transcript::write(&folder, "cid", &[]).expect("write");

        assert!(openable(&hit, &[("cid".into(), folder.clone())]));
        assert!(!openable(&hit, &[("cid".into(), "C:/yok".into())]));
        assert!(!openable(&hit, &[("baska".into(), folder)]));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
