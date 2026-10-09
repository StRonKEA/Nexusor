//! Vision Sidecar engine: converts image inputs into detailed text/code extractions
//! when the target model does not natively support vision.

use futures_util::StreamExt;
use tokio_util::sync::CancellationToken;

use crate::{
    model::{ContentPart, ModelInvocation, ProjectedContent, ProjectedMessage, Role},
    plugin::PluginRegistry,
    provider::ModelEvent,
    store::Store,
    Result,
};

pub fn has_images(invocation: &ModelInvocation) -> bool {
    invocation
        .request
        .history
        .iter()
        .any(|msg| match &msg.content {
            ProjectedContent::Parts(parts) => {
                parts.iter().any(|p| matches!(p, ContentPart::Image { .. }))
            }
            _ => false,
        })
}

pub async fn model_supports_images(
    store: &Store,
    plugins: &PluginRegistry,
    model_id: &str,
) -> bool {
    if model_id.starts_with(crate::plugin::ADAPTER_ID_PREFIX) {
        if let Ok(desc) = plugins.model_descriptor(model_id).await {
            return desc.images;
        }
    }
    let lower = model_id.to_lowercase();
    if lower.contains("o3-mini")
        || lower.contains("o1-mini")
        || lower.contains("o1-preview")
        || lower.contains("deepseek-coder")
        || lower.contains("deepseek-r1-distill")
        || lower.contains("codex-auto-review")
    {
        return false;
    }
    if let Ok(models) = store.models().await {
        if let Some(m) = models
            .into_iter()
            .find(|m| m.model_hash == model_id || m.model_id == model_id)
        {
            let lower_id = m.model_id.to_lowercase();
            if lower_id.contains("o3-mini")
                || lower_id.contains("o1-mini")
                || lower_id.contains("o1-preview")
                || lower_id.contains("deepseek-coder")
            {
                return false;
            }
        }
    }
    true
}

pub async fn select_vision_candidates(
    store: &Store,
    plugins: &PluginRegistry,
) -> Vec<(String, Option<Vec<String>>)> {
    let mut candidates = Vec::new();
    let config = store.auto_router_config().await.unwrap_or_default();

    if !config.vision_auto && !config.vision_slots.is_empty() {
        for slot in config.vision_slots {
            candidates.push((slot.model_id, slot.account_ids));
        }
        if !candidates.is_empty() {
            return candidates;
        }
    }

    let configured = plugins.configured_models().await;
    // 1. First priority: High-speed, high-quality vision models (Flash, 4o)
    for pm in configured
        .iter()
        .filter(|m| m.images && (m.model_id.contains("flash") || m.model_id.contains("4o")))
    {
        candidates.push((pm.id.clone(), None));
    }

    // 2. Second priority: Other vision-capable models (Sonnet, Pro, Opus)
    for pm in configured.iter().filter(|m| m.images) {
        if !candidates.iter().any(|(id, _)| id == &pm.id) {
            candidates.push((pm.id.clone(), None));
        }
    }

    // 3. Third priority: Builtin models from store
    if let Ok(models) = store.models().await {
        for m in models
            .into_iter()
            .filter(|m| m.model_id.contains("flash") || m.model_id.contains("4o"))
        {
            if !candidates.iter().any(|(id, _)| id == &m.model_hash) {
                candidates.push((m.model_hash, None));
            }
        }
    }

    if candidates.is_empty() {
        candidates.push((
            "plugin:dev.nexusor.plugins.antigravity-auth/antigravity/gemini-2.5-flash".into(),
            None,
        ));
    }

    candidates
}

pub async fn process_vision_sidecar(
    store: &Store,
    plugins: &PluginRegistry,
    clients: &crate::network::NetworkClients,
    mut invocation: ModelInvocation,
    target_model: &str,
    cancellation: &CancellationToken,
) -> Result<ModelInvocation> {
    if !has_images(&invocation) {
        return Ok(invocation);
    }

    if model_supports_images(store, plugins, target_model).await {
        return Ok(invocation);
    }

    tracing::info!(
        target_model,
        "Vision Sidecar triggered: target model does not support images, extracting visuals"
    );

    let candidates = select_vision_candidates(store, plugins).await;
    let run_id = invocation.run_id.clone();
    let conversation_id = invocation.conversation_id.clone();

    for msg in &mut invocation.request.history {
        if let ProjectedContent::Parts(parts) = &mut msg.content {
            let mut new_parts = Vec::with_capacity(parts.len());
            for part in parts.drain(..) {
                match part {
                    ContentPart::Image { mime_type, data } => {
                        let mut extracted = None;
                        for (model_id, accounts) in &candidates {
                            match analyze_image(
                                store,
                                plugins,
                                clients,
                                model_id,
                                accounts.as_deref(),
                                &mime_type,
                                &data,
                                &run_id,
                                &conversation_id,
                                cancellation,
                            )
                            .await
                            {
                                Ok(description) if !description.trim().is_empty() => {
                                    extracted = Some(description);
                                    break;
                                }
                                Ok(_) => continue,
                                Err(err) => {
                                    tracing::warn!(
                                        %err,
                                        model = %model_id,
                                        "Vision Sidecar candidate failed, trying next candidate in chain"
                                    );
                                }
                            }
                        }

                        match extracted {
                            Some(description) => {
                                new_parts.push(ContentPart::Text {
                                    text: format!(
                                        "\n[Visual Content / Image Transcription]:\n{}\n",
                                        description.trim()
                                    ),
                                });
                            }
                            None => {
                                tracing::warn!("All Vision Sidecar candidates exhausted, passing placeholder notice");
                                new_parts.push(ContentPart::Text {
                                    text: "[An image was attached by the user, but the vision sidecar models were unable to transcribe it.]".into(),
                                });
                            }
                        }
                    }
                    other => new_parts.push(other),
                }
            }
            *parts = new_parts;
        }
    }

    Ok(invocation)
}

#[allow(clippy::too_many_arguments)]
async fn analyze_image(
    store: &Store,
    plugins: &PluginRegistry,
    clients: &crate::network::NetworkClients,
    vision_model: &str,
    allowed_accounts: Option<&[String]>,
    mime_type: &str,
    data: &[u8],
    run_id: &str,
    conversation_id: &str,
    cancellation: &CancellationToken,
) -> Result<String> {
    let vision_invocation = ModelInvocation {
        call_id: format!("vision-sidecar:{}", uuid::Uuid::new_v4()),
        run_id: run_id.to_string(),
        conversation_id: conversation_id.to_string(),
        provider_call_index: 9999,
        request: crate::model::ModelRequest {
            model: crate::model::ModelSpec::new(vision_model),
            history: vec![ProjectedMessage {
                message_id: "vision-msg-1".into(),
                role: Role::User,
                content: ProjectedContent::Parts(vec![
                    ContentPart::Text {
                        text: "Analyze this image in detail for a software developer. Extract all code, error messages, stack traces, UI components, and logs verbatim into structured Markdown text and code blocks:".into(),
                    },
                    ContentPart::Image {
                        mime_type: mime_type.to_string(),
                        data: data.to_vec(),
                    },
                ]),
            }],
            prompt: crate::model::PromptSpec {
                instructions: "You are an expert developer vision assistant. Transcribe and extract all visual text, code, logs, and technical details accurately without speculation.".into(),
                tools: Vec::new(),
            },
        },
        slot_account_ids: allowed_accounts.map(|s| s.to_vec()),
        slot_strategy: None,
    };

    let (_recorder, _guard, mut stream) = super::router::build_model_stream(
        store,
        plugins,
        clients,
        std::time::Duration::from_secs(30),
        vision_model,
        &vision_invocation,
        cancellation,
    )
    .await?;

    let mut result_text = String::new();
    while let Some(event) = stream.next().await {
        match event {
            Ok(ModelEvent::TextDelta(delta)) => {
                result_text.push_str(&delta);
            }
            Ok(ModelEvent::Done(_)) => break,
            Err(e) => return Err(e),
            _ => {}
        }
    }

    Ok(result_text)
}
