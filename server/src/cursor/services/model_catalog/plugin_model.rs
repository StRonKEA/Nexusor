use super::*;

pub(crate) fn cli_local_model_credentials() -> agent::model_details::Credentials {
    agent::model_details::Credentials::ApiKeyCredentials(agent::ApiKeyCredentials {
        api_key: CLI_LOCAL_MODEL_API_KEY.into(),
        base_url: None,
    })
}

pub(crate) fn usable_plugin_model(model: &PluginModelDescriptor) -> agent::ModelDetails {
    agent::ModelDetails {
        model_id: model.id.clone(),
        display_model_id: model.id.clone(),
        display_name: model.display_name.clone(),
        display_name_short: model.display_name.clone(),
        thinking_details: Some(agent::ThinkingDetails::default()),
        credentials: Some(cli_local_model_credentials()),
        ..Default::default()
    }
}

pub(crate) fn usable_model(model: &ModelConfig) -> agent::ModelDetails {
    agent::ModelDetails {
        model_id: model.model_hash.clone(),
        display_model_id: model.model_hash.clone(),
        display_name: model.display_name.clone(),
        display_name_short: model.display_name.clone(),
        thinking_details: Some(agent::ThinkingDetails::default()),
        credentials: Some(cli_local_model_credentials()),
        ..Default::default()
    }
}

pub(crate) fn available_plugin_model(model: &PluginModelDescriptor) -> AvailableModel {
    let tooltip = TooltipData {
        markdown_content: model.description.clone(),
    };
    let display_name = pretty_model_name(&model.display_name);
    let contexts = context_options(None);
    let thinking = !(model.plugin_id == "dev.nexusor.plugins.github-copilot"
        && model.provider_id == "copilot"
        && crate::provider::providers::copilot::models::rejects_reasoning_effort(&model.model_id));
    let variants = model_variants(&model.id, &display_name, &tooltip, &contexts, thinking);
    let legacy_slugs = variants
        .iter()
        .filter_map(|variant| variant.legacy_slug.clone())
        .collect();
    AvailableModel {
        name: model.id.clone(),
        default_on: true,
        supports_agent: Some(true),
        degradation_status: Some(0),
        tooltip_data: Some(tooltip.clone()),
        supports_thinking: Some(thinking),
        supports_images: Some(model.images),
        supports_max_mode: Some(true),
        client_display_name: Some(display_name.clone()),
        server_model_name: Some(model.id.clone()),
        supports_non_max_mode: Some(true),
        tooltip_data_for_max_mode: Some(tooltip.clone()),
        is_recommended_for_background_composer: Some(true),
        supports_plan_mode: Some(true),
        inputbox_short_model_name: Some(display_name),
        supports_sandboxing: Some(true),
        supports_cmd_k: Some(true),
        parameter_definitions: model_parameters(&contexts, thinking),
        variants,
        legacy_slugs,
        named_model_section_index: Some(1),
        visible_in_routed_model_view: Some(true),
        vendor_name: Some("cursor".into()),
        vendor: Some(AvailableModelVendor {
            id: 6,
            display_name: "Cursor".into(),
        }),
        model_picker_badges: vec![ModelPickerBadge {
            label: "Nexusor".into(),
            variant: 1,
            dismiss_on_selection: false,
        }],
    }
}
