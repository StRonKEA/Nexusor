//! Local lexical relevance; scores are independent of candidate ordering.
use std::collections::{BTreeSet, HashMap};

fn terms(text: &str) -> BTreeSet<String> {
    let mut expanded = String::with_capacity(text.len());
    let mut lower = false;
    for ch in text.chars() {
        if lower && ch.is_uppercase() {
            expanded.push(' ');
        }
        expanded.extend(ch.to_lowercase());
        lower = ch.is_lowercase() || ch.is_numeric();
    }
    expanded
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|word| {
            word.len() > 1
                && !matches!(
                    *word,
                    "the"
                        | "and"
                        | "for"
                        | "with"
                        | "this"
                        | "that"
                        | "return"
                        | "const"
                        | "let"
                        | "function"
                        | "pub"
                        | "fn"
                        | "use"
                )
        })
        .map(str::to_owned)
        .collect()
}

pub(super) fn scores<'a>(
    query: &str,
    candidates: impl Iterator<Item = (&'a str, &'a str)>,
) -> Vec<f32> {
    let query = terms(query);
    let docs: Vec<_> = candidates
        .map(|(path, content)| (terms(path), terms(content)))
        .collect();
    let mut frequency: HashMap<&str, usize> = HashMap::new();
    for (path, content) in &docs {
        for term in path.union(content).filter(|term| query.contains(*term)) {
            *frequency.entry(term.as_str()).or_default() += 1;
        }
    }
    let weight = |term: &str| {
        ((docs.len() as f32 + 1.0) / (frequency.get(term).copied().unwrap_or(0) as f32 + 1.0)).ln()
            + 1.0
    };
    let total: f32 = query.iter().map(|term| weight(term)).sum();
    docs.iter()
        .map(|(path, content)| {
            if total == 0.0 {
                return 0.0;
            }
            query
                .iter()
                .map(|term| {
                    weight(term)
                        * if path.contains(term) {
                            1.0
                        } else if content.contains(term) {
                            0.7
                        } else {
                            0.0
                        }
                })
                .sum::<f32>()
                / total
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::scores;
    #[test]
    fn ranks_content_and_identifiers_independently_of_input_order() {
        let docs = [
            ("style.css", "body color red"),
            ("auth/token.rs", "refreshToken expired OAuth"),
            ("notes.md", "OAuth examples"),
        ];
        let ranked = scores("fix expired refreshToken", docs.into_iter());
        assert!(ranked[1] > ranked[2] && ranked[1] > ranked[0]);
        let reversed = scores("fix expired refreshToken", docs.into_iter().rev());
        assert_eq!(ranked, reversed.into_iter().rev().collect::<Vec<_>>());
        assert_eq!(scores("", docs.into_iter()), vec![0.0; 3]);
        assert!(scores("absent", docs.into_iter())
            .iter()
            .all(|value| *value == 0.0));
    }
}
