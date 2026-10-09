use super::*;

pub async fn usable_models(
    State(registry): State<TransportRegistry>,
    Extension(proxy): Extension<CursorProxy>,
    request: Request<Body>,
) -> Result<Response<Body>> {
    let models = registry.store().models().await?;
    let plugin_models = match registry.plugins() {
        Some(plugins) => plugins.configured_models().await,
        None => Vec::new(),
    };
    let customizations = registry
        .store()
        .model_customizations()
        .await
        .unwrap_or_default();
    let custom_map: std::collections::HashMap<_, _> = customizations
        .into_iter()
        .map(|c| (c.model_id, (c.custom_name, c.enabled)))
        .collect();

    let filtered_models: Vec<_> = models
        .into_iter()
        .filter_map(|mut model| {
            if let Some((custom_name, enabled)) = custom_map.get(&model.model_hash) {
                if !enabled {
                    return None;
                }
                if let Some(name) = custom_name {
                    model.display_name = name.clone();
                }
            }
            Some(model)
        })
        .collect();

    let filtered_plugin_models: Vec<_> = plugin_models
        .into_iter()
        .filter_map(|mut model| {
            if let Some((custom_name, enabled)) = custom_map.get(&model.id) {
                if !enabled {
                    return None;
                }
                if let Some(name) = custom_name {
                    model.display_name = name.clone();
                }
            }
            Some(model)
        })
        .collect();

    let auto_enabled = custom_map
        .get("auto-smart")
        .map(|(_, e)| *e)
        .unwrap_or(true);
    let auto_display = custom_map
        .get("auto-smart")
        .and_then(|(n, _)| n.clone())
        .unwrap_or_else(|| "Auto (Akıllı Yönlendirici)".into());

    let combos = registry.store().router_combos().await.unwrap_or_default();
    let active_combos: Vec<_> = combos
        .into_iter()
        .filter(|c| c.enabled && !c.models.is_empty())
        .collect();

    let mut usable_list: Vec<agent::ModelDetails> = Vec::new();
    if auto_enabled {
        usable_list.push(agent::ModelDetails {
            model_id: "auto-smart".into(),
            display_model_id: "auto-smart".into(),
            display_name: auto_display,
            display_name_short: "Auto".into(),
            thinking_details: Some(agent::ThinkingDetails::default()),
            credentials: Some(cli_local_model_credentials()),
            ..Default::default()
        });
    }

    for combo in &active_combos {
        let combo_model_id = format!("combo:{}", combo.combo_id);
        let display = combo.name.clone();
        usable_list.push(agent::ModelDetails {
            model_id: combo_model_id.clone(),
            display_model_id: combo_model_id,
            display_name: display.clone(),
            display_name_short: display,
            thinking_details: Some(agent::ThinkingDetails::default()),
            credentials: Some(cli_local_model_credentials()),
            ..Default::default()
        });
    }

    usable_list.extend(filtered_models.iter().map(usable_model));
    usable_list.extend(filtered_plugin_models.iter().map(usable_plugin_model));

    let hide_builtin = registry
        .store()
        .desktop_settings()
        .await
        .map(|s| s.hide_cursor_builtin_models)
        .unwrap_or(true);

    let local = UsableModelsAddition {
        models: usable_list,
    }
    .encode_to_vec();

    if hide_builtin {
        tracing::debug!(
            "built-in Cursor models hidden: served local Nexusor usable models immediately"
        );
        return Ok(local_response(local));
    }

    match proxy::forward_buffered(&proxy, request).await {
        Ok(upstream) => merge_response(upstream, local, hide_builtin),
        Err(error) => {
            tracing::warn!(%error, "Cursor GetUsableModels upstream unavailable; using local catalog");
            Ok(local_response(local))
        }
    }
}
