//! Text helpers shared by anything that has to read a stored message.
//!
//! A stored user turn is mostly scaffolding: environment, rules, skills, a tool
//! manifest, and then the request inside `<user_query>`. Two callers need to
//! look past it — conversation search, which must not match or quote the
//! scaffolding, and the transcript writer, which must not bury a conversation
//! under it. Keeping the stripping here stops the two from drifting apart.

/// Removes every `<name>…</name>` block, whatever the name.
///
/// A list of known wrapper names kept going stale: `<git_status>` and
/// `<branch_status>` were absent from it, so those tags surfaced as titles. Any
/// tag-shaped block is scaffolding, so all of them go.
pub(crate) fn strip_all_tags(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    loop {
        let Some(open) = rest.find('<') else {
            out.push_str(rest);
            return out;
        };
        let Some(relative) = rest[open..].find('>') else {
            // No closing angle bracket: the rest is prose, not markup.
            out.push_str(rest);
            return out;
        };
        let gt = open + relative;
        let name = &rest[open + 1..gt];
        if !is_tag_name(name) {
            out.push_str(&rest[..=gt]);
            rest = &rest[gt + 1..];
            continue;
        }
        if name.starts_with('/') {
            // A closing tag with no opener. Keeping the prose after it would
            // leave a visible seam where the tag used to be, so the tag is
            // dropped together with the whitespace that followed it.
            out.push_str(&rest[..open]);
            rest = rest[gt + 1..].trim_start();
            continue;
        }
        let close = format!("</{name}>");
        match rest[open..].find(&close) {
            Some(end) => {
                out.push_str(&rest[..open]);
                rest = &rest[open + end + close.len()..];
            }
            // An unterminated block would otherwise leak its opening tag into
            // every title and snippet drawn from this turn.
            None => {
                out.push_str(&rest[..open]);
                return out;
            }
        }
    }
}

/// True for something that looks like a tag name rather than prose.
///
/// A closing tag counts too: `</timestamp>` left behind by an unbalanced block
/// would otherwise reach the snippet as prose. `< 3 and 5 >` is not a tag, and
/// treating it as one would delete the rest of a sentence.
fn is_tag_name(name: &str) -> bool {
    let name = name.strip_prefix('/').unwrap_or(name);
    !name.is_empty()
        && name
            .chars()
            .next()
            .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Returns the text of every `<name>…</name>` block, in order.
pub(crate) fn blocks_content(text: &str, name: &str) -> Vec<String> {
    let open = format!("<{name}>");
    let close = format!("</{name}>");
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find(&open) {
        let body_start = start + open.len();
        match rest[body_start..].find(&close) {
            Some(end) => {
                found.push(rest[body_start..body_start + end].to_string());
                rest = &rest[body_start + end + close.len()..];
            }
            None => break,
        }
    }
    found
}

/// Squeezes runs of blank lines so a snippet does not open with whitespace.
pub(crate) fn collapse_blank_lines(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut blanks = 0;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            blanks += 1;
            if blanks > 1 {
                continue;
            }
            out.push('\n');
        } else {
            blanks = 0;
            out.push_str(trimmed);
            out.push('\n');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prose_with_angle_brackets_survives() {
        let text = "if a < b and c > d then compare";
        assert_eq!(strip_all_tags(text), text);
    }

    #[test]
    fn a_stray_closing_tag_is_dropped_without_truncating() {
        assert_eq!(strip_all_tags("abc</timestamp> sonra"), "abcsonra");
    }

    #[test]
    fn an_unterminated_block_is_dropped_not_shown() {
        assert_eq!(strip_all_tags("<user_info>\nOS Version: win32\n"), "");
    }

    #[test]
    fn every_block_of_a_name_is_returned_in_order() {
        let text = "<q>bir</q> arada <q>iki</q>";
        assert_eq!(blocks_content(text, "q"), vec!["bir", "iki"]);
    }

    #[test]
    fn runs_of_blank_lines_collapse_to_one() {
        assert_eq!(collapse_blank_lines("a\n\n\n\nb\n"), "a\n\nb\n");
    }
}
