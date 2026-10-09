//! Decides when to compact provider-visible context and builds a stable fallback summary.

use std::collections::HashSet;

use crate::{
    model::{
        estimate_context_tokens, estimate_projected_messages_tokens, CanonicalMessage, ContentPart,
        PreparedRun, ProjectedContent, ProjectedMessage, Role,
    },
    store::ContextUsageAnchor,
};

const FALLBACK_CHARS: usize = 12_000;

/// Fraction of the context window kept free, as a divisor: 10 = 10%.
///
/// The estimate runs behind the provider: the anchor is what the provider
/// charged for the *previous* call, and the next request re-sends request
/// context and carries provider-side overhead the message-tail estimate does
/// not model. A real conversation measured 948K estimated against 1,017,628
/// actual, a 7% shortfall that landed it over a 1M window while the check said
/// there was room. A proportional reserve absorbs that drift and scales with
/// the model: a 200K window keeps 20K free and a 1M window keeps 100K.
const CONTEXT_RESERVE_DIVISOR: u64 = 10;
pub(super) const OUTPUT_TOKENS: u64 = 4_096;
pub(super) const INSTRUCTIONS: &str = "Summarize the conversation for the next model turn. Preserve goals, constraints, decisions, files, commands, errors, results, and unfinished work. Distinguish earlier tasks from the current request. Record which steps of the current request have already completed, their actual results, and the exact remaining step (or final reply). A requested or failed tool call is not proof of completion. Do not call tools. Return only the concise durable summary.";

/// Usable prompt budget: the window minus the proportional reserve.
pub(super) fn context_budget(context_window: u64) -> u64 {
    context_window.saturating_sub(context_window / CONTEXT_RESERVE_DIVISOR)
}

pub(super) fn input_budget(prepared: &PreparedRun) -> Option<u64> {
    prepared.model.context_window_tokens.map(context_budget)
}

/// Whether a provider failure means the prompt did not fit.
///
/// Providers report this as a plain 400 with prose, so there is nothing
/// structured to match on. Anthropic says "prompt is too long"; OpenAI-style
/// gateways use `context_length_exceeded` or "maximum context length".
pub(super) fn is_context_overflow(message: &str) -> bool {
    let lowered = message.to_ascii_lowercase();
    lowered.contains("prompt is too long")
        || lowered.contains("context window exceeded")
        || lowered.contains("model_context_window_exceeded")
        || lowered.contains("model_max_prompt_tokens_exceeded")
        || (lowered.contains("prompt token count") && lowered.contains("exceeds the limit"))
        || lowered.contains("context_length_exceeded")
        || (lowered.contains("input token count") && lowered.contains("maximum"))
        || (lowered.contains("maximum context length") && lowered.contains("token"))
        || (lowered.contains("context")
            && (lowered.contains("exceeded")
                || lowered.contains("overflow")
                || lowered.contains("too large")))
        || (lowered.contains("token")
            && (lowered.contains("exceeded the maximum")
                || lowered.contains("maximum context token limit")))
}

/// Builds the history for a compaction call.
///
/// Two properties matter, and replaying the raw history guarantees neither:
///
/// 1. The history must end with a user message. Otherwise providers read the
///    request as an assistant prefill and refuse it outright: Anthropic answers
///    "This model does not support assistant message prefill. The conversation
///    must end with a user message."
/// 2. The history must fit the context window. Compaction runs precisely
///    because the conversation is too large, so replaying all of it asks the
///    summarizer to accept a prompt that is already over the limit and the
///    call fails with "prompt is too long".
///
/// Either failure falls back to the truncated summary, which is usually still
/// too large, so the conversation stays over its window and cannot recover.
///
/// Trimming keeps whole recent turns. If one turn exceeds the window, send a
/// bounded textual excerpt instead of malformed partial tool-call history.
pub(super) fn compaction_history(
    history: Vec<ProjectedMessage>,
    context_window: Option<u64>,
) -> Vec<ProjectedMessage> {
    let mut history = trim_to_context(history, context_window);
    // A trailing tool result otherwise invites continuation of the old task,
    // even with no tools advertised. Always end with the summarization request.
    history.push(ProjectedMessage {
        message_id: "compaction:instruction".into(),
        role: Role::User,
        content: ProjectedContent::Parts(vec![ContentPart::Text {
            text: INSTRUCTIONS.into(),
        }]),
    });
    history
}

fn trim_to_context(
    mut history: Vec<ProjectedMessage>,
    context_window: Option<u64>,
) -> Vec<ProjectedMessage> {
    let Some(budget) = context_window
        .filter(|window| *window > 0)
        .map(context_budget)
        // Reserve both the system instruction and possible user termination.
        .map(|budget| budget.saturating_sub(OUTPUT_TOKENS + INSTRUCTIONS.len() as u64 * 2 + 32))
    else {
        return history;
    };
    if estimate_projected_messages_tokens(&history) <= budget {
        return history;
    }
    // Walk back from the newest turn, keeping whole user-delimited turns.
    let mut kept = 0;
    let mut newest_turn = None;
    for (index, message) in history.iter().enumerate().rev() {
        if !is_turn_boundary(message) {
            continue;
        }
        newest_turn.get_or_insert(index);
        if estimate_projected_messages_tokens(&history[index..]) > budget {
            break;
        }
        kept = history.len() - index;
    }
    if kept == 0 {
        let start = newest_turn.unwrap_or(0);
        return oversized_turn_excerpt(&history[start..], budget);
    }
    history.split_off(history.len() - kept)
}

/// Provider limits are authoritative for this run, not persisted model settings.
/// Unknown formats use a bounded conservative recovery window, never a guessed
/// larger one. Repeated overflow is handled by the engine's one-recovery guard.
pub(super) fn recovery_window(message: &str, configured: Option<u64>) -> u64 {
    let patterns = [
        r"(?i)prompt token count of\s*[\d,]+\s+exceeds the limit of\s*([\d,]+)",
        r"(?i)tokens\s*>\s*([\d,]+)\s+maximum",
        r"(?i)maximum context length is\s*([\d,]+)",
        r"(?i)maximum (?:number of tokens allowed|input tokens?|context token limit)\s*[:=]?\s*\(?([\d,]+)",
    ];
    let reported = patterns.iter().find_map(|pattern| {
        regex::Regex::new(pattern)
            .ok()?
            .captures(message)?
            .get(1)?
            .as_str()
            .replace(',', "")
            .parse::<u64>()
            .ok()
            .filter(|n| *n > 0)
    });
    match (reported, configured.filter(|n| *n > 0)) {
        (Some(limit), Some(window)) => limit.min(window),
        (Some(limit), None) => limit,
        (None, Some(window)) => (window / 2).min(32_000),
        (None, None) => 32_000,
    }
}

fn oversized_turn_excerpt(history: &[ProjectedMessage], budget: u64) -> Vec<ProjectedMessage> {
    use crate::model::ContentPart;

    let mut transcript = String::new();
    for message in history {
        transcript.push_str(&format!("\n{:?}:\n", message.role));
        match &message.content {
            ProjectedContent::Parts(parts) => {
                for part in parts {
                    match part {
                        ContentPart::Text { text } => transcript.push_str(text),
                        ContentPart::Image { .. } => {
                            transcript.push_str("[image omitted from oversized summary input]")
                        }
                    }
                }
            }
            ProjectedContent::Assistant { text, calls, .. } => {
                transcript.push_str(text);
                for call in calls {
                    transcript.push_str(&format!(
                        "\nTool call {} {}: {}",
                        call.call_id, call.name, call.arguments
                    ));
                }
            }
            ProjectedContent::ToolResult(result) => {
                transcript.push_str(&format!(
                    "Tool result {} {} (error={}):\n{}",
                    result.call_id, result.name, result.is_error, result.content
                ));
            }
        }
    }
    const NOTICE: &str = "Conversation excerpt for summarization; the oversized turn was shortened. Omitted content is unknown; do not infer its results. Full history remains archived.\n";
    const GAP: &str = "\n[... middle of oversized turn omitted ...]\n";
    // A byte cap is conservative even for Unicode, punctuation and code, where
    // the usual four-characters/token heuristic can underestimate badly.
    let bytes = usize::try_from(budget.saturating_sub(32)).unwrap_or(usize::MAX);
    let available = bytes.saturating_sub(NOTICE.len() + GAP.len());
    let mut head = available / 2;
    head = head.min(transcript.len());
    while !transcript.is_char_boundary(head) {
        head -= 1;
    }
    let mut tail = transcript.len().saturating_sub(available - head).max(head);
    while !transcript.is_char_boundary(tail) {
        tail += 1;
    }
    let text = format!(
        "{NOTICE}{}{GAP}{}",
        &transcript[..head],
        &transcript[tail..]
    );
    vec![ProjectedMessage {
        message_id: "compaction:oversized-turn".into(),
        role: Role::User,
        content: ProjectedContent::Parts(vec![ContentPart::Text { text }]),
    }]
}

fn is_turn_boundary(message: &ProjectedMessage) -> bool {
    message.role == Role::User && matches!(message.content, ProjectedContent::Parts(_))
}

pub(super) fn estimated_tokens(
    prepared: &PreparedRun,
    projected_messages: &[ProjectedMessage],
    anchor: Option<ContextUsageAnchor>,
) -> u64 {
    anchor
        .filter(|anchor| anchor.message_count <= projected_messages.len())
        .map(|anchor| {
            anchor
                .context_input_tokens
                .saturating_add(estimate_projected_messages_tokens(
                    &projected_messages[anchor.message_count..],
                ))
        })
        .unwrap_or_else(|| estimate_context_tokens(&prepared.prompt, projected_messages))
}

pub(super) fn compaction_estimate(
    prepared: &PreparedRun,
    projected_messages: &[ProjectedMessage],
    anchor: Option<ContextUsageAnchor>,
) -> Option<u64> {
    let budget = input_budget(prepared)?;
    let estimated = estimated_tokens(prepared, projected_messages, anchor);
    (estimated > budget).then_some(estimated)
}

#[cfg(test)]
pub(super) fn should_compact(
    prepared: &PreparedRun,
    projected_messages: &[ProjectedMessage],
    anchor: Option<ContextUsageAnchor>,
) -> bool {
    compaction_estimate(prepared, projected_messages, anchor).is_some()
}

pub(super) fn validate_compacted(
    prepared: &PreparedRun,
    projected_messages: &[ProjectedMessage],
) -> std::result::Result<u64, String> {
    let estimated = estimate_context_tokens(&prepared.prompt, projected_messages);
    let Some(budget) = input_budget(prepared) else {
        return Ok(estimated);
    };
    if estimated <= budget {
        return Ok(estimated);
    }
    Err(format!(
        "context overflow after compaction: estimated input {estimated} tokens exceeds budget {budget} tokens"
    ))
}

pub(super) fn partition(
    messages: &[CanonicalMessage],
    current_ids: &HashSet<&str>,
) -> (Vec<CanonicalMessage>, Option<CanonicalMessage>) {
    let latest_request_context = messages
        .iter()
        .rposition(|message| message.message_id.starts_with("request-context:"));
    let compactable = messages
        .iter()
        .enumerate()
        .filter(|(index, message)| {
            Some(*index) != latest_request_context
                && !current_ids.contains(message.message_id.as_str())
        })
        .map(|(_, message)| message.clone())
        .collect();
    let retained = latest_request_context
        .and_then(|index| messages.get(index))
        .filter(|message| !current_ids.contains(message.message_id.as_str()))
        .cloned();
    (compactable, retained)
}

pub(super) fn fallback_summary(messages: &[CanonicalMessage]) -> String {
    let serialized = serde_json::to_string(messages).unwrap_or_default();
    let start = serialized
        .char_indices()
        .rev()
        .nth(FALLBACK_CHARS.saturating_sub(1))
        .map_or(0, |(index, _)| index);
    format!(
        "Automatic summarization failed. This is only a raw tail excerpt, not a complete summary. Earlier goals, results and decisions may be missing; do not infer them or claim full coverage. Original history remains archived.\n{}",
        &serialized[start..]
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        project_messages, CheckpointId, ContentPart, ConversationId, ModelSpec, Origin, PromptSpec,
        Role, RunAction, RunId, RunKind,
    };

    fn prepared(context_window_tokens: u64) -> PreparedRun {
        let mut model = ModelSpec::new("model");
        model.context_window_tokens = Some(context_window_tokens);
        PreparedRun {
            run_id: RunId::new("run"),
            cursor_request_id: None,
            conversation_id: ConversationId::new("conversation"),
            kind: RunKind::Root,
            model,
            prompt: PromptSpec {
                instructions: String::new(),
                tools: Vec::new(),
            },
            initial_messages: Vec::new(),
            action: RunAction::Start,
            base_checkpoint_id: CheckpointId(1),
        }
    }

    #[test]
    fn automatic_compaction_uses_proportional_reserve_for_every_action() {
        let messages = vec![CanonicalMessage::text(
            "user",
            Role::User,
            Origin::Runtime,
            "x".repeat(40_000),
        )];
        let projected = project_messages(&messages).unwrap();
        let estimated = estimate_context_tokens(&prepared(1).prompt, &projected);
        // Smallest multiple-of-ten window whose 90% budget covers the estimate.
        let window = estimated.div_ceil(9) * 10;
        let mut prepared = prepared(window);
        assert!(context_budget(window) >= estimated);
        assert!(context_budget(window - 10) < estimated);

        assert!(!should_compact(&prepared, &projected, None));
        prepared.model.context_window_tokens = Some(window - 10);
        assert!(should_compact(&prepared, &projected, None));

        prepared.action = RunAction::Resume {
            pending_tool_round: None,
        };
        assert!(should_compact(&prepared, &projected, None));
    }

    #[test]
    fn the_reserve_leaves_room_for_the_estimate_to_run_behind() {
        // Reproduces a conversation that wedged itself against a 1M window.
        // The anchor said 947,797 tokens, so a fixed 10K reserve found room
        // and let the request through. Anthropic counted 1,017,628 and
        // refused it, and every retry repeated the same arithmetic.
        let messages = vec![CanonicalMessage::text(
            "user",
            Role::User,
            Origin::Runtime,
            "hello",
        )];
        let projected = project_messages(&messages).unwrap();
        let anchor = ContextUsageAnchor {
            context_input_tokens: 947_797,
            message_count: 1,
        };
        assert!(should_compact(
            &prepared(1_000_000),
            &projected,
            Some(anchor)
        ));
    }

    #[test]
    fn the_reserve_scales_with_the_window() {
        assert_eq!(context_budget(200_000), 180_000);
        assert_eq!(context_budget(1_000_000), 900_000);
        assert_eq!(context_budget(0), 0);
        assert_eq!(context_budget(1), 1);
    }

    #[test]
    fn provider_refusals_that_mean_the_prompt_did_not_fit_are_recognized() {
        assert!(is_context_overflow(
            "provider error: Anthropic 400 Bad Request: {\"type\":\"error\",\"error\":\
             {\"type\":\"invalid_request_error\",\"message\":\"prompt is too long: \
             1017628 tokens > 1000000 maximum\"}}"
        ));
        assert!(is_context_overflow("model_context_window_exceeded"));
        assert!(is_context_overflow("context_length_exceeded"));
        assert!(is_context_overflow(
            "The input token count (140000) exceeds the maximum number of tokens allowed (128000)."
        ));
        assert!(is_context_overflow(
            "This model's maximum context length is 128000 tokens"
        ));

        // Unrelated failures must not trigger a compaction, which would
        // destroy history to fix something compaction cannot fix.
        assert!(!is_context_overflow("401 Unauthorized: invalid api key"));
        assert!(!is_context_overflow("429 Too Many Requests"));
        assert!(!is_context_overflow(
            "This model does not support assistant message prefill"
        ));
    }

    #[test]
    fn recovery_uses_reported_limit_without_expanding_configured_window() {
        assert!(is_context_overflow(
            "prompt token count of 365785 exceeds the limit of 64000"
        ));
        assert!(is_context_overflow("model_max_prompt_tokens_exceeded"));
        assert_eq!(
            recovery_window(
                "prompt token count of 365785 exceeds the limit of 64000",
                Some(2_000_000)
            ),
            64_000
        );
        assert_eq!(
            recovery_window(
                "prompt is too long: 1002148 tokens > 1000000 maximum",
                Some(2_000_000)
            ),
            1_000_000
        );
        assert_eq!(
            recovery_window("maximum context length is 128,000 tokens", Some(1_000_000)),
            128_000
        );
        assert_eq!(recovery_window("The input token count (140000) exceeds the maximum number of tokens allowed (128000).", None), 128_000);
        assert_eq!(
            recovery_window("maximum context length is 128000 tokens", Some(32_000)),
            32_000
        );
        assert_eq!(
            recovery_window("context_length_exceeded", Some(1_000_000)),
            32_000
        );
        assert_eq!(recovery_window("context_length_exceeded", None), 32_000);
    }

    fn user(id: &str, text: &str) -> ProjectedMessage {
        ProjectedMessage {
            message_id: id.into(),
            role: Role::User,
            content: ProjectedContent::Parts(vec![ContentPart::Text { text: text.into() }]),
        }
    }

    fn assistant(id: &str, text: &str) -> ProjectedMessage {
        ProjectedMessage {
            message_id: id.into(),
            role: Role::Assistant,
            content: ProjectedContent::Assistant {
                text: text.into(),
                thinking: String::new(),
                replay_state: None,
                calls: Vec::new(),
            },
        }
    }

    #[test]
    fn compaction_history_always_ends_with_a_user_message() {
        // Providers reject an assistant-terminated history as a prefill, which
        // made every automatic compaction fall back to the truncated summary.
        let history = vec![user("u1", "question"), assistant("a1", "answer")];
        let prepared = compaction_history(history, Some(200_000));
        assert_eq!(prepared.last().unwrap().role, Role::User);
        assert_eq!(
            prepared.last().unwrap().message_id,
            "compaction:instruction"
        );

        // Even an existing user request must be superseded by the summary task.
        let history = vec![assistant("a1", "answer"), user("u2", "next")];
        let prepared = compaction_history(history.clone(), Some(200_000));
        assert_eq!(prepared[..history.len()], history);
        assert_eq!(
            prepared.last().unwrap().message_id,
            "compaction:instruction"
        );

        let history = vec![ProjectedMessage {
            message_id: "read-result".into(),
            role: Role::Tool,
            content: ProjectedContent::ToolResult(crate::model::ToolResultContent {
                call_id: "read-1".into(),
                name: "Read".into(),
                content: "archive data".into(),
                is_error: false,
                image: None,
                images: vec![],
                provider_parts: vec![],
            }),
        }];
        let prepared = compaction_history(history.clone(), Some(200_000));
        assert_eq!(prepared[..history.len()], history);
        assert_eq!(
            prepared.last().unwrap().message_id,
            "compaction:instruction"
        );
    }

    #[test]
    fn compaction_history_is_trimmed_to_fit_the_context_window() {
        // Compaction runs because the conversation is too large, so the
        // summarize call must not replay a prompt that is over the window.
        let big = "x".repeat(400_000);
        let history = vec![
            user("u1", &big),
            assistant("a1", &big),
            user("u2", &big),
            assistant("a2", "recent answer"),
        ];
        let window = 200_000;
        let prepared = compaction_history(history, Some(window));

        let budget = context_budget(window) - OUTPUT_TOKENS;
        assert!(estimate_projected_messages_tokens(&prepared) <= budget);
        assert_eq!(prepared.last().unwrap().role, Role::User);
        // The newest turn survives the trim and is never split.
        assert!(prepared.iter().any(|message| message.message_id == "u2"));
        assert!(prepared.iter().any(|message| message.message_id == "a2"));
        assert!(!prepared.iter().any(|message| message.message_id == "u1"));
    }

    #[test]
    fn compaction_history_bounds_an_oversized_turn_with_explicit_omissions() {
        let history = vec![
            user("u1", "old"),
            assistant("a1", "old answer"),
            user("u2", &format!("GOAL_OPAL {}", "ğ🦀\n".repeat(100_000))),
            assistant("a2", "RESULT_OPAL"),
        ];
        let prepared = compaction_history(history, Some(50_000));
        assert!(
            estimate_projected_messages_tokens(&prepared)
                + OUTPUT_TOKENS
                + (INSTRUCTIONS.len() as u64)
                < context_budget(50_000)
        );
        let ProjectedContent::Parts(parts) = &prepared[0].content else {
            panic!("expected excerpt")
        };
        let ContentPart::Text { text } = &parts[0] else {
            panic!("expected text")
        };
        assert!(text.contains("GOAL_OPAL"));
        assert!(text.contains("RESULT_OPAL"));
        assert!(text.contains("middle of oversized turn omitted"));
        assert!(!text.contains("old answer"));
        assert_eq!(prepared.last().unwrap().role, Role::User);
    }

    #[test]
    fn compaction_history_without_a_context_window_is_untouched_apart_from_termination() {
        let history = vec![user("u1", "question"), assistant("a1", "answer")];
        let prepared = compaction_history(history.clone(), None);
        assert_eq!(prepared[..2], history[..]);
        assert_eq!(prepared.last().unwrap().role, Role::User);
    }

    #[test]
    fn provider_usage_anchor_only_estimates_messages_added_after_last_request() {
        let messages = vec![
            CanonicalMessage::text("old", Role::User, Origin::Runtime, "x".repeat(400_000)),
            CanonicalMessage::text("new", Role::User, Origin::Runtime, "short follow-up"),
        ];
        let projected = project_messages(&messages).unwrap();
        let anchor = ContextUsageAnchor {
            context_input_tokens: 103_904,
            message_count: 1,
        };
        let expected = 103_904 + estimate_projected_messages_tokens(&projected[1..]);

        assert_eq!(
            estimated_tokens(&prepared(200_000), &projected, Some(anchor)),
            expected
        );
        assert!(!should_compact(
            &prepared(200_000),
            &projected,
            Some(anchor)
        ));
    }

    #[test]
    fn provider_usage_anchor_triggers_after_new_messages_cross_budget() {
        let messages = vec![
            CanonicalMessage::text("old", Role::User, Origin::Runtime, "old"),
            CanonicalMessage::text("new", Role::User, Origin::Runtime, "x".repeat(80_000)),
        ];
        let projected = project_messages(&messages).unwrap();

        assert!(should_compact(
            &prepared(200_000),
            &projected,
            Some(ContextUsageAnchor {
                context_input_tokens: 180_000,
                message_count: 1,
            })
        ));
    }

    #[test]
    fn missing_anchor_uses_full_fallback() {
        let messages = vec![CanonicalMessage::text(
            "user",
            Role::User,
            Origin::Runtime,
            "x".repeat(40_000),
        )];
        let projected = project_messages(&messages).unwrap();
        let prepared = prepared(200_000);

        assert_eq!(
            estimated_tokens(&prepared, &projected, None),
            estimate_context_tokens(&prepared.prompt, &projected)
        );
    }

    #[test]
    fn invalid_anchor_message_count_uses_full_fallback() {
        let messages = vec![CanonicalMessage::text(
            "user",
            Role::User,
            Origin::Runtime,
            "x".repeat(40_000),
        )];
        let projected = project_messages(&messages).unwrap();
        let expected = estimate_context_tokens(&prepared(200_000).prompt, &projected);

        assert_eq!(
            estimated_tokens(
                &prepared(200_000),
                &projected,
                Some(ContextUsageAnchor {
                    context_input_tokens: 1,
                    message_count: 2,
                })
            ),
            expected
        );
    }

    #[test]
    fn compacted_history_is_validated_against_the_same_budget() {
        let messages = vec![CanonicalMessage::text(
            "user",
            Role::User,
            Origin::Runtime,
            "x".repeat(40_000),
        )];
        let projected = project_messages(&messages).unwrap();
        let estimated = estimate_context_tokens(&prepared(1).prompt, &projected);
        let window = estimated.div_ceil(9) * 10;
        assert!(context_budget(window) >= estimated);
        assert!(context_budget(window - 10) < estimated);

        assert_eq!(
            validate_compacted(&prepared(window), &projected),
            Ok(estimated)
        );
        assert!(validate_compacted(&prepared(window - 10), &projected)
            .unwrap_err()
            .contains("context overflow after compaction"));
    }
}
