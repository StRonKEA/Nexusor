//! Classifies Cursor requests into high-level tasks to guide smart routing.

use crate::model::{ContentPart, ModelInvocation, ProjectedContent, Role};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskIntent {
    /// Fast responses: explanations, small questions, terminal lookups.
    Fast,
    /// Coding: multi-file modifications, refactoring, agent tool executions.
    Coding,
    /// Creative / Complex: large architecture planning or visual analysis.
    Complex,
}

pub fn classify_intent(invocation: &ModelInvocation) -> TaskIntent {
    let req = &invocation.request;

    // 0. If reasoning/thinking is explicitly enabled on the model spec, it's Complex/Reasoning.
    if req.model.reasoning.enabled {
        return TaskIntent::Complex;
    }

    // 1. Inspect the latest user message content for intent keywords (preserving user goal across tool turns)
    let mut text = String::new();
    let user_msg = req.history.iter().rev().find(|m| m.role == Role::User);

    if let Some(msg) = user_msg.or_else(|| req.history.last()) {
        match &msg.content {
            ProjectedContent::Parts(parts) => {
                for part in parts {
                    if let ContentPart::Text { text: t } = part {
                        text.push_str(t);
                        text.push(' ');
                    }
                }
            }
            ProjectedContent::Assistant { text: t, .. } => {
                text.push_str(t);
            }
            ProjectedContent::ToolResult(r) => {
                text.push_str(&r.content);
            }
        }
    }

    // Cursor wraps the user's request with mode reminders and timestamps. Those
    // reminders mention architecture/planning even for a simple question.
    let text = text
        .rsplit_once("<user_query>")
        .and_then(|(_, query)| query.split_once("</user_query>"))
        .map(|(query, _)| query)
        .unwrap_or(&text)
        .trim();
    let lower = text.to_ascii_lowercase();

    // Complex / Reasoning keywords (English & Turkish)
    if lower.contains("architecture")
        || lower.contains("architect")
        || lower.contains("mimari")
        || lower.contains("redesign")
        || lower.contains("audit")
        || lower.contains("denetim")
        || lower.contains("security review")
        || lower.contains("güvenlik")
        || lower.contains("trade-off")
        || lower.contains("algorithm")
        || lower.contains("algoritma")
        || lower.contains("deep think")
        || lower.contains("derin düşün")
        || lower.contains("system design")
        || lower.contains("sistem tasarımı")
        || lower.contains("planlama")
        || lower.contains("roadmap")
    {
        return TaskIntent::Complex;
    }

    // Coding keywords (English & Turkish)
    if lower.contains("fix")
        || lower.contains("düzelt")
        || lower.contains("bug")
        || lower.contains("hata")
        || lower.contains("implement")
        || lower.contains("uygula")
        || lower.contains("refactor")
        || lower.contains("write a function")
        || lower.contains("fonksiyon")
        || lower.contains("class")
        || lower.contains("sınıf")
        || lower.contains("code")
        || lower.contains("kod")
        || lower.contains("exception")
        || lower.contains("compile")
        || lower.contains("derle")
        || lower.split_whitespace().any(|word| {
            matches!(
                word.trim_matches(|c: char| c.is_ascii_punctuation()),
                "edit"
                    | "write"
                    | "delete"
                    | "rename"
                    | "remove"
                    | "patch"
                    | "düzenle"
                    | "yaz"
                    | "sil"
                    | "değiştir"
                    | "continue"
                    | "devam"
            )
        })
        || text.contains("```")
        || text.contains("diff")
    {
        return TaskIntent::Coding;
    }

    // Cursor supplies editing tools even in Ask mode. Available tools alone do
    // not make a short question a coding task; explicit intent above takes priority.
    // If message is short (< 150 chars) or only exploration tools, fast intent
    let has_only_explore_tools = !req.prompt.tools.is_empty()
        && req.prompt.tools.iter().all(|t| {
            let name = t.name.to_ascii_lowercase();
            name.contains("read")
                || name.contains("grep")
                || name.contains("search")
                || name.contains("list")
                || name.contains("find")
        });

    if has_only_explore_tools || (text.len() < 150 && !text.is_empty()) {
        return TaskIntent::Fast;
    }

    TaskIntent::Coding
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        ContentPart, ModelInvocation, ModelRequest, ModelSpec, ProjectedContent, ProjectedMessage,
        PromptSpec, Role,
    };

    fn make_invocation(text: &str, tools: Vec<&str>) -> ModelInvocation {
        ModelInvocation {
            call_id: "call-1".into(),
            provider_call_index: 0,
            run_id: "test-run".into(),
            conversation_id: "conv-1".into(),
            request: ModelRequest {
                prompt: PromptSpec {
                    instructions: String::new(),
                    tools: tools
                        .into_iter()
                        .map(|n| crate::model::ToolDefinition {
                            name: n.into(),
                            description: String::new(),
                            parameters: serde_json::json!({}),
                        })
                        .collect(),
                },
                history: vec![ProjectedMessage {
                    message_id: "m1".into(),
                    role: Role::User,
                    content: ProjectedContent::Parts(vec![ContentPart::Text { text: text.into() }]),
                }],
                model: ModelSpec {
                    model_id: "auto-smart".into(),
                    display_name: None,
                    max_output_tokens: None,
                    context_window_tokens: None,
                    supports_image_generation: false,
                    reasoning: Default::default(),
                    latency: Default::default(),
                    extra_params: Default::default(),
                },
            },
            slot_account_ids: None,
            slot_strategy: None,
        }
    }

    #[test]
    fn cursor_editing_tools_do_not_override_short_user_requests() {
        let tools = vec![
            "AskQuestion",
            "Read",
            "Shell",
            "Delete",
            "StrReplace",
            "TodoWrite",
            "Write",
            "CallMcpTool",
        ];
        for query in [
            "Hi. Reply only with FAST_ASK_JADE_814. Do not use tools.",
            "Merhaba, nasılsın?",
            "2 + 2 kaç eder?",
        ] {
            let text = format!("<system_reminder>You are in Ask Mode. Do not write code.</system_reminder><user_query>\r\n{query}\r\n</user_query>");
            assert_eq!(
                classify_intent(&make_invocation(&text, tools.clone())),
                TaskIntent::Fast,
                "{query}"
            );
        }
        for query in [
            "Fix this bug.",
            "Edit README.md.",
            "Write a test.",
            "Delete old.txt.",
            "Dosyayı düzenle.",
            "Devam et.",
        ] {
            assert_eq!(
                classify_intent(&make_invocation(query, tools.clone())),
                TaskIntent::Coding,
                "{query}"
            );
        }
        assert_eq!(
            classify_intent(&make_invocation("Review the architecture.", tools)),
            TaskIntent::Complex
        );
    }

    #[test]
    fn classifies_short_questions_as_fast() {
        let inv = make_invocation("merhaba nasılsın?", vec![]);
        assert_eq!(classify_intent(&inv), TaskIntent::Fast);
    }

    #[test]
    fn classifies_bug_fix_as_coding() {
        let inv = make_invocation("bu fonksiyondaki bug nedir ve error neden çıkıyor?", vec![]);
        assert_eq!(classify_intent(&inv), TaskIntent::Coding);
    }

    #[test]
    fn cursor_mode_reminders_do_not_override_user_intent() {
        for (query, tools, expected) in [
            ("Reply exactly READY.", vec!["read_file"], TaskIntent::Fast),
            ("Fix this bug.", vec!["edit_file"], TaskIntent::Coding),
            (
                "Review the system architecture.",
                vec!["read_file"],
                TaskIntent::Complex,
            ),
        ] {
            let text = format!("<system_reminder>Proactive Planning Rule: complex system architecture</system_reminder><user_query>{query}</user_query>");
            assert_eq!(classify_intent(&make_invocation(&text, tools)), expected);
        }
        let mut thinking = make_invocation("<user_query>Hello</user_query>", vec![]);
        thinking.request.model.reasoning.enabled = true;
        assert_eq!(classify_intent(&thinking), TaskIntent::Complex);
    }

    #[test]
    fn preserves_intent_in_multiturn_tool_results() {
        let mut inv = make_invocation(
            "sistem mimarisi ve architecture planlama",
            vec!["read_file"],
        );
        // Append a tool result as the last message in history
        inv.request.history.push(ProjectedMessage {
            message_id: "m2".into(),
            role: Role::Tool,
            content: ProjectedContent::ToolResult(crate::model::ToolResultContent {
                call_id: "c1".into(),
                name: "read_file".into(),
                content: "plain text with no keywords".into(),
                is_error: false,
                image: None,
                images: Vec::new(),
                provider_parts: vec![],
            }),
        });
        assert_eq!(classify_intent(&inv), TaskIntent::Complex);
    }
}
