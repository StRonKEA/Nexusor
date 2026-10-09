use super::*;

pub async fn default_model_for_cli(
    State(registry): State<TransportRegistry>,
) -> Result<Response<Body>> {
    let models = registry.store().models().await?;
    let plugin_models = configured_plugin_models(&registry).await;
    Ok(local_response(
        agent::GetDefaultModelForCliResponse {
            model: default_model_details(&models, &plugin_models),
        }
        .encode_to_vec(),
    ))
}

pub async fn default_model(State(registry): State<TransportRegistry>) -> Result<Response<Body>> {
    let models = registry.store().models().await?;
    let plugin_models = configured_plugin_models(&registry).await;
    Ok(local_response(
        default_model_response(&models, &plugin_models).encode_to_vec(),
    ))
}

pub async fn default_model_nudge(
    State(registry): State<TransportRegistry>,
) -> Result<Response<Body>> {
    let models = registry.store().models().await?;
    let plugin_models = configured_plugin_models(&registry).await;
    Ok(local_response(
        default_model_nudge_response(&models, &plugin_models).encode_to_vec(),
    ))
}

async fn configured_plugin_models(registry: &TransportRegistry) -> Vec<PluginModelDescriptor> {
    match registry.plugins() {
        Some(plugins) => plugins.configured_models().await,
        None => Vec::new(),
    }
}

pub(crate) fn default_model_details(
    models: &[ModelConfig],
    plugin_models: &[PluginModelDescriptor],
) -> Option<agent::ModelDetails> {
    models
        .first()
        .map(usable_model)
        .or_else(|| plugin_models.first().map(usable_plugin_model))
}

fn default_model_id<'a>(
    models: &'a [ModelConfig],
    plugin_models: &'a [PluginModelDescriptor],
) -> &'a str {
    models
        .first()
        .map(|model| model.model_hash.as_str())
        .or_else(|| plugin_models.first().map(|model| model.id.as_str()))
        .unwrap_or_default()
}

pub(crate) fn default_model_response(
    models: &[ModelConfig],
    plugin_models: &[PluginModelDescriptor],
) -> DefaultModelResponse {
    let model = default_model_id(models, plugin_models).to_owned();
    DefaultModelResponse {
        thinking_model: model.clone(),
        model,
        max_mode: false,
        next_default_set_date: String::new(),
    }
}

pub(crate) fn default_model_nudge_response(
    models: &[ModelConfig],
    plugin_models: &[PluginModelDescriptor],
) -> DefaultModelNudgeDataResponse {
    DefaultModelNudgeDataResponse {
        nudge_date: "0".into(),
        should_default_switch_on_new_chat: false,
        models_with_no_default_switch: models
            .iter()
            .map(|model| model.model_hash.clone())
            .chain(plugin_models.iter().map(|model| model.id.clone()))
            .collect(),
        conversion_model_override: String::new(),
    }
}
