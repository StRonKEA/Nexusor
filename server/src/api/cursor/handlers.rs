//! Implements Cursor HTTP endpoints outside the Agent Run stream.
use std::time::Duration;

use axum::{
    body::{to_bytes, Body, Bytes},
    extract::{DefaultBodyLimit, Extension, State},
    http::{header, HeaderMap, HeaderValue, Request, Response, StatusCode},
    routing::{get, post},
    Router,
};
use tower_http::decompression::RequestDecompressionLayer;

use crate::{
    api::cursor::{
        bidi,
        proxy::{self, CursorProxy},
        run_sse,
    },
    cursor::{
        protocol::{
            connect,
            proto::{agent::v1 as agent, aiserver::v1 as ai},
        },
        services::{
            account, analytics, commit_message, compatibility, knowledge, model_catalog,
            server_config, tab,
        },
        transport::{TransportParent, TransportRegistry, TransportRoute},
    },
    Result,
};

const INITIAL_APPEND_WAIT: Duration = Duration::from_secs(30);
pub const CURSOR_MAX_BODY_BYTES: usize = 128 * 1024 * 1024;

pub fn router(
    registry: TransportRegistry,
    clients: crate::network::NetworkClients,
) -> Result<Router> {
    let proxy = CursorProxy::cursor(clients);
    let knowledge = knowledge::KnowledgeService::managed()?;
    Ok(router_with_proxy(registry, proxy, knowledge))
}

fn router_with_proxy(
    registry: TransportRegistry,
    proxy: CursorProxy,
    knowledge_service: knowledge::KnowledgeService,
) -> Router {
    let web_cache = registry.web_cache().router();
    Router::new()
        .route(
            "/favicon.ico",
            get(|| async { axum::http::StatusCode::NO_CONTENT }),
        )
        .route("/__byok-api__/healthz", get(health))
        .route(
            "/__byok-api__/api/local-runs",
            get(super::local_controls::list),
        )
        .route(
            "/__byok-api__/api/local-runs/{request_id}/control",
            post(super::local_controls::control),
        )
        .route("/agent.v1.AgentService/RunSSE", post(run_sse_handler))
        .route(
            "/aiserver.v1.AiService/RunGenerateImage",
            post(crate::cursor::services::image::generate),
        )
        .route("/aiserver.v1.BidiService/BidiAppend", post(bidi_handler))
        .route(
            "/aiserver.v1.CmdKService/StreamCmdK",
            post(crate::cursor::services::cmdk::edit),
        )
        .route(
            "/aiserver.v1.CmdKService/StreamTerminalCmdK",
            post(crate::cursor::services::cmdk::terminal),
        )
        .route(
            "/aiserver.v1.AiService/AvailableDocs",
            post(compatibility::available_docs),
        )
        .route(
            "/aiserver.v1.DashboardService/GetEffectiveUserPlugins",
            post(compatibility::effective_user_plugins),
        )
        .route(
            "/aiserver.v1.DashboardService/GetUserPrivacyMode",
            post(compatibility::user_privacy_mode),
        )
        .route(
            "/agent.v1.AgentService/UpdateConversationMetadata",
            post(compatibility::update_conversation_metadata),
        )
        .route(
            "/aiserver.v1.AiService/GetServerConfig",
            post(server_config::get),
        )
        .route(
            "/aiserver.v1.ServerConfigService/GetServerConfig",
            post(server_config::get),
        )
        .route(
            "/aiserver.v1.AiService/AvailableModels",
            post(model_catalog::available_models),
        )
        .route(
            "/agent.v1.AgentService/GetUsableModels",
            post(model_catalog::usable_models),
        )
        .route(
            "/aiserver.v1.AiService/GetUsableModels",
            post(model_catalog::usable_models),
        )
        .route(
            "/aiserver.v1.AiService/WriteGitCommitMessage",
            post(commit_message::write_git_commit_message),
        )
        .route(
            "/aiserver.v1.NetworkService/IsConnected",
            post(is_connected),
        )
        .route(
            "/agent.v1.AgentService/GetDefaultModelForCli",
            post(model_catalog::default_model_for_cli),
        )
        .route(
            "/aiserver.v1.AiService/GetDefaultModelForCli",
            post(model_catalog::default_model_for_cli),
        )
        .route(
            "/aiserver.v1.AiService/GetDefaultModel",
            post(model_catalog::default_model),
        )
        .route(
            "/aiserver.v1.AiService/GetDefaultModelNudgeData",
            post(model_catalog::default_model_nudge),
        )
        .route(
            "/aiserver.v1.AuthService/GetEmail",
            post(account::get_email),
        )
        .route(
            "/aiserver.v1.AuthService/GetUserMeta",
            post(account::get_user_meta),
        )
        .route("/aiserver.v1.DashboardService/GetMe", post(account::get_me))
        .route(
            "/aiserver.v1.DashboardService/GetTeams",
            post(account::get_teams),
        )
        .route(
            "/aiserver.v1.DashboardService/GetUserProfile",
            post(account::get_user_profile),
        )
        .route(
            "/aiserver.v1.DashboardService/GetCurrentPeriodUsage",
            post(account::current_period_usage),
        )
        .route(
            "/aiserver.v1.DashboardService/GetUsageLimitStatusAndActiveGrants",
            post(account::usage_limit_status),
        )
        .route(
            "/aiserver.v1.DashboardService/GetPlanInfo",
            post(account::get_plan_info),
        )
        .route(
            "/aiserver.v1.DashboardService/IsOnNewPricing",
            post(account::is_on_new_pricing),
        )
        .route(
            "/aiserver.v1.DashboardService/GetManagedSkills",
            post(account::get_managed_skills),
        )
        .route(
            "/aiserver.v1.DashboardService/GetAvailableMcpServers",
            post(account::get_available_mcp_servers),
        )
        .route(
            "/aiserver.v1.MCPRegistryService/GetKnownServers",
            post(account::get_known_servers),
        )
        .route(
            "/auth/has_valid_payment_method",
            get(account::has_valid_payment_method),
        )
        .route(
            "/aiserver.v1.DashboardService/GetCliDownloadUrl",
            post(account::get_cli_download_url),
        )
        .route(
            "/aiserver.v1.DashboardService/CheckHttpMcpStatus",
            post(account::check_http_mcp_status),
        )
        .route("/aiserver.v1.AiService/NameTab", post(account::name_tab))
        .route(
            "/aiserver.v1.AiService/ServerTime",
            post(account::server_time),
        )
        .route(
            "/aiserver.v1.DashboardService/ListMarketplaces",
            post(account::list_marketplaces),
        )
        .route(
            "/aiserver.v1.DashboardService/ListMarketplacePlugins",
            post(account::list_marketplace_plugins),
        )
        .route(
            "/aiserver.v1.AiService/WriteGitBranchName",
            post(account::write_git_branch_name),
        )
        .route(
            "/aiserver.v1.AiService/AutoContext",
            post(account::auto_context),
        )
        .route(
            "/aiserver.v1.AiService/ContextReranking",
            post(account::context_reranking),
        )
        .route(
            "/aiserver.v1.AiService/CheckFeaturesStatus",
            post(account::check_features_status),
        )
        .route(
            "/aiserver.v1.AiService/CheckFeatureStatus",
            post(account::check_feature_status),
        )
        .route(
            "/aiserver.v1.AiService/CheckFeatureStatusUnauthenticated",
            post(account::check_feature_status),
        )
        .route(
            "/aiserver.v1.AiService/WarmComposerCache",
            post(account::warm_composer_cache),
        )
        .route(
            "/aiserver.v1.AiService/KeepComposerCacheWarm",
            post(account::keep_composer_cache_warm),
        )
        .route(
            "/aiserver.v1.AiService/CountTokens",
            post(account::count_tokens),
        )
        .route(
            "/aiserver.v1.AiService/StreamTerminalAutocomplete",
            post(crate::cursor::services::terminal::autocomplete),
        )
        .route(
            "/aiserver.v1.FullSelfDrivingService/GetFullSelfDrivingConfig",
            post(account::get_full_self_driving_config),
        )
        .route(
            "/aiserver.v1.AiService/ShouldTurnOnCppOnboarding",
            post(account::should_turn_on_cpp_onboarding),
        )
        .route(
            "/aiserver.v1.GitGraphService/IsGitGraphEnabled",
            post(account::is_git_graph_enabled),
        )
        .route(
            "/aiserver.v1.GitGraphService/GetGitGraphStatus",
            post(account::get_git_graph_status),
        )
        .route(
            "/aiserver.v1.GitGraphService/GetGitGraphRelatedFiles",
            post(account::get_git_graph_related_files),
        )
        .route(
            "/agent.v1.AgentService/NameAgent",
            post(account::name_agent),
        )
        .route(
            "/agent.v1.AgentService/GetAllowedModelIntents",
            post(account::get_allowed_model_intents),
        )
        .route(
            "/aiserver.v1.LinterService/LintFile",
            post(account::lint_file),
        )
        .route(
            "/aiserver.v1.LinterService/LintChunk",
            post(account::lint_chunk),
        )
        .route(
            "/aiserver.v1.CursorPredictionService/CursorPredictionConfig",
            post(account::cursor_prediction_config),
        )
        .route(
            "/aiserver.v1.FastApplyService/WarmApply",
            post(account::warm_apply),
        )
        .route(
            "/aiserver.v1.FastApplyService/ReportEditFate",
            post(account::report_edit_fate),
        )
        .route(
            "/aiserver.v1.LinterService/LintExplanation2",
            post(account::lint_explanation2),
        )
        .route(
            "/aiserver.v1.ReviewService/BugConfig",
            post(account::bug_config),
        )
        .route(
            "/aiserver.v1.ReviewService/StreamReview",
            post(account::stream_review),
        )
        .route(
            "/aiserver.v1.ReviewService/StreamSlowReview",
            post(account::stream_review),
        )
        .route(
            "/aiserver.v1.AiService/StreamBugBotAgentic",
            post(account::stream_review),
        )
        .route(
            "/aiserver.v1.CmdKService/RerankCmdKContext",
            post(account::rerank_cmdk_context),
        )
        .route(
            "/aiserver.v1.CmdKService/RerankTerminalCmdKContext",
            post(account::rerank_cmdk_context),
        )
        .route(
            "/agent.v1.AgentService/CreateTranscriptOverview",
            post(account::create_transcript_overview),
        )
        .route(
            "/aiserver.v1.AiService/KnowledgeBaseAdd",
            post(knowledge::add),
        )
        .route(
            "/aiserver.v1.AiService/KnowledgeBaseList",
            post(knowledge::list),
        )
        .route(
            "/aiserver.v1.AiService/KnowledgeBaseUpdate",
            post(knowledge::update),
        )
        .route(
            "/aiserver.v1.AiService/KnowledgeBaseRemove",
            post(knowledge::remove),
        )
        .route(
            analytics::BOOTSTRAP_STATSIG_PATH,
            post(analytics::bootstrap_statsig),
        )
        .route("/auth/full_stripe_profile", get(account::stripe_profile))
        .route("/auth/stripe_profile", get(account::stripe_profile))
        .merge(tab::router())
        .route_layer(DefaultBodyLimit::max(CURSOR_MAX_BODY_BYTES))
        .route_layer(RequestDecompressionLayer::new())
        .fallback(proxy::forward)
        .method_not_allowed_fallback(proxy::forward)
        .layer(Extension(proxy))
        .layer(Extension(knowledge_service))
        .with_state(registry)
        .merge(web_cache)
}

async fn health() -> StatusCode {
    StatusCode::NO_CONTENT
}

/// `NetworkService/IsConnected` probe. Cursor's always-local extension checks
/// connectivity roughly 10s after any slow request starts; a 404/error here is
/// treated as "network disconnected" and aborts in-flight work (e.g. commit
/// message generation) even while the model is still streaming. Always answer
/// connected with an empty `IsConnectedResponse` so local Nexusor generation is
/// never cancelled by this probe.
async fn is_connected() -> Result<Response<Body>> {
    let payload = connect::encode_message(&ai::IsConnectedResponse {})?;
    let mut response = Response::new(Body::from(payload));
    *response.status_mut() = StatusCode::OK;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/proto"),
    );
    Ok(response)
}

async fn run_sse_handler(
    State(registry): State<TransportRegistry>,
    Extension(proxy): Extension<CursorProxy>,
    request: Request<Body>,
) -> Result<Response<Body>> {
    let (parts, body) = buffered(request).await?;
    let request: agent::BidiRequestId = connect::decode_unary(&body)?;
    let route = registry.wait_route(&request.request_id).await;
    let trace = registry.trace(&request.request_id);
    trace.resume();
    trace.request(
        "run_sse_request",
        body.clone(),
        serde_json::json!({"request_id": request.request_id}),
    );
    match route {
        crate::cursor::transport::TransportRoute::Local => {
            run_sse::stream(&registry, &request.request_id).await
        }
        crate::cursor::transport::TransportRoute::Upstream(generation) => {
            let response = proxy::forward(
                Extension(proxy),
                Request::from_parts(parts, Body::from(body)),
            )
            .await?;
            Ok(run_sse::upstream(
                registry,
                request.request_id,
                generation,
                response,
                Some(trace),
            )
            .await)
        }
    }
}

async fn bidi_handler(
    State(registry): State<TransportRegistry>,
    Extension(proxy): Extension<CursorProxy>,
    request: Request<Body>,
) -> Result<Response<Body>> {
    let (parts, body) = buffered(request).await?;
    let request: ai::BidiAppendRequest = connect::decode_unary(&body)?;
    let decoded = bidi::decode(&request)?;
    let first_model = decoded.model_id().map(str::to_owned);
    let conversation_id = decoded.conversation_id().map(str::to_owned);
    let trace_metadata = decoded.trace_metadata();
    let trace = registry.trace(&decoded.request_id);
    let local = if let Some(model_id) = decoded.model_id() {
        if is_byok_model(&registry, model_id).await? {
            tracing::info!(
                request_id = decoded.request_id,
                model_id,
                "routing Cursor Run to local Nexusor provider"
            );
            true
        } else {
            tracing::info!(
                request_id = decoded.request_id,
                model_id,
                "routing Cursor Run to Cursor upstream"
            );
            false
        }
    } else if registry.local(&decoded.request_id).await.is_some() {
        true
    } else if registry.upstream(&decoded.request_id).await {
        false
    } else if decoded.seqno > 0 {
        // BidiAppend uploads are concurrent. A small heartbeat or interaction can arrive before
        // the much larger seqno=0 RunRequest has finished uploading/decoding.
        // Wait for its model-selected route; never guess local vs upstream.
        let route = tokio::time::timeout(
            INITIAL_APPEND_WAIT,
            registry.wait_route(&decoded.request_id),
        )
        .await
        .map_err(|_| {
            crate::Error::Protocol(
                "timed out waiting for the initial BidiAppend model selection".into(),
            )
        })?;
        matches!(route, TransportRoute::Local)
    } else {
        trace.resume();
        trace.request(
            "bidi_request",
            body.clone(),
            trace_outcome(trace_metadata, false, "missing_transport", None),
        );
        return Err(crate::Error::Protocol(
            "first BidiAppend message must select a model".into(),
        ));
    };
    if first_model.is_some() {
        trace.begin(
            conversation_id.as_deref(),
            if local {
                "local_byok"
            } else {
                "cursor_official"
            },
            first_model.as_deref(),
        );
    } else {
        trace.resume();
    }
    if !local {
        if first_model.is_some() {
            registry.mark_upstream(&decoded.request_id).await;
        }
        trace.request(
            "bidi_request",
            body.clone(),
            trace_outcome(trace_metadata, true, "upstream", None),
        );
        return proxy::forward(
            Extension(proxy),
            Request::from_parts(parts, Body::from(body)),
        )
        .await;
    }
    let parent = match parent_headers(&parts.headers) {
        Ok(parent) => parent,
        Err(error) => {
            trace.request(
                "bidi_request",
                body,
                trace_outcome(
                    trace_metadata,
                    false,
                    "invalid_parent",
                    Some(error.to_string()),
                ),
            );
            return Err(error);
        }
    };
    match bidi::append(&registry, decoded, parent).await {
        Ok(_) => trace.request(
            "bidi_request",
            body,
            trace_outcome(trace_metadata, true, "local", None),
        ),
        Err(error) => {
            trace.request(
                "bidi_request",
                body,
                trace_outcome(
                    trace_metadata,
                    false,
                    "command_rejected",
                    Some(error.to_string()),
                ),
            );
            return Err(error);
        }
    }
    let mut response = Response::new(axum::body::Body::empty());
    *response.status_mut() = StatusCode::OK;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/proto"),
    );
    Ok(response)
}

fn trace_outcome(
    mut metadata: serde_json::Value,
    accepted: bool,
    route_outcome: &str,
    error: Option<String>,
) -> serde_json::Value {
    if let Some(metadata) = metadata.as_object_mut() {
        metadata.insert("accepted".into(), accepted.into());
        metadata.insert("route_outcome".into(), route_outcome.into());
        if let Some(error) = error {
            metadata.insert("error".into(), error.into());
        }
    }
    metadata
}

async fn buffered(request: Request<Body>) -> Result<(axum::http::request::Parts, Bytes)> {
    let (parts, body) = request.into_parts();
    let body = to_bytes(body, CURSOR_MAX_BODY_BYTES)
        .await
        .map_err(|error| crate::Error::Protocol(format!("cannot read request body: {error}")))?;
    Ok((parts, body))
}

fn parent_headers(headers: &HeaderMap) -> Result<Option<TransportParent>> {
    let request_id = header_text(headers, "x-parent-request-id")?;
    let tool_call_id = header_text(headers, "x-parent-agent-tool-call-id")?;
    match (request_id, tool_call_id) {
        (None, None) => Ok(None),
        (request_id, tool_call_id) => Ok(Some(TransportParent {
            // Cursor 3.22 emits these optional ancestry fields independently.
            request_id: request_id.unwrap_or_default().into(),
            tool_call_id: tool_call_id.unwrap_or_default().into(),
        })),
    }
}

pub(crate) async fn is_byok_model(registry: &TransportRegistry, model_id: &str) -> Result<bool> {
    if model_id == "auto-smart"
        || model_id == crate::store::AutoRouterConfig::SUBAGENT_ROUTE
        || model_id == "auto"
        || model_id == "cursor-auto"
        || model_id.starts_with("combo:")
        || model_id.starts_with(crate::plugin::ADAPTER_ID_PREFIX)
    {
        return Ok(true);
    }
    Ok(
        crate::provider::resolve_model_target(registry.store(), registry.plugins(), model_id)
            .await?
            .is_some(),
    )
}

fn header_text<'a>(headers: &'a HeaderMap, name: &str) -> Result<Option<&'a str>> {
    headers
        .get(name)
        .map(|value| value.to_str())
        .transpose()
        .map_err(|error| crate::Error::Protocol(format!("invalid {name} header: {error}")))
}

#[cfg(test)]
#[path = "handlers_tests.rs"]
mod compatibility_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn optional_subagent_ancestry_headers_are_independent() {
        let mut headers = HeaderMap::new();
        assert!(parent_headers(&headers).unwrap().is_none());
        headers.insert("x-parent-request-id", HeaderValue::from_static("parent"));
        let parent = parent_headers(&headers).unwrap().unwrap();
        assert_eq!(parent.request_id, "parent");
        assert!(parent.tool_call_id.is_empty());
        headers.insert(
            "x-parent-agent-tool-call-id",
            HeaderValue::from_static("task"),
        );
        assert_eq!(
            parent_headers(&headers).unwrap().unwrap().tool_call_id,
            "task"
        );
        headers.remove("x-parent-request-id");
        let parent = parent_headers(&headers).unwrap().unwrap();
        assert!(parent.request_id.is_empty());
        assert_eq!(parent.tool_call_id, "task");
    }

    #[test]
    fn cursor_max_body_bytes_is_set_to_128_mb() {
        assert_eq!(CURSOR_MAX_BODY_BYTES, 128 * 1024 * 1024);
    }
}
