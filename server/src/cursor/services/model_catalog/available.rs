use super::*;
pub async fn available_models(
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

    // Filter and customize built-in models
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

    // Filter and customize plugin models
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

    tracing::info!(
        model_count = filtered_models.len(),
        plugin_model_count = filtered_plugin_models.len(),
        combo_count = active_combos.len(),
        "appending Nexusor models to Cursor AvailableModels"
    );
    let mut available_models = filtered_models
        .iter()
        .map(available_model)
        .collect::<Vec<_>>();
    available_models.extend(filtered_plugin_models.iter().map(available_plugin_model));

    let mut model_names: Vec<String> = filtered_models
        .iter()
        .map(|model| model.model_hash.clone())
        .chain(filtered_plugin_models.iter().map(|model| model.id.clone()))
        .collect();

    // Insert Combos as distinct select-able models in Cursor picker
    for combo in &active_combos {
        let combo_model_id = format!("combo:{}", combo.combo_id);
        let display = combo.name.clone();
        available_models.push(combo_available_model(
            &combo_model_id,
            &display,
            combo.description.as_deref(),
        ));
        model_names.push(combo_model_id);
    }

    if auto_enabled {
        available_models.insert(0, auto_router_available_model(&auto_display));
        model_names.insert(0, "auto-smart".into());
    }

    let hide_builtin = registry
        .store()
        .desktop_settings()
        .await
        .map(|s| s.hide_cursor_builtin_models)
        .unwrap_or(true);

    let local = AvailableModelsAddition {
        model_names,
        models: available_models,
    }
    .encode_to_vec();

    if hide_builtin {
        tracing::debug!(
            "built-in Cursor models hidden: served local Nexusor model catalog immediately"
        );
        return Ok(local_response(local));
    }

    match proxy::forward_buffered(&proxy, request).await {
        Ok(upstream) => merge_response(upstream, local, hide_builtin),
        Err(error) => {
            tracing::warn!(%error, "Cursor AvailableModels upstream unavailable; using local catalog");
            Ok(local_response(local))
        }
    }
}
