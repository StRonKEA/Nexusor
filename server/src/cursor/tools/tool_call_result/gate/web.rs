//! Trims a WebFetch or WebSearch result, and bounds the reference list a search
//! returns.

use crate::cursor::protocol::proto::agent::v1 as pb;

use super::truncation::{truncate_text, truncation_notice, utf8_prefix};
use super::*;

pub(super) fn gate_web_fetch(tool: &mut pb::WebFetchToolCall) {
    let Some(pb::web_fetch_result::Result::Success(success)) = tool
        .result
        .as_mut()
        .and_then(|result| result.result.as_mut())
    else {
        return;
    };
    success.markdown = truncate_text("WebFetch", &success.markdown, WEB_FETCH_LIMIT);
}

pub(super) fn gate_web_search(tool: &mut pb::WebSearchToolCall) {
    let Some(pb::web_search_result::Result::Success(success)) = tool
        .result
        .as_mut()
        .and_then(|result| result.result.as_mut())
    else {
        return;
    };
    for reference in &mut success.references {
        reference.title =
            truncate_text("WebSearch title", &reference.title, WEB_SEARCH_TITLE_LIMIT);
        reference.chunk = truncate_text(
            "WebSearch snippet",
            &reference.chunk,
            WEB_SEARCH_SNIPPET_LIMIT,
        );
    }
    let original = web_search_bytes(&success.references);
    while success.references.len() > 1 && web_search_bytes(&success.references) > WEB_SEARCH_LIMIT {
        success.references.pop();
    }
    if original > WEB_SEARCH_LIMIT {
        let total = web_search_bytes(&success.references);
        if let Some(reference) = success.references.last_mut() {
            let other = total.saturating_sub(reference.chunk.len());
            let notice = truncation_notice(
                "WebSearch",
                WEB_SEARCH_LIMIT,
                WEB_SEARCH_LIMIT.saturating_sub(other),
                original,
            );
            let available = WEB_SEARCH_LIMIT.saturating_sub(other + notice.len() + 2);
            reference.chunk = format!(
                "{}\n\n{notice}",
                utf8_prefix(&reference.chunk, available).trim_end_matches('\n')
            );
        }
    }
}

pub(super) fn web_search_bytes(references: &[pb::WebSearchReference]) -> usize {
    references
        .iter()
        .map(|reference| reference.title.len() + reference.url.len() + reference.chunk.len())
        .sum()
}
