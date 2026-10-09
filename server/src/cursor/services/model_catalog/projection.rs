use super::*;

pub(crate) fn auto_router_available_model(display_name: &str) -> AvailableModel {
    AvailableModel {
        name: "auto-smart".into(),
        default_on: true,
        supports_agent: Some(true),
        degradation_status: Some(0),
        tooltip_data: Some(TooltipData {
            markdown_content: Some("Nexusor Smart Router: İsteğin türüne göre en uygun modele otomatik yönlendirme ve kota aşımında yedek modele kesintisiz geçiş.".into()),
        }),
        supports_thinking: Some(true),
        supports_images: Some(true),
        supports_max_mode: Some(true),
        client_display_name: Some(display_name.into()),
        server_model_name: Some("auto-smart".into()),
        supports_non_max_mode: Some(true),
        tooltip_data_for_max_mode: None,
        is_recommended_for_background_composer: Some(true),
        supports_plan_mode: Some(true),
        inputbox_short_model_name: Some("Auto".into()),
        supports_sandboxing: Some(true),
        supports_cmd_k: Some(true),
        parameter_definitions: Vec::new(),
        variants: Vec::new(),
        legacy_slugs: vec!["auto".into(), "cursor-auto".into()],
        named_model_section_index: Some(0),
        visible_in_routed_model_view: Some(true),
        vendor_name: Some("cursor".into()),
        vendor: Some(AvailableModelVendor {
            id: 5,
            display_name: "Nexusor".into(),
        }),
        model_picker_badges: vec![ModelPickerBadge {
            label: "Nexusor".into(),
            variant: 1,
            dismiss_on_selection: false,
        }],
    }
}

pub(crate) fn combo_available_model(
    combo_id: &str,
    display_name: &str,
    description: Option<&str>,
) -> AvailableModel {
    AvailableModel {
        name: combo_id.into(),
        default_on: true,
        supports_agent: Some(true),
        degradation_status: Some(0),
        tooltip_data: Some(TooltipData {
            markdown_content: Some(description.unwrap_or("Model Fallback Zinciri").into()),
        }),
        supports_thinking: Some(true),
        supports_images: Some(true),
        supports_max_mode: Some(true),
        client_display_name: Some(display_name.into()),
        server_model_name: Some(combo_id.into()),
        supports_non_max_mode: Some(true),
        tooltip_data_for_max_mode: None,
        is_recommended_for_background_composer: Some(true),
        supports_plan_mode: Some(true),
        inputbox_short_model_name: Some(display_name.into()),
        supports_sandboxing: Some(true),
        supports_cmd_k: Some(true),
        parameter_definitions: Vec::new(),
        variants: Vec::new(),
        legacy_slugs: Vec::new(),
        named_model_section_index: Some(0),
        visible_in_routed_model_view: Some(true),
        vendor_name: Some("cursor".into()),
        vendor: Some(AvailableModelVendor {
            id: 5,
            display_name: "Nexusor Combo".into(),
        }),
        model_picker_badges: vec![ModelPickerBadge {
            label: "Nexusor".into(),
            variant: 1,
            dismiss_on_selection: false,
        }],
    }
}

pub(crate) fn pretty_model_name(name: &str) -> String {
    let clean = name
        .replace(" (Copilot)", "")
        .replace(" (Claude)", "")
        .trim()
        .to_string();
    if clean.starts_with("gemini-") {
        let rest = clean.trim_start_matches("gemini-");
        format!("Gemini {}", rest.replace('-', " "))
    } else if clean.starts_with("claude-") {
        let rest = clean.trim_start_matches("claude-");
        format!("Claude {}", rest.replace('-', " "))
    } else if clean == "gpt-4o" {
        "GPT-4o".into()
    } else if clean == "gpt-4o-mini" {
        "GPT-4o mini".into()
    } else if clean == "kimi-for-coding" {
        "Kimi for Coding".into()
    } else if clean == "kimi-k3" {
        "Kimi K3".into()
    } else if clean == "kimi-k2.5" {
        "Kimi K2.5".into()
    } else {
        clean
    }
}

pub(crate) fn available_model(model: &ModelConfig) -> AvailableModel {
    let contexts = context_options(model.context_window_tokens);
    let tooltip = model_tooltip(model);
    let display_name = pretty_model_name(&model.display_name);
    let variants = model_variants(&model.model_hash, &display_name, &tooltip, &contexts, true);
    let legacy_slugs = variants
        .iter()
        .filter_map(|variant| variant.legacy_slug.clone())
        .collect();
    AvailableModel {
        name: model.model_hash.clone(),
        default_on: true,
        supports_agent: Some(true),
        degradation_status: Some(0),
        tooltip_data: Some(tooltip.clone()),
        supports_thinking: Some(true),
        supports_images: Some(true),
        supports_max_mode: Some(true),
        client_display_name: Some(display_name.clone()),
        server_model_name: Some(model.model_hash.clone()),
        supports_non_max_mode: Some(true),
        tooltip_data_for_max_mode: Some(tooltip),
        is_recommended_for_background_composer: Some(true),
        supports_plan_mode: Some(true),
        inputbox_short_model_name: Some(display_name),
        supports_sandboxing: Some(true),
        supports_cmd_k: Some(true),
        parameter_definitions: model_parameters(&contexts, true),
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

pub(crate) fn model_parameters(
    contexts: &[(String, String)],
    thinking: bool,
) -> Vec<ModelParameterDefinition> {
    let mut parameters = vec![ModelParameterDefinition {
        id: "context".into(),
        name: "Context".into(),
        markdown_tooltip: Some("Context size used to trigger conversation compaction.".into()),
        parameter_type: Some(ModelParameterType {
            boolean_parameter: None,
            enum_parameter: Some(EnumParameter {
                values: contexts
                    .iter()
                    .map(|(value, display_name)| EnumParameterValue {
                        value: value.clone(),
                        display_name: Some(display_name.clone()),
                    })
                    .collect(),
            }),
        }),
        is_cycleable_by_hotkey: Some(false),
    }];
    if thinking {
        parameters.push(ModelParameterDefinition {
            id: "reasoning".into(),
            name: "Effort".into(),
            markdown_tooltip: Some("Effort the model uses to generate its response.".into()),
            parameter_type: Some(ModelParameterType {
                boolean_parameter: None,
                enum_parameter: Some(EnumParameter {
                    values: EFFORTS
                        .into_iter()
                        .map(|(value, display_name)| EnumParameterValue {
                            value: value.into(),
                            display_name: Some(display_name.into()),
                        })
                        .collect(),
                }),
            }),
            is_cycleable_by_hotkey: Some(true),
        });
    }
    parameters.push(ModelParameterDefinition {
        id: "fast".into(),
        name: "Fast".into(),
        markdown_tooltip: Some("Significantly faster but consumes more usage".into()),
        parameter_type: Some(ModelParameterType {
            boolean_parameter: Some(BooleanParameter {
                values: vec![
                    BooleanParameterValue {
                        value: "false".into(),
                        display_name: None,
                        increases_model_cost: None,
                    },
                    BooleanParameterValue {
                        value: "true".into(),
                        display_name: Some("Fast".into()),
                        increases_model_cost: Some(true),
                    },
                ],
            }),
            enum_parameter: None,
        }),
        is_cycleable_by_hotkey: Some(false),
    });
    parameters
}

pub(crate) fn model_variants(
    name: &str,
    display_name: &str,
    tooltip: &TooltipData,
    contexts: &[(String, String)],
    thinking: bool,
) -> Vec<ModelVariant> {
    // 非思考模型没有 Effort 轴,变体网格只剩 Context × Fast。
    let efforts: &[Option<(&str, &str)>] = if thinking {
        &[
            Some(EFFORTS[0]),
            Some(EFFORTS[1]),
            Some(EFFORTS[2]),
            Some(EFFORTS[3]),
            Some(EFFORTS[4]),
        ]
    } else {
        &[None]
    };
    let mut variants = Vec::with_capacity(contexts.len() * efforts.len() * 2);
    for (context, context_name) in contexts {
        for effort in efforts {
            for fast in [false, true] {
                variants.push(model_variant(
                    name,
                    display_name,
                    tooltip,
                    context,
                    context_name,
                    *effort,
                    fast,
                ));
            }
        }
    }
    variants
}

pub(crate) fn model_variant(
    name: &str,
    display_name: &str,
    tooltip: &TooltipData,
    context: &str,
    context_name: &str,
    effort: Option<(&str, &str)>,
    fast: bool,
) -> ModelVariant {
    let mut suffix = Vec::with_capacity(3);
    if context != DEFAULT_CONTEXT {
        suffix.push(context_name);
    }
    if let Some((_, effort_name)) = effort {
        suffix.push(effort_name);
    }
    if fast {
        suffix.push("Fast");
    }
    let suffix = suffix.join(" ");
    let display_name = if suffix.is_empty() {
        display_name.to_owned()
    } else {
        format!(
            "{display_name} <span style=\"color: var(--cursor-text-tertiary);\">{suffix}</span>"
        )
    };
    let is_default =
        context == DEFAULT_CONTEXT && !fast && effort.is_none_or(|(effort, _)| effort == "high");
    let mut parameter_values = vec![ModelParameterValue {
        id: "context".into(),
        value: context.into(),
    }];
    if let Some((effort, _)) = effort {
        parameter_values.push(ModelParameterValue {
            id: "reasoning".into(),
            value: effort.into(),
        });
    }
    parameter_values.push(ModelParameterValue {
        id: "fast".into(),
        value: fast.to_string(),
    });
    ModelVariant {
        parameter_values,
        display_name: display_name.clone(),
        is_max_mode: false,
        is_default_max_config: is_default.then_some(true),
        is_default_non_max_config: is_default.then_some(true),
        tooltip_data: Some(tooltip.clone()),
        display_name_outside_picker: Some(display_name),
        variant_string_representation: Some(match effort {
            Some((effort, _)) => {
                format!("{name}[context={context},reasoning={effort},fast={fast}]")
            }
            None => format!("{name}[context={context},fast={fast}]"),
        }),
        legacy_slug: Some(format!(
            "{name}-{context}{}{}",
            effort
                .map(|(effort, _)| format!("-{effort}"))
                .unwrap_or_default(),
            if fast { "-fast" } else { "" }
        )),
    }
}

pub(crate) fn model_tooltip(model: &ModelConfig) -> TooltipData {
    TooltipData {
        markdown_content: Some(model.tooltip_data.clone()),
    }
}
