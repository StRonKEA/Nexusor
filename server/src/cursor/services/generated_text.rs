//! Removes a complete Markdown wrapper without trimming actual generated code.
pub(super) fn without_outer_fence(text: &str) -> &str {
    let outer = text.trim();
    let Some((opening, rest)) = outer.split_once('\n') else {
        return text;
    };
    let Some(language) = opening.strip_prefix("```") else {
        return text;
    };
    if language.contains('`') || language.split_whitespace().count() > 1 {
        return text;
    }
    let Some(body) = rest.strip_suffix("```") else {
        return text;
    };
    body.strip_suffix("\r\n")
        .or_else(|| body.strip_suffix('\n'))
        .unwrap_or(body)
}

#[cfg(test)]
mod tests {
    use super::without_outer_fence;

    #[test]
    fn removes_only_complete_outer_wrappers_preserving_code_whitespace() {
        assert_eq!(
            without_outer_fence("```js\r\n  return 1;\r\n```\n"),
            "  return 1;"
        );
        assert_eq!(without_outer_fence("  return 1;\n"), "  return 1;\n");
        assert_eq!(without_outer_fence("```\na\n\n```"), "a\n");
        assert_eq!(without_outer_fence("```js\na"), "```js\na");
        assert_eq!(
            without_outer_fence("```js\na\n```\nexplanation"),
            "```js\na\n```\nexplanation"
        );
    }
}
