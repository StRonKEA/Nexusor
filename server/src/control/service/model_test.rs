//! One-shot connectivity and throughput probes against a configured model.

use std::time::Instant;

use futures_util::StreamExt;
use tokio_util::sync::CancellationToken;

use crate::{
    model::{
        ContentPart, ModelInvocation, ModelRequest, ModelSpec, ProjectedContent, ProjectedMessage,
        PromptSpec, Role,
    },
    provider::{is_valid_response_event, ModelEvent},
    Error, Result,
};

use super::{discovery::estimate_output_tokens, types::*, ControlService};

impl ControlService {
    pub async fn test_model(
        &self,
        model_hash: &str,
        test_id: &str,
    ) -> Result<ModelConnectivityResult> {
        let cancellation = CancellationToken::new();
        let cancellation = {
            let mut tests = self
                .model_tests
                .lock()
                .expect("model test registry mutex poisoned");
            tests
                .entry(test_id.to_owned())
                .or_insert_with(|| cancellation.clone())
                .clone()
        };
        let mut result = self.run_model_test(model_hash, cancellation.clone()).await;

        // Auto-Probe & Self-Healing:
        // If test failed with 404, 405, or unsupported endpoint, probe the alternative OpenAI endpoint!
        if let Err(ref err) = result {
            let err_str = err.to_string();
            let is_endpoint_issue = err_str.contains("404")
                || err_str.contains("405")
                || err_str.contains("Unsupported parameter")
                || err_str.contains("not found");
            if is_endpoint_issue && !model_hash.starts_with(crate::plugin::ADAPTER_ID_PREFIX) {
                if let Ok(Some(model_cfg)) = self.store.model(model_hash).await {
                    if model_cfg.model_type == crate::model::ModelType::OpenAi {
                        let current_endpoint = model_cfg.openai_endpoint.trim();
                        let alt_endpoint = if current_endpoint.ends_with("responses") {
                            crate::model::OPENAI_CHAT_ENDPOINT
                        } else {
                            crate::model::OPENAI_RESPONSES_ENDPOINT
                        };
                        tracing::info!(
                            model_hash,
                            current_endpoint,
                            alt_endpoint,
                            "model test failed with endpoint mismatch, probing alternative endpoint..."
                        );
                        let alt_input = crate::model::ModelConfigInput {
                            sort_order: model_cfg.sort_order,
                            display_name: model_cfg.display_name.clone(),
                            group_name: model_cfg.group_name.clone(),
                            model_type: model_cfg.model_type,
                            base_url: model_cfg.base_url.clone(),
                            use_full_url: model_cfg.use_full_url,
                            api_key: model_cfg.api_key.clone(),
                            tooltip_data: model_cfg.tooltip_data.clone(),
                            model_id: model_cfg.model_id.clone(),
                            reasoning_effort: model_cfg.reasoning_effort.clone(),
                            openai_endpoint: alt_endpoint.to_string(),
                            openai_extra_params_enabled: model_cfg.openai_extra_params_enabled,
                            openai_extra_params: model_cfg.openai_extra_params.clone(),
                            custom_headers_enabled: model_cfg.custom_headers_enabled,
                            custom_headers: model_cfg.custom_headers.clone(),
                            anthropic_extra_params_enabled: model_cfg
                                .anthropic_extra_params_enabled,
                            anthropic_extra_params: model_cfg.anthropic_extra_params.clone(),
                            context_window_tokens: model_cfg.context_window_tokens,
                            max_completion_tokens: model_cfg.max_completion_tokens,
                            anthropic_max_tokens: model_cfg.anthropic_max_tokens,
                            anthropic_thinking_effort: model_cfg.anthropic_thinking_effort.clone(),
                            thinking_budget_tokens: model_cfg.thinking_budget_tokens,
                        };
                        if let Ok(saved) = self.store.update_model(model_hash, &alt_input).await {
                            let alt_result =
                                self.run_model_test(&saved.model_hash, cancellation).await;
                            if alt_result.is_ok() {
                                tracing::info!(
                                    old_hash = model_hash,
                                    new_hash = %saved.model_hash,
                                    endpoint = alt_endpoint,
                                    "auto-healed model endpoint successfully!"
                                );
                                result = alt_result;
                            } else {
                                // Revert back if alternative also failed
                                let _ = self
                                    .store
                                    .update_model(
                                        &saved.model_hash,
                                        &crate::model::ModelConfigInput {
                                            openai_endpoint: current_endpoint.to_string(),
                                            ..alt_input
                                        },
                                    )
                                    .await;
                            }
                        }
                    }
                }
            }
        }

        self.model_tests
            .lock()
            .expect("model test registry mutex poisoned")
            .remove(test_id);
        result
    }

    pub fn cancel_model_test(&self, test_id: &str) {
        let cancellation = {
            let mut tests = self
                .model_tests
                .lock()
                .expect("model test registry mutex poisoned");
            tests.entry(test_id.to_owned()).or_default().clone()
        };
        cancellation.cancel();
    }

    async fn run_model_test(
        &self,
        model_hash: &str,
        cancellation: CancellationToken,
    ) -> Result<ModelConnectivityResult> {
        const TEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(45);
        const TEST_PROMPT: &str = "Reply with ok";

        let mut model = ModelSpec::new(model_hash);
        if model_hash.starts_with(crate::plugin::ADAPTER_ID_PREFIX) {
            let descriptor = self.plugins.model_descriptor(model_hash).await?;
            model.display_name = Some(descriptor.display_name);
            // A connectivity check only needs a few tokens.
            model.max_output_tokens = Some(32);
            // Do NOT force thinking effort to "low" for Antigravity/Gemini models,
            // because models like gemini-2.5-flash/gemini-2.5-pro reject "LOW" with:
            // "Thinking level LOW is not supported for this model".
            model.reasoning.effort = None;
            model.reasoning.enabled = false;
        } else {
            let configured = self
                .store
                .model(model_hash)
                .await?
                .ok_or_else(|| Error::RunNotFound(format!("model {model_hash}")))?;
            configured.configure(&mut model);
            model.max_output_tokens = Some(32);
        }
        let call_id = format!("model-test-{}", uuid::Uuid::new_v4());
        let invocation = ModelInvocation {
            call_id: call_id.clone(),
            run_id: call_id.clone(),
            conversation_id: call_id.clone(),
            provider_call_index: 0,
            request: ModelRequest {
                prompt: PromptSpec {
                    instructions: String::new(),
                    tools: Vec::new(),
                },
                model,
                history: vec![ProjectedMessage {
                    message_id: "connectivity-test".into(),
                    role: Role::User,
                    content: ProjectedContent::Parts(vec![ContentPart::Text {
                        text: TEST_PROMPT.into(),
                    }]),
                }],
            },
            slot_account_ids: None,
            slot_strategy: None,
        };
        let started = Instant::now();
        let mut first_valid_response_at = None;
        let mut output_tokens = None;
        let mut output = String::new();
        let stream = self.provider.stream(invocation, cancellation.clone());
        let completed = tokio::time::timeout(TEST_TIMEOUT, async {
            futures_util::pin_mut!(stream);
            let mut finished = false;
            while let Some(event) = stream.next().await {
                let event = event?;
                if first_valid_response_at.is_none() && is_valid_response_event(&event) {
                    first_valid_response_at = Some(Instant::now());
                }
                match event {
                    ModelEvent::TextDelta(delta) => {
                        output.push_str(&delta);
                    }
                    ModelEvent::Usage(usage) => {
                        if let Some(tokens) = usage.output_tokens.filter(|tokens| *tokens > 0) {
                            output_tokens = Some(
                                output_tokens.map_or(tokens, |current: u64| current.max(tokens)),
                            );
                        }
                    }
                    ModelEvent::Done(_) => finished = true,
                    _ => {}
                }
            }
            if cancellation.is_cancelled() {
                return Err(Error::Cancelled);
            }
            if !finished {
                return Err(Error::Protocol(
                    "provider stream ended without Done during connectivity test".into(),
                ));
            }
            Ok(())
        })
        .await;
        match completed {
            Ok(result) => result?,
            Err(_) => {
                cancellation.cancel();
                self.store
                    .finish_llm_call(
                        &call_id,
                        "error",
                        None,
                        started.elapsed().as_millis().min(i64::MAX as u128) as i64,
                        Some("timeout"),
                        Some("model connectivity test timed out after 45 seconds"),
                    )
                    .await?;
                return Err(Error::Provider(
                    "model connectivity test timed out after 45 seconds".into(),
                ));
            }
        }
        let elapsed = started.elapsed();
        let output = output.trim().to_string();
        if first_valid_response_at.is_none() {
            return Err(Error::Provider(
                "model connectivity test received no valid response".into(),
            ));
        }
        let tokens_estimated = output_tokens.is_none();
        let output_tokens = output_tokens.unwrap_or_else(|| estimate_output_tokens(&output));
        Ok(ModelConnectivityResult {
            duration_ms: elapsed.as_millis().min(u128::from(u64::MAX)) as u64,
            first_valid_response_ms: first_valid_response_at.map(|first| {
                first
                    .duration_since(started)
                    .as_millis()
                    .min(u128::from(u64::MAX)) as u64
            }),
            output_tokens,
            tokens_per_second: if elapsed.is_zero() {
                0.0
            } else {
                output_tokens as f64 / elapsed.as_secs_f64()
            },
            tokens_estimated,
            output,
        })
    }
}
