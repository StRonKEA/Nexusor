//! Routes model requests to built-in configurations or stable plugin model IDs.
use std::{sync::Arc, time::Duration};

use async_stream::try_stream;
use futures_util::StreamExt;
use tokio_util::sync::CancellationToken;

use crate::{
    config::{ProviderConfig, ProviderKind},
    model::{ModelInvocation, ModelLatency, NewLlmCall, ProviderType},
    plugin::{parse_model_id, PluginRegistry, ADAPTER_ID_PREFIX},
    store::Store,
    Error, Result,
};

use super::{
    normalize::NormalizedProvider, recorder::CancelOnDrop, AnthropicProvider, CallRecorder,
    OpenAiChatProvider, OpenAiResponsesProvider, Provider, ProviderStream,
};

type RoutingCandidate = (
    String,
    Option<Vec<String>>,
    Option<crate::plugin::PoolStrategy>,
);

static KEY_ROTATION_COUNTER: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

fn select_api_key(api_key: &str) -> String {
    let keys: Vec<&str> = api_key
        .split([',', '\n'])
        .map(str::trim)
        .filter(|k| !k.is_empty())
        .collect();
    if keys.len() <= 1 {
        return api_key.trim().to_string();
    }
    let index = KEY_ROTATION_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    keys[index % keys.len()].to_string()
}

static COMBO_ROUND_ROBIN_COUNTERS: std::sync::OnceLock<
    tokio::sync::Mutex<std::collections::HashMap<String, usize>>,
> = std::sync::OnceLock::new();

fn get_combo_counters() -> &'static tokio::sync::Mutex<std::collections::HashMap<String, usize>> {
    COMBO_ROUND_ROBIN_COUNTERS
        .get_or_init(|| tokio::sync::Mutex::new(std::collections::HashMap::new()))
}

async fn is_slot_available(plugins: &PluginRegistry, slot: &crate::store::ComboSlot) -> bool {
    let model_id = &slot.model_id;
    if !model_id.starts_with(ADAPTER_ID_PREFIX) {
        return true;
    }
    let Some((plugin_id, provider_id, upstream_id)) = parse_model_id(model_id) else {
        return true;
    };
    let resource_type = match (plugin_id, provider_id) {
        ("dev.nexusor.examples.codex-auth" | "dev.cursorbyok.examples.codex-auth", "codex") => {
            crate::provider::providers::codex::RESOURCE_TYPE
        }
        ("dev.nexusor.examples.grok-auth" | "dev.cursorbyok.examples.grok-auth", "grok") => {
            crate::provider::providers::grok::RESOURCE_TYPE
        }
        (
            "dev.nexusor.plugins.antigravity-auth" | "dev.cursorbyok.plugins.antigravity-auth",
            "antigravity",
        ) => crate::provider::providers::antigravity::RESOURCE_TYPE,
        ("dev.nexusor.plugins.github-copilot", "copilot") => {
            crate::provider::providers::copilot::RESOURCE_TYPE
        }
        ("dev.nexusor.plugins.kimi-auth", "kimi") => {
            crate::provider::providers::kimi::RESOURCE_TYPE
        }
        ("dev.nexusor.plugins.claude-code", "claude-code") => {
            crate::provider::providers::claude_code::RESOURCE_TYPE
        }
        _ => return true,
    };
    let now = crate::store::now_ms();
    let Ok(records) = plugins.resources(plugin_id, resource_type).await else {
        return true;
    };
    if records.is_empty() {
        return false;
    }
    let allowed = slot.account_ids.as_deref().unwrap_or(&[]);
    let model_lower = upstream_id.to_ascii_lowercase();
    let is_gemini = model_lower.contains("gemini");
    let is_claude = model_lower.contains("claude")
        || model_lower.contains("sonnet")
        || model_lower.contains("opus");

    records.iter().any(|r| {
        if !allowed.is_empty() && !allowed.contains(&r.id) {
            return false;
        }
        if !r.state.is_ready(now) {
            return false;
        }
        if provider_id == "antigravity" {
            if super::providers::quota::antigravity_model_is_cooling(
                &r.private_data,
                upstream_id,
                now,
            ) {
                return false;
            }
            if let Some(quota) = r.private_data.get("quota") {
                if is_gemini {
                    let g_5h = quota
                        .pointer("/gemini/remaining_percent")
                        .and_then(serde_json::Value::as_f64)
                        .unwrap_or(100.0);
                    let g_5h_reset = quota
                        .pointer("/gemini/reset_at_ms")
                        .and_then(serde_json::Value::as_i64)
                        .unwrap_or(i64::MAX);
                    let g_wk = quota
                        .pointer("/gemini_weekly/remaining_percent")
                        .and_then(serde_json::Value::as_f64)
                        .unwrap_or(100.0);
                    let g_wk_reset = quota
                        .pointer("/gemini_weekly/reset_at_ms")
                        .and_then(serde_json::Value::as_i64)
                        .unwrap_or(i64::MAX);
                    if (g_5h <= 0.0 && now < g_5h_reset) || (g_wk <= 1.0 && now < g_wk_reset) {
                        return false;
                    }
                } else if is_claude {
                    let c_5h = quota
                        .pointer("/claude/remaining_percent")
                        .and_then(serde_json::Value::as_f64)
                        .unwrap_or(100.0);
                    let c_5h_reset = quota
                        .pointer("/claude/reset_at_ms")
                        .and_then(serde_json::Value::as_i64)
                        .unwrap_or(i64::MAX);
                    let c_wk = quota
                        .pointer("/claude_weekly/remaining_percent")
                        .and_then(serde_json::Value::as_f64)
                        .unwrap_or(100.0);
                    let c_wk_reset = quota
                        .pointer("/claude_weekly/reset_at_ms")
                        .and_then(serde_json::Value::as_i64)
                        .unwrap_or(i64::MAX);
                    if (c_5h <= 0.0 && now < c_5h_reset) || (c_wk <= 1.0 && now < c_wk_reset) {
                        return false;
                    }
                }
            }
        }
        true
    })
}

async fn resolve_combo_slots(
    plugins: &PluginRegistry,
    store: &crate::store::Store,
    combo_id: &str,
    strategy: &str,
    mut slots: Vec<crate::store::ComboSlot>,
) -> Vec<crate::store::ComboSlot> {
    if strategy == "most_quota" && slots.len() > 1 {
        let mut scored = Vec::with_capacity(slots.len());
        for slot in slots {
            let score = plugins.slot_quota(&slot).await;
            scored.push((score, slot));
        }
        scored.sort_by(|a, b| b.0.total_cmp(&a.0));
        return scored.into_iter().map(|(_, slot)| slot).collect();
    }
    if strategy == "round_robin" && slots.len() > 1 {
        let counters = get_combo_counters();
        let mut map = counters.lock().await;
        // Drop counters for combos that no longer exist, so deleting and recreating
        // combos does not grow this map for the life of the process.
        let live: std::collections::HashSet<String> = store
            .router_combos()
            .await
            .unwrap_or_default()
            .iter()
            .filter(|combo| combo.enabled && !combo.slots.is_empty())
            .map(|combo| combo.combo_id.clone())
            .collect();
        map.retain(|id, _| live.contains(id));
        let counter = map.entry(combo_id.to_string()).or_insert(0);
        let offset = *counter % slots.len();
        *counter = counter.wrapping_add(1);
        drop(map);

        slots.rotate_left(offset);
        tracing::info!(
            combo_id,
            round_robin_offset = offset,
            "rotated combo slots for round-robin strategy"
        );
    } else if slots.len() > 1 {
        let mut first_available_idx = None;
        for (idx, slot) in slots.iter().enumerate() {
            if is_slot_available(plugins, slot).await {
                first_available_idx = Some(idx);
                break;
            }
        }
        if let Some(idx) = first_available_idx {
            if idx > 0 {
                tracing::info!(
                    combo_id,
                    skipped_cooling_slots = idx,
                    promoted_model = %slots[idx].model_id,
                    "promoted first available ready slot over cooling slots"
                );
                slots.rotate_left(idx);
            }
        }
    }

    slots
}

pub struct ProviderRouter {
    store: Store,
    plugins: PluginRegistry,
    clients: crate::network::NetworkClients,
    request_timeout: Duration,
    stream_idle_timeout: Duration,
}

impl ProviderRouter {
    pub fn new(
        store: Store,
        plugins: PluginRegistry,
        clients: crate::network::NetworkClients,
        request_timeout: Duration,
        stream_idle_timeout: Duration,
    ) -> Self {
        Self {
            store,
            plugins,
            clients,
            request_timeout,
            stream_idle_timeout,
        }
    }
}

impl Provider for ProviderRouter {
    fn stream(
        &self,
        invocation: ModelInvocation,
        cancellation: CancellationToken,
    ) -> ProviderStream {
        let store = self.store.clone();
        let plugins = self.plugins.clone();
        let clients = self.clients.clone();
        let request_timeout = self.request_timeout;
        let stream_idle_timeout = self.stream_idle_timeout;
        Box::pin(try_stream! {
            let mut selected = invocation.request.model.model_id.clone();
            let mut routed_slots = Vec::new();

            if selected == crate::store::AutoRouterConfig::SUBAGENT_ROUTE {
                let config = store.auto_router_config().await?;
                if config.manual_subagent_route().is_none() {
                    Err(crate::Error::Config("Manual subagent route is no longer configured; start a new task with the current selection".into()))?;
                }
                routed_slots = config.subagent_slots;
            }

            // Direct Combo Resolution
            if let Some(combo_id) = selected.strip_prefix("combo:") {
                let combos = store.router_combos().await.unwrap_or_default();
                if let Some(combo) = combos.into_iter().find(|c| c.combo_id == combo_id && c.enabled && !c.slots.is_empty()) {
                    routed_slots = resolve_combo_slots(&plugins, &store, &combo.combo_id, &combo.strategy, combo.slots).await;
                }
            }

            // Smart Router Resolution
            if selected == "auto-smart" || selected == "auto" || selected == "cursor-auto" {
                let intent = super::classifier::classify_intent(&invocation);
                tracing::info!(?intent, "resolving smart router intent for request");

                let auto_config = store.auto_router_config().await.unwrap_or_default();
                let configured_slots = if auto_config.enabled {
                    let primary = match intent {
                        super::classifier::TaskIntent::Coding => &auto_config.coding_slots,
                        super::classifier::TaskIntent::Complex => &auto_config.reasoning_slots,
                        super::classifier::TaskIntent::Fast => &auto_config.fast_slots,
                    };
                    if !primary.is_empty() {
                        Some(primary.clone())
                    } else if !auto_config.coding_slots.is_empty() {
                        Some(auto_config.coding_slots.clone())
                    } else if !auto_config.reasoning_slots.is_empty() {
                        Some(auto_config.reasoning_slots.clone())
                    } else if !auto_config.fast_slots.is_empty() {
                        Some(auto_config.fast_slots.clone())
                    } else {
                        None
                    }
                } else {
                    None
                };

                if let Some(slots_vec) = configured_slots {
                    routed_slots = slots_vec;
                } else {
                    // Fallback to active custom combo
                    let combos = store.router_combos().await.unwrap_or_default();
                    let active_combo = combos.into_iter().find(|c| c.enabled && !c.slots.is_empty());
                    if let Some(combo) = active_combo {
                        routed_slots = resolve_combo_slots(&plugins, &store, &combo.combo_id, &combo.strategy, combo.slots).await;
                    } else {
                        let plugin_models = plugins.configured_models().await;
                        if !plugin_models.is_empty() {
                            // Substring families only. Keep these to real, current
                            // model names: a speculative name silently degrades to
                            // `plugin_models.first()` instead of failing loudly.
                            const FAST: &[&str] = &[
                                "flash", "haiku", "mini", "instant", "lightning", "turbo",
                                "small", "nano",
                            ];
                            const COMPLEX: &[&str] = &[
                                "r1", "o1", "o3", "thinking", "reasoning", "opus", "grok",
                                "glm", "deepseek", "kimi",
                            ];
                            const CODING: &[&str] = &[
                                "coder", "codex", "sonnet", "4o", "pro", "claude", "qwen",
                                "deepseek", "kimi", "llama", "gemini",
                            ];
                            let chosen = match intent {
                                super::classifier::TaskIntent::Fast => {
                                    plugin_models.iter().find(|m| {
                                        let id = m.id.to_ascii_lowercase();
                                        FAST.iter().any(|family| id.contains(family))
                                    }).or_else(|| plugin_models.first())
                                }
                                super::classifier::TaskIntent::Complex => {
                                    plugin_models.iter().find(|m| {
                                        let id = m.id.to_ascii_lowercase();
                                        COMPLEX.iter().any(|family| id.contains(family))
                                    }).or_else(|| plugin_models.first())
                                }
                                super::classifier::TaskIntent::Coding => {
                                    plugin_models.iter().find(|m| {
                                        let id = m.id.to_ascii_lowercase();
                                        CODING.iter().any(|family| id.contains(family))
                                    }).or_else(|| plugin_models.first())
                                }
                            };
                            if let Some(m) = chosen {
                                selected = m.id.clone();
                            }
                        } else if let Ok(models) = store.models().await {
                            if let Some(first) = models.first() {
                                selected = first.model_hash.clone();
                            }
                        }
                    }
                    tracing::info!(resolved_model = %selected, "smart router dispatched fallback model");
                }
            }
            let candidates = if routed_slots.is_empty() {
                vec![(selected.clone(), invocation.slot_account_ids.clone(), invocation.slot_strategy)]
            } else {
                routed_slots.into_iter().map(|slot| {
                    let strategy = slot.pool_strategy();
                    (slot.model_id, slot.account_ids.filter(|ids| !ids.is_empty()), strategy)
                }).collect()
            };
            // Probe candidate streams until the first valid response event.
            let mut active = None;
            let mut last_error = None;
            for (attempt, (candidate, slot_accounts, slot_strategy)) in candidates.iter().enumerate() {
                let candidate: &str = candidate;
                let mut attempt_invocation = invocation.clone();
                attempt_invocation.slot_account_ids = slot_accounts.clone();
                attempt_invocation.slot_strategy = *slot_strategy;
                if attempt > 0 {
                    attempt_invocation.call_id =
                        format!("{}:combo-{attempt}", invocation.call_id);
                }

                // If candidate model does not natively support vision, run Vision Sidecar
                attempt_invocation = super::vision_sidecar::process_vision_sidecar(
                    &store,
                    &plugins,
                    &clients,
                    attempt_invocation,
                    candidate,
                    &cancellation,
                ).await?;

                let built = build_model_stream(
                    &store,
                    &plugins,
                    &clients,
                    request_timeout,
                    candidate,
                    &attempt_invocation,
                    &cancellation,
                )
                .await;
                let (recorder, guard, mut candidate_stream) = match built {
                    Ok(parts) => parts,
                    Err(error) => {
                        tracing::warn!(model = %candidate, attempt, %error, "combo model setup failed");
                        let call_id = if attempt > 0 {
                            format!("{}:combo-{attempt}", invocation.call_id)
                        } else {
                            invocation.call_id.clone()
                        };
                        let new_call = NewLlmCall {
                            call_id: call_id.clone(),
                            run_id: invocation.run_id.clone(),
                            conversation_id: invocation.conversation_id.clone(),
                            provider_call_index: (invocation.provider_call_index.min(i64::MAX as u64) as i64) + (attempt as i64),
                            model_hash: candidate.to_string(),
                            provider_type: ProviderType::Plugin,
                            provider_url: String::new(),
                            request_type: ProviderType::Plugin,
                            request_url: String::new(),
                            model_id: candidate.to_string(),
                            display_name: candidate.to_string(),
                            reasoning_effort: invocation.request.model.reasoning.effort.clone(),
                            fast: invocation.request.model.latency == crate::model::ModelLatency::Fast,
                            message_count: invocation.request.history.len(),
                            tool_count: invocation.request.prompt.tools.len(),
                            detailed: false,
                        };
                        let _ = store.start_llm_call(&new_call).await;
                        let _ = store.finish_llm_call(
                            &call_id,
                            "error",
                            None,
                            0,
                            Some("setup_error"),
                            Some(&error.to_string()),
                        ).await;
                        last_error = Some(error);
                        log_combo_next(&candidates, attempt, candidate);
                        continue;
                    }
                };

                let mut pending: Vec<Result<super::ModelEvent>> = Vec::new();
                let mut probe_error = None;
                loop {
                    let item = match next_provider_event(
                        &mut candidate_stream,
                        stream_idle_timeout,
                    )
                    .await
                    {
                        Ok(Some(item)) => item,
                        Ok(None) => {
                            if !pending.iter().any(|ev| ev.as_ref().is_ok_and(super::is_valid_response_event)) {
                                probe_error = Some(Error::Provider(format!(
                                    "provider stream ended prematurely without producing any response content for candidate '{candidate}'"
                                )));
                            }
                            break;
                        }
                        Err(_) => {
                            probe_error =
                                Some(stream_idle_timeout_error(stream_idle_timeout));
                            break;
                        }
                    };
                    match item {
                        Ok(event) => {
                            let response_started = super::is_valid_response_event(&event);
                            pending.push(Ok(event));
                            if response_started {
                                break;
                            }
                        }
                        Err(error) => {
                            probe_error = Some(normalize_provider_stream_error(
                                error,
                                request_timeout,
                            ));
                            break;
                        }
                    }
                }

                if probe_outcome(&pending) == ProbeOutcome::Commit {
                    // A valid response arrived, or the stream ended cleanly without
                    // yielding one; in either case this candidate is committed.
                    active = Some((candidate, recorder, guard, candidate_stream, pending));
                    break;
                }

                let error = probe_error.expect("probe_error is Some here");
                if let Err(finish_error) = recorder.failed(&error).await {
                    tracing::warn!(model = %candidate, %finish_error, "failed to mark combo attempt failed");
                }
                last_error = Some(error);
                log_combo_next(&candidates, attempt, candidate);
            }
            // `unreachable!()` closes the diverging `let-else` arm; it is never reached.
            let Some((selected, recorder, _cancel_on_drop, mut stream, mut pending)) = active else {
                Err(last_error.unwrap_or_else(|| Error::Provider("combo exhausted without a response".into())))?;
                unreachable!();
            };
            for item in pending.drain(..) {
                let event = item?;
                recorder.event(&event).await?;
                yield event;
            }

            let stream_started = std::time::Instant::now();
            tracing::debug!(
                model = %selected,
                request_timeout_ms = request_timeout.as_millis() as u64,
                stream_idle_timeout_ms = stream_idle_timeout.as_millis() as u64,
                "provider stream created"
            );
            let mut last_event_time = std::time::Instant::now();
            let mut event_count: u64 = 0;
            loop {
                let event = match next_provider_event(&mut stream, stream_idle_timeout).await {
                    Ok(Some(event)) => event,
                    Ok(None) => break,
                    Err(_) => {
                        let elapsed_ms = stream_started.elapsed().as_millis() as u64;
                        let error = stream_idle_timeout_error(stream_idle_timeout);
                        tracing::warn!(
                            error = %error,
                            elapsed_ms,
                            event_count,
                            idle_timeout_ms = stream_idle_timeout.as_millis() as u64,
                            "provider stream idle timeout"
                        );
                        Err(error)
                    }
                };
                let now = std::time::Instant::now();
                let gap_ms = now.duration_since(last_event_time).as_millis() as u64;
                let elapsed_ms = now.duration_since(stream_started).as_millis() as u64;
                event_count += 1;
                match event {
                    Ok(event) => {
                        if gap_ms > 5000 {
                            tracing::debug!(
                                gap_ms,
                                elapsed_ms,
                                event = event_name(&event),
                                event_count,
                                "slow gap detected between provider events"
                            );
                        }
                        recorder.event(&event).await?;
                        last_event_time = now;
                        yield event;
                    }
                    Err(error) => {
                        let error = normalize_provider_stream_error(error, request_timeout);
                        tracing::debug!(
                            error = %error,
                            elapsed_ms,
                            gap_ms,
                            event_count,
                            "provider stream error"
                        );
                        recorder.failed(&error).await?;
                        Err(error)?;
                    }
                }
            }
            finish_stream(&recorder, &cancellation).await?;
        })
    }
}

fn event_name(event: &super::ModelEvent) -> &'static str {
    match event {
        super::ModelEvent::Start { .. } => "Start",
        super::ModelEvent::TextStart => "TextStart",
        super::ModelEvent::TextDelta(_) => "TextDelta",
        super::ModelEvent::TextEnd => "TextEnd",
        super::ModelEvent::ThinkingStart => "ThinkingStart",
        super::ModelEvent::ThinkingDelta(_) => "ThinkingDelta",
        super::ModelEvent::ThinkingEnd => "ThinkingEnd",
        super::ModelEvent::ToolCallStart { .. } => "ToolCallStart",
        super::ModelEvent::ToolCallArgumentsDelta { .. } => "ToolCallArgsDelta",
        super::ModelEvent::ToolCallEnd { .. } => "ToolCallEnd",
        super::ModelEvent::ProviderReplayState(_) => "ReplayState",
        super::ModelEvent::Usage(_) => "Usage",
        super::ModelEvent::Done(_) => "Done",
    }
}

pub(crate) async fn resolve_model_target(
    store: &Store,
    plugins: Option<&PluginRegistry>,
    selected: &str,
) -> Result<Option<String>> {
    if selected.starts_with(ADAPTER_ID_PREFIX) {
        return Ok(Some(selected.to_string()));
    }
    if store.model(selected).await?.is_some() {
        return Ok(Some(selected.to_string()));
    }
    let mut matches: Vec<String> = store
        .models()
        .await?
        .into_iter()
        .filter(|model| model.model_id == selected)
        .map(|model| model.model_hash)
        .collect();
    if let Some(plugins) = plugins {
        matches.extend(
            plugins
                .configured_models()
                .await
                .into_iter()
                .filter(|model| model.model_id == selected)
                .map(|model| model.id),
        );
    }
    matches.sort();
    matches.dedup();
    match matches.len() {
        0 => Ok(None),
        1 => Ok(matches.pop()),
        _ => Err(Error::Provider(format!(
            "ambiguous model {selected}; select its unique Nexusor catalog ID"
        ))),
    }
}

pub(crate) async fn build_model_stream(
    store: &Store,
    plugins: &PluginRegistry,
    clients: &crate::network::NetworkClients,
    request_timeout: Duration,
    selected_raw: &str,
    invocation: &ModelInvocation,
    cancellation: &CancellationToken,
) -> Result<(CallRecorder, CancelOnDrop, ProviderStream)> {
    let resolved = resolve_model_target(store, Some(plugins), selected_raw)
        .await?
        .unwrap_or_else(|| selected_raw.to_owned());
    let selected = resolved.as_str();
    if selected.starts_with(ADAPTER_ID_PREFIX) {
        let plan = plugins.plan_model(selected).await?;
        let recorder = start_recorder(
            store,
            invocation,
            selected,
            &plan.model.display_name,
            ProviderType::Plugin,
            &plan.request_url,
            &plan.model.model_id,
        )
        .await?;
        let guard = recorder.cancel_on_drop();
        let mut routed = invocation.clone();
        routed.request.model.model_id = selected.to_string();
        routed.request.model.display_name = Some(plan.model.display_name.clone());
        if let Some(tokens) = plan.model.max_output_tokens {
            routed.request.model.max_output_tokens.get_or_insert(tokens);
        }
        let provider: Arc<dyn Provider> =
            Arc::new(NormalizedProvider::new(Arc::new(PluginModelProvider {
                registry: plugins.clone(),
                recorder: recorder.clone(),
            })));
        Ok((
            recorder,
            guard,
            provider.stream(routed, cancellation.clone()),
        ))
    } else {
        let mut routed = invocation.clone();
        let model = store
            .model(selected)
            .await?
            .ok_or_else(|| Error::Provider(format!("unknown model: {selected}")))?;
        let provider_type = model.provider_type();
        let request_url = model.request_url()?;
        model.configure(&mut routed.request.model);
        routed.request.model.extra_params = super::request_template::render_json_strings(
            model.extra_params(),
            &invocation.conversation_id,
        );
        routed.request.model.model_id = model.model_id.clone();
        let recorder = start_recorder(
            store,
            invocation,
            &model.model_hash,
            &model.display_name,
            provider_type,
            &request_url,
            &model.model_id,
        )
        .await?;
        let guard = recorder.cancel_on_drop();
        let api_key = select_api_key(&model.api_key);
        let headers = if model.custom_headers_enabled {
            custom_headers(&model.custom_headers, &invocation.conversation_id)?
        } else {
            reqwest::header::HeaderMap::new()
        };
        let config = ProviderConfig {
            kind: provider_kind(provider_type),
            request_url,
            api_key,
            custom_headers: headers,
            max_output_tokens: model.max_output_tokens(),
            request_timeout,
            allowed_body_fields: None,
        };
        let client = clients.provider_client(request_timeout).await?;
        let provider = build_observed(&config, recorder.clone(), client)?;
        Ok((
            recorder,
            guard,
            provider.stream(routed, cancellation.clone()),
        ))
    }
}

fn probe_outcome(events: &[Result<super::ModelEvent>]) -> ProbeOutcome {
    if events
        .iter()
        .any(|item| item.as_ref().is_ok_and(super::is_valid_response_event))
    {
        ProbeOutcome::Commit
    } else {
        ProbeOutcome::Retry
    }
}

#[derive(Debug, PartialEq, Eq)]
enum ProbeOutcome {
    Commit,
    Retry,
}

fn log_combo_next(candidates: &[RoutingCandidate], attempt: usize, candidate: &str) {
    if attempt + 1 < candidates.len() {
        tracing::warn!(model = %candidate, next_model = %candidates[attempt + 1].0, "trying next combo model after pre-response failure");
    }
}

async fn start_recorder(
    store: &Store,
    invocation: &ModelInvocation,
    model_hash: &str,
    display_name: &str,
    provider_type: ProviderType,
    request_url: &str,
    model_id: &str,
) -> Result<CallRecorder> {
    CallRecorder::start(
        store.clone(),
        NewLlmCall {
            call_id: invocation.call_id.clone(),
            run_id: invocation.run_id.clone(),
            conversation_id: invocation.conversation_id.clone(),
            provider_call_index: invocation.provider_call_index.min(i64::MAX as u64) as i64,
            model_hash: model_hash.into(),
            provider_type,
            provider_url: request_url.into(),
            request_type: provider_type,
            request_url: request_url.into(),
            model_id: model_id.into(),
            display_name: display_name.into(),
            reasoning_effort: invocation.request.model.reasoning.effort.clone(),
            fast: invocation.request.model.latency == ModelLatency::Fast,
            message_count: invocation.request.history.len(),
            tool_count: invocation.request.prompt.tools.len(),
            detailed: false,
        },
    )
    .await
}

async fn finish_stream(recorder: &CallRecorder, cancellation: &CancellationToken) -> Result<()> {
    if recorder.is_finished() {
        return Ok(());
    }
    if cancellation.is_cancelled() {
        recorder.cancelled().await
    } else {
        let error = Error::Provider("provider stream ended without Done".into());
        recorder.failed(&error).await?;
        Err(error)
    }
}

/// 插件模型的 Provider 实现;对路由与规范化层完全等同于内置 Provider。
struct PluginModelProvider {
    registry: PluginRegistry,
    recorder: CallRecorder,
}

impl Provider for PluginModelProvider {
    fn stream(
        &self,
        invocation: ModelInvocation,
        cancellation: CancellationToken,
    ) -> ProviderStream {
        self.registry
            .stream_model(invocation, cancellation, self.recorder.clone())
    }
}

fn provider_kind(provider_type: ProviderType) -> ProviderKind {
    match provider_type {
        ProviderType::OpenAiChat => ProviderKind::OpenAiChat,
        ProviderType::OpenAiResponses => ProviderKind::OpenAiResponses,
        ProviderType::Anthropic => ProviderKind::Anthropic,
        // 内置模型的 provider_type 只来自 ModelType,不可能是插件。
        ProviderType::Plugin => unreachable!("plugin models never use built-in provider configs"),
    }
}

async fn next_provider_event(
    stream: &mut ProviderStream,
    idle_timeout: Duration,
) -> std::result::Result<Option<Result<super::ModelEvent>>, tokio::time::error::Elapsed> {
    tokio::time::timeout(idle_timeout, stream.next()).await
}

fn stream_idle_timeout_error(idle_timeout: Duration) -> Error {
    Error::Provider(format!(
        "provider stream idle timeout: no events received for {} seconds ({} minutes)",
        idle_timeout.as_secs(),
        idle_timeout.as_secs() / 60
    ))
}

fn request_timeout_error(request_timeout: Duration) -> Error {
    Error::Provider(format!(
        "provider request timed out after {} seconds ({} minutes)",
        request_timeout.as_secs(),
        request_timeout.as_secs() / 60
    ))
}

fn normalize_provider_stream_error(error: Error, request_timeout: Duration) -> Error {
    match error {
        Error::Http(source) if source.is_timeout() => request_timeout_error(request_timeout),
        Error::Http(source) if source.is_body() => Error::Provider(format!(
            "provider stream transport failed while reading the response body: {}",
            root_error_message(&source)
        )),
        error => error,
    }
}

fn root_error_message(error: &(dyn std::error::Error + 'static)) -> String {
    let mut current = error;
    while let Some(source) = current.source() {
        current = source;
    }
    current.to_string()
}

fn custom_headers(
    value: &serde_json::Value,
    conversation_id: &str,
) -> Result<reqwest::header::HeaderMap> {
    let rendered = super::request_template::render_json_strings(value, conversation_id);
    let object = rendered
        .as_object()
        .ok_or_else(|| Error::Config("custom headers must be an object".into()))?;
    let mut headers = reqwest::header::HeaderMap::new();
    for (name, value) in object {
        let name = reqwest::header::HeaderName::from_bytes(name.as_bytes())
            .map_err(|error| Error::Config(format!("invalid custom header name: {error}")))?;
        let value = value
            .as_str()
            .ok_or_else(|| Error::Config("custom header values must be strings".into()))?;
        let value = reqwest::header::HeaderValue::from_str(value)
            .map_err(|error| Error::Config(format!("invalid custom header value: {error}")))?;
        headers.insert(name, value);
    }
    Ok(headers)
}

pub fn build(config: &ProviderConfig) -> Result<Arc<dyn Provider>> {
    build_inner(config, None, None)
}

fn build_observed(
    config: &ProviderConfig,
    recorder: CallRecorder,
    client: reqwest::Client,
) -> Result<Arc<dyn Provider>> {
    build_inner(config, Some(recorder), Some(client))
}

fn build_inner(
    config: &ProviderConfig,
    recorder: Option<CallRecorder>,
    client: Option<reqwest::Client>,
) -> Result<Arc<dyn Provider>> {
    let client = match client {
        Some(client) => client,
        None => reqwest::Client::builder()
            .tcp_nodelay(true)
            .tcp_keepalive(Duration::from_secs(60))
            .timeout(config.request_timeout)
            .build()?,
    };
    let provider: Arc<dyn Provider> = match config.kind {
        ProviderKind::OpenAiChat => {
            Arc::new(OpenAiChatProvider::new(client, config.clone()).with_recorder(recorder))
        }
        ProviderKind::OpenAiResponses | ProviderKind::Codex => {
            Arc::new(OpenAiResponsesProvider::new(client, config.clone()).with_recorder(recorder))
        }
        ProviderKind::Anthropic => {
            Arc::new(AnthropicProvider::new(client, config.clone()).with_recorder(recorder))
        }
    };
    Ok(Arc::new(NormalizedProvider::new(provider)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::ModelEvent;

    #[tokio::test]
    async fn model_resolution_requires_exact_unambiguous_identity() {
        let store = Store::connect("sqlite::memory:").await.unwrap();
        let mut input: crate::model::ModelConfigInput = serde_json::from_value(serde_json::json!({
            "display_name": "My model", "type": "openai", "base_url": "https://provider.example/v1",
            "api_key": "test", "tooltip_data": "Routing test model", "model_id": "gpt-test"
        }))
        .unwrap();
        let first = store.create_model(&input).await.unwrap();
        assert_eq!(
            resolve_model_target(&store, None, "gpt-test")
                .await
                .unwrap(),
            Some(first.model_hash.clone())
        );
        for unknown in [
            "gpt",
            "gpt-test-fast",
            "My model",
            "GPT-TEST",
            "claude-test",
        ] {
            assert_eq!(
                resolve_model_target(&store, None, unknown).await.unwrap(),
                None
            );
        }
        input.base_url = "https://second.example/v1".into();
        let second = store.create_model(&input).await.unwrap();
        assert!(resolve_model_target(&store, None, "gpt-test")
            .await
            .unwrap_err()
            .to_string()
            .contains("ambiguous"));
        for model in [first, second] {
            assert_eq!(
                resolve_model_target(&store, None, &model.model_hash)
                    .await
                    .unwrap(),
                Some(model.model_hash)
            );
        }
    }

    #[test]
    fn renders_cursor_conversation_id_in_custom_header_values() {
        let template = serde_json::json!({
            "x-opencode-session-id": "{{SessionId}}",
            "x-label": "cursor/{{SessionId}}"
        });

        let headers = custom_headers(&template, "cursor-conversation-id").unwrap();

        assert_eq!(
            headers.get("x-opencode-session-id").unwrap(),
            "cursor-conversation-id"
        );
        assert_eq!(
            headers.get("x-label").unwrap(),
            "cursor/cursor-conversation-id"
        );
        assert_eq!(template["x-opencode-session-id"], "{{SessionId}}");
    }

    #[tokio::test]
    async fn combo_rotation_and_quota_order_use_real_registry_state() {
        use crate::plugin::{ResourceRecord, ResourceState};
        use crate::store::ComboSlot;
        let root = tempfile::tempdir().unwrap();
        let store = Store::connect("sqlite::memory:").await.unwrap();
        let plugins = PluginRegistry::for_test(store.clone(), root.path());
        let plugin = "dev.nexusor.examples.codex-auth";
        let resource_type = crate::provider::providers::codex::RESOURCE_TYPE;
        let records = [("low", 80), ("high", 10)]
            .into_iter()
            .map(|(id, used)| ResourceRecord {
                id: id.into(),
                key: id.into(),
                state: ResourceState::Ready,
                private_data: serde_json::json!({"quota":{"primary_window":{"used_percent":used}}}),
                created_at_ms: 0,
                updated_at_ms: 0,
            })
            .collect();
        plugins
            .restore_resources(plugin, resource_type, records)
            .await
            .unwrap();
        let slots: Vec<_> = ["low", "high"]
            .into_iter()
            .map(|id| ComboSlot {
                model_id: format!("plugin:{plugin}/codex/test"),
                account_ids: Some(vec![id.into()]),
                slot_strategy: "round_robin".into(),
            })
            .collect();
        let ranked =
            resolve_combo_slots(&plugins, &store, "quota-test", "most_quota", slots.clone()).await;
        assert_eq!(ranked[0].account_ids, slots[1].account_ids);
        // Round-robin state is pruned to persisted combos, so the combo has to exist
        // for consecutive calls to advance the rotation.
        let combo = crate::store::RouterCombo {
            combo_id: "rotation-test".into(),
            name: "rotation".into(),
            description: None,
            models: Vec::new(),
            enabled: true,
            strategy: "round_robin".into(),
            slots: slots.clone(),
            created_at_ms: 0,
            updated_at_ms: 0,
        };
        store.upsert_router_combo(combo).await.unwrap();
        let first = resolve_combo_slots(
            &plugins,
            &store,
            "rotation-test",
            "round_robin",
            slots.clone(),
        )
        .await;
        let second = resolve_combo_slots(
            &plugins,
            &store,
            "rotation-test",
            "round_robin",
            slots.clone(),
        )
        .await;
        assert_eq!(first, slots);
        assert_eq!(second[0], slots[1]);
        assert_eq!(second[1], slots[0]);
    }

    #[tokio::test]
    async fn pending_provider_event_hits_the_idle_timeout() {
        let mut stream: ProviderStream = Box::pin(futures_util::stream::pending());

        let result = next_provider_event(&mut stream, Duration::from_millis(1)).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn antigravity_slot_cooldown_is_family_and_account_scoped() {
        use crate::plugin::{ResourceRecord, ResourceState};
        use crate::store::ComboSlot;
        let root = tempfile::tempdir().unwrap();
        let plugins = PluginRegistry::for_test(
            Store::connect("sqlite::memory:").await.unwrap(),
            root.path(),
        );
        let plugin = "dev.nexusor.plugins.antigravity-auth";
        plugins.restore_resources(plugin, "google-account", ["a", "b"].into_iter().map(|id| ResourceRecord {
            id: id.into(), key: id.into(), state: ResourceState::Ready,
            private_data: if id == "a" { serde_json::json!({"modelFamilyCooldowns": {"claude": crate::store::now_ms() + 600_000}}) } else { serde_json::json!({}) },
            created_at_ms: 0, updated_at_ms: 0,
        }).collect()).await.unwrap();
        let mut slot = ComboSlot {
            model_id: format!("plugin:{plugin}/antigravity/claude-sonnet-4-6"),
            account_ids: Some(vec!["a".into()]),
            slot_strategy: "failover".into(),
        };
        assert!(!is_slot_available(&plugins, &slot).await);
        slot.account_ids = Some(vec!["b".into()]);
        assert!(is_slot_available(&plugins, &slot).await);
        slot.account_ids = Some(vec!["a".into()]);
        slot.model_id = format!("plugin:{plugin}/antigravity/gemini-3.8-flash");
        assert!(is_slot_available(&plugins, &slot).await);
    }

    #[test]
    fn probe_result_commits_once_a_valid_response_event_arrives() {
        let pending: Vec<Result<ModelEvent>> = vec![Ok(ModelEvent::TextDelta("hi".into()))];
        assert_eq!(probe_outcome(&pending), ProbeOutcome::Commit);
    }

    #[test]
    fn probe_result_retries_when_no_valid_response_event_arrived() {
        let without_response: Vec<Result<ModelEvent>> = Vec::new();
        assert_eq!(probe_outcome(&without_response), ProbeOutcome::Retry);
    }

    #[test]
    fn timeout_errors_state_the_boundary_and_duration() {
        let Error::Provider(idle) = stream_idle_timeout_error(Duration::from_secs(30 * 60)) else {
            panic!("idle timeout must be a provider error");
        };
        assert_eq!(
            idle,
            "provider stream idle timeout: no events received for 1800 seconds (30 minutes)"
        );

        let Error::Provider(request) = request_timeout_error(Duration::from_secs(60 * 60)) else {
            panic!("request timeout must be a provider error");
        };
        assert_eq!(
            request,
            "provider request timed out after 3600 seconds (60 minutes)"
        );
    }
}
