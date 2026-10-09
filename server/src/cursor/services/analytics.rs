//! Implements Cursor analytics endpoints and event handling.
use axum::{
    body::{to_bytes, Body},
    extract::{Extension, State},
    http::{header, HeaderValue, Request, Response, StatusCode},
};
use base64::{engine::general_purpose::STANDARD, Engine};
use prost::Message;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use crate::{api::cursor::proxy, local_app, Error, Result};

pub const BOOTSTRAP_STATSIG_PATH: &str = "/aiserver.v1.AnalyticsService/BootstrapStatsig";
const FEATURE_GATES: &[&str] = &[
    "nal_agent_retries",
    "explicit_subagent_models",
    "subagent_support_interrupt",
    "opt_devs_into_experimental_model_toggle",
    "meta_mcp_tool",
    "mcp_input_schema_json",
    "glass_custom_modes",
    "cursor_plan_mode",
    "cursor_auto_mode",
    "composer_background_agent",
    "composer_auto_context",
    "composer_diff_review",
    "composer_loop_on_lints",
    "composer_mega_planner",
    "agent_parallel_subagents",
    "subagent_multi_turn",
    "mcp_tools_enabled",
    "enable_fast_mode",
    "enable_thinking_models",
    "enable_model_picker_max_mode",
];
const LOCAL_RULE: &str = "local_enabled";

#[derive(Clone, PartialEq, Message)]
struct BootstrapStatsigResponse {
    #[prost(string, tag = "1")]
    config: String,
    #[prost(uint64, tag = "2")]
    generated_at_ms: u64,
}

pub async fn bootstrap_statsig(
    State(registry): State<crate::cursor::transport::TransportRegistry>,
    Extension(upstream): Extension<proxy::CursorProxy>,
    request: Request<Body>,
) -> Result<Response<Body>> {
    if local_app::request_uses_local_cursor_token(request.headers()) {
        to_bytes(
            request.into_body(),
            crate::api::cursor::CURSOR_MAX_BODY_BYTES,
        )
        .await
        .map_err(|error| Error::Protocol(format!("cannot read Statsig body: {error}")))?;
        return local_response();
    }
    if !registry.store().cursor_takeover_enabled().await? {
        return proxy::forward(Extension(upstream), request).await;
    }
    let response = proxy::forward_buffered(&upstream, request).await?;
    if response.status != StatusCode::OK {
        return Ok(response.into_response());
    }
    match http_transport_config(&response.body) {
        Ok(body) => Ok(response.with_body(body.into())),
        Err(error) => {
            tracing::warn!(%error, "preserving unrecognized Statsig response");
            Ok(response.into_response())
        }
    }
}

fn http_transport_config(body: &[u8]) -> Result<Vec<u8>> {
    let message = BootstrapStatsigResponse::decode(body)?;
    let mut config: Value = serde_json::from_str(&message.config)?;
    let key = format_statsig_key(
        config.get("hash_used").and_then(Value::as_str),
        "nal_websocket_client",
    );
    let gates = config
        .get_mut("feature_gates")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| Error::Protocol("Statsig feature_gates must be an object".into()))?;
    let gate = gates
        .entry(key.clone())
        .or_insert_with(|| enabled_gate(&key));
    let gate = gate
        .as_object_mut()
        .ok_or_else(|| Error::Protocol("Statsig gate must be an object".into()))?;
    gate.insert("value".into(), Value::Bool(false));
    // Protobuf singular fields use the last value. Keep the original wire bytes,
    // including fields unknown to this build, and override only the JSON config.
    let mut output = body.to_vec();
    prost::encoding::string::encode(1, &serde_json::to_string(&config)?, &mut output);
    Ok(output)
}

fn local_response() -> Result<Response<Body>> {
    let generated_at_ms = chrono::Utc::now().timestamp_millis() as u64;
    let mut config = json!({
        "feature_gates": {},
        "dynamic_configs": {},
        "layer_configs": {},
        "user": {
            "userID": "local_ultra",
            "customIDs": { "localUserID": "local_ultra" }
        },
        "has_updates": true,
        "hash_used": "none",
        "sdkParams": {
            "stableID": "local_ultra",
            "disableDiagnosticsLogging": true
        },
        "time": generated_at_ms
    });
    enable_feature_gates(&mut config)?;
    let message = BootstrapStatsigResponse {
        config: serde_json::to_string(&config)?,
        generated_at_ms,
    };
    let body = message.encode_to_vec();
    let mut response = Response::new(Body::from(body.clone()));
    *response.status_mut() = StatusCode::OK;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/proto"),
    );
    response.headers_mut().insert(
        header::CONTENT_LENGTH,
        body.len()
            .to_string()
            .parse()
            .expect("body length is a valid header value"),
    );
    Ok(response)
}

fn enable_feature_gates(config: &mut Value) -> Result<()> {
    let hash_used = config
        .get("hash_used")
        .and_then(Value::as_str)
        .map(str::to_owned);

    let root = config
        .as_object_mut()
        .ok_or_else(|| Error::Protocol("Statsig bootstrap config must be an object".into()))?;
    let gates = root
        .entry("feature_gates")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| Error::Protocol("Statsig feature_gates must be an object".into()))?;

    for &gate_name in FEATURE_GATES {
        let gate_key = format_statsig_key(hash_used.as_deref(), gate_name);
        gates.insert(gate_key.clone(), enabled_gate(&gate_key));
    }
    Ok(())
}

fn format_statsig_key(hash_used: Option<&str>, name: &str) -> String {
    match hash_used {
        Some("djb2") => djb2(name),
        Some("sha256") => STANDARD.encode(Sha256::digest(name.as_bytes())),
        _ => name.to_owned(),
    }
}

fn djb2(value: &str) -> String {
    value
        .encode_utf16()
        .fold(0_u32, |hash, character| {
            hash.wrapping_mul(31).wrapping_add(u32::from(character))
        })
        .to_string()
}

fn enabled_gate(name: &str) -> Value {
    json!({
        "name": name,
        "value": true,
        "rule_id": LOCAL_RULE,
        "ruleID": LOCAL_RULE,
        "group_name": LOCAL_RULE,
        "groupName": LOCAL_RULE,
        "secondary_exposures": [],
        "secondaryExposures": [],
        "undelegated_secondary_exposures": [],
        "undelegatedSecondaryExposures": [],
        "is_device_based": false,
        "isDeviceBased": false,
        "id_type": "userID",
        "idType": "userID"
    })
}
