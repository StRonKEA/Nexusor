//! Endpoints for model customizations and smart router combos.

use axum::{
    extract::{Path, State},
    Json,
};
use serde::Deserialize;

use crate::{
    control::ControlService,
    store::{AutoRouterConfig, ModelCustomization, RouterCombo},
    Result,
};

#[derive(Deserialize)]
pub struct SetModelCustomizationInput {
    pub custom_name: Option<String>,
    pub enabled: bool,
}

pub async fn list_customizations(
    State(service): State<ControlService>,
) -> Result<Json<Vec<ModelCustomization>>> {
    Ok(Json(service.store().model_customizations().await?))
}

pub async fn set_customization(
    State(service): State<ControlService>,
    Path(model_id): Path<String>,
    Json(input): Json<SetModelCustomizationInput>,
) -> Result<Json<ModelCustomization>> {
    let result = service
        .store()
        .set_model_customization(&model_id, input.custom_name.as_deref(), input.enabled)
        .await?;
    Ok(Json(result))
}

pub async fn list_combos(State(service): State<ControlService>) -> Result<Json<Vec<RouterCombo>>> {
    Ok(Json(service.store().router_combos().await?))
}

pub async fn upsert_combo(
    State(service): State<ControlService>,
    Json(combo): Json<RouterCombo>,
) -> Result<Json<RouterCombo>> {
    let result = service.store().upsert_router_combo(combo).await?;
    Ok(Json(result))
}

pub async fn delete_combo(
    State(service): State<ControlService>,
    Path(combo_id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    service.store().delete_router_combo(&combo_id).await?;
    Ok(Json(serde_json::json!({ "success": true })))
}

pub async fn get_auto_router(
    State(service): State<ControlService>,
) -> Result<Json<AutoRouterConfig>> {
    Ok(Json(service.store().auto_router_config().await?))
}

pub async fn set_auto_router(
    State(service): State<ControlService>,
    Json(config): Json<AutoRouterConfig>,
) -> Result<Json<AutoRouterConfig>> {
    let result = service.store().set_auto_router_config(config).await?;
    Ok(Json(result))
}
