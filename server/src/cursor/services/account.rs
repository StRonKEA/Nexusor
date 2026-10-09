//! Implements Cursor account information services.
use axum::{
    body::{to_bytes, Body},
    extract::{Extension, State},
    http::{header, HeaderValue, Request, Response},
};
use futures_util::StreamExt as _;
use prost::Message;
use serde_json::Value;

use crate::{api::cursor::proxy, cursor::transport::TransportRegistry, local_app, Result};

const LOCAL_AUTH_ID: &str = "cursor-local-user";
const LOCAL_EMAIL: &str = "cursor@ai.com";
const LOCAL_ULTRA_PLAN_INCLUDED_CENTS: i32 = 20_000;

#[derive(Clone, PartialEq, Message)]
struct GetEmailResponse {
    #[prost(string, tag = "1")]
    email: String,
    #[prost(int32, tag = "2")]
    sign_up_type: i32,
}

#[derive(Clone, PartialEq, Message)]
struct GetUserMetaResponse {
    #[prost(string, tag = "1")]
    email: String,
    #[prost(int32, tag = "2")]
    sign_up_type: i32,
    #[prost(int64, tag = "3")]
    user_id: i64,
    #[prost(string, optional, tag = "4")]
    workos_id: Option<String>,
    #[prost(string, optional, tag = "5")]
    profile_picture_url: Option<String>,
}

#[derive(Clone, PartialEq, Message)]
struct GetMeResponse {
    #[prost(string, tag = "1")]
    auth_id: String,
    #[prost(int32, tag = "2")]
    user_id: i32,
    #[prost(string, optional, tag = "3")]
    email: Option<String>,
    #[prost(string, optional, tag = "4")]
    first_name: Option<String>,
    #[prost(string, optional, tag = "5")]
    last_name: Option<String>,
    #[prost(string, optional, tag = "8")]
    created_at: Option<String>,
    #[prost(bool, optional, tag = "9")]
    is_enterprise_user: Option<bool>,
    #[prost(string, optional, tag = "11")]
    email_domain_type: Option<String>,
    #[prost(string, optional, tag = "12")]
    country: Option<String>,
    #[prost(string, optional, tag = "13")]
    profile_picture_url: Option<String>,
}

#[derive(Clone, PartialEq, Message)]
struct GetUserProfileResponse {
    #[prost(bool, optional, tag = "4")]
    public_visibility_allowed: Option<bool>,
    #[prost(string, optional, tag = "5")]
    max_visibility: Option<String>,
}

#[derive(Clone, PartialEq, Message)]
struct GetCurrentPeriodUsageResponse {
    #[prost(int64, tag = "1")]
    billing_cycle_start: i64,
    #[prost(int64, tag = "2")]
    billing_cycle_end: i64,
    #[prost(message, optional, tag = "3")]
    plan_usage: Option<PlanUsage>,
    #[prost(message, optional, tag = "4")]
    spend_limit_usage: Option<SpendLimitUsage>,
    #[prost(int32, optional, tag = "5")]
    display_threshold: Option<i32>,
    #[prost(bool, tag = "6")]
    enabled: bool,
    #[prost(string, tag = "7")]
    display_message: String,
    #[prost(string, optional, tag = "11")]
    auto_model_selected_display_message: Option<String>,
    #[prost(string, optional, tag = "12")]
    named_model_selected_display_message: Option<String>,
}

#[derive(Clone, PartialEq, Message)]
struct PlanUsage {
    #[prost(int32, tag = "1")]
    total_spend: i32,
    #[prost(int32, tag = "2")]
    included_spend: i32,
    #[prost(int32, tag = "4")]
    remaining: i32,
    #[prost(int32, tag = "5")]
    limit: i32,
    #[prost(bool, optional, tag = "6")]
    remaining_bonus: Option<bool>,
    #[prost(string, optional, tag = "7")]
    bonus_tooltip: Option<String>,
    #[prost(int32, optional, tag = "8")]
    auto_spend: Option<i32>,
    #[prost(int32, optional, tag = "9")]
    api_spend: Option<i32>,
    #[prost(double, optional, tag = "12")]
    auto_percent_used: Option<f64>,
    #[prost(double, optional, tag = "13")]
    api_percent_used: Option<f64>,
    #[prost(double, optional, tag = "14")]
    total_percent_used: Option<f64>,
}

#[derive(Clone, PartialEq, Message)]
struct SpendLimitUsage {
    #[prost(string, tag = "8")]
    limit_type: String,
}

#[derive(Clone, PartialEq, Message)]
struct GetUsageLimitStatusAndActiveGrantsResponse {
    #[prost(message, optional, tag = "1")]
    usage_limit_policy_status: Option<UsageLimitPolicyStatus>,
}

#[derive(Clone, PartialEq, Message)]
struct UsageLimitPolicyStatus {
    #[prost(bool, tag = "1")]
    is_in_slow_pool: bool,
    #[prost(map = "string, string", tag = "5")]
    features: std::collections::HashMap<String, String>,
    #[prost(bool, tag = "6")]
    can_configure_spend_limit: bool,
    #[prost(bool, tag = "8")]
    has_pending_request: bool,
    #[prost(string, repeated, tag = "9")]
    allowed_model_ids: Vec<String>,
    #[prost(string, repeated, tag = "10")]
    allowed_model_tags: Vec<String>,
}

#[derive(Clone, PartialEq, Message)]
pub struct GetPlanInfoResponse {
    #[prost(message, optional, tag = "1")]
    pub plan_info: Option<PlanInfoProto>,
    #[prost(message, optional, tag = "2")]
    pub next_upgrade: Option<NextUpgradeProto>,
}

#[derive(Clone, PartialEq, Message)]
pub struct PlanInfoProto {
    #[prost(string, tag = "1")]
    pub plan_name: String,
    #[prost(int32, tag = "2")]
    pub included_amount_cents: i32,
    #[prost(string, optional, tag = "3")]
    pub price: Option<String>,
    #[prost(int64, optional, tag = "4")]
    pub billing_cycle_end: Option<i64>,
    #[prost(int32, tag = "5")]
    pub plan_owner: i32,
}

#[derive(Clone, PartialEq, Message)]
pub struct NextUpgradeProto {
    #[prost(string, tag = "1")]
    pub tier: String,
    #[prost(string, tag = "2")]
    pub name: String,
    #[prost(int32, tag = "3")]
    pub included_amount_cents: i32,
    #[prost(string, tag = "4")]
    pub price: String,
    #[prost(string, tag = "5")]
    pub description: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct IsOnNewPricingResponse {
    #[prost(bool, tag = "1")]
    pub is_on_new_pricing: bool,
    #[prost(bool, tag = "2")]
    pub is_opted_out: bool,
    #[prost(bool, tag = "3")]
    pub has_auto_spillover: bool,
    #[prost(int32, optional, tag = "4")]
    pub dashboard_user_id: Option<i32>,
    #[prost(bool, tag = "5")]
    pub has_tiered_self_serve_team_spillover: bool,
}

#[derive(Clone, PartialEq, Message)]
pub struct GetManagedSkillsResponse {
    #[prost(message, repeated, tag = "1")]
    pub skills: Vec<ManagedSkillProto>,
}

#[derive(Clone, PartialEq, Message)]
pub struct ManagedSkillProto {
    #[prost(string, tag = "1")]
    pub id: String,
    #[prost(string, tag = "2")]
    pub description: String,
    #[prost(string, tag = "3")]
    pub content: String,
    #[prost(bool, tag = "4")]
    pub disable_model_invocation: bool,
    #[prost(string, repeated, tag = "5")]
    pub environments: Vec<String>,
    #[prost(string, repeated, tag = "6")]
    pub disabled_environments: Vec<String>,
    #[prost(bool, optional, tag = "7")]
    pub enabled: Option<bool>,
}

#[derive(Clone, PartialEq, Message)]
pub struct GetAvailableMcpServersResponse {
    #[prost(message, repeated, tag = "1")]
    pub servers: Vec<McpServerInfoProto>,
}

#[derive(Clone, PartialEq, Message)]
pub struct McpServerInfoProto {
    #[prost(int32, tag = "1")]
    pub id: i32,
    #[prost(string, tag = "2")]
    pub name: String,
    #[prost(bool, tag = "3")]
    pub is_team_server: bool,
    #[prost(bool, tag = "4")]
    pub enabled: bool,
    #[prost(string, tag = "5")]
    pub r#type: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct GetKnownServersResponse {
    #[prost(message, repeated, tag = "1")]
    pub servers: Vec<McpServerRegistrationProto>,
}

#[derive(Clone, PartialEq, Message)]
pub struct McpServerRegistrationProto {
    #[prost(string, tag = "1")]
    pub id: String,
    #[prost(string, tag = "2")]
    pub name: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct GetCliDownloadUrlResponse {
    #[prost(string, tag = "1")]
    pub url: String,
    #[prost(string, tag = "2")]
    pub version: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct CheckHttpMcpStatusResponse {
    #[prost(message, repeated, tag = "1")]
    pub statuses: Vec<McpServerStatusProto>,
}

#[derive(Clone, PartialEq, Message)]
pub struct McpServerStatusProto {
    #[prost(int32, tag = "1")]
    pub id: i32,
    #[prost(bool, tag = "2")]
    pub is_available: bool,
    #[prost(bool, tag = "3")]
    pub requires_auth: bool,
    #[prost(string, optional, tag = "4")]
    pub auth_url: Option<String>,
    #[prost(string, optional, tag = "5")]
    pub error: Option<String>,
    #[prost(bool, tag = "6")]
    pub has_valid_token: bool,
}

#[derive(Clone, PartialEq, Message)]
pub struct NameTabResponse {
    #[prost(string, tag = "1")]
    pub name: String,
    #[prost(string, tag = "2")]
    pub reason: String,
    #[prost(string, tag = "3")]
    pub icon: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct ServerTimeResponse {
    #[prost(double, tag = "1")]
    pub receive_timestamp: f64,
    #[prost(double, tag = "2")]
    pub transmit_timestamp: f64,
}

#[derive(Clone, PartialEq, Message)]
pub struct ListMarketplacesResponse {
    #[prost(message, repeated, tag = "1")]
    pub marketplaces: Vec<MarketplaceProto>,
}

#[derive(Clone, PartialEq, Message)]
pub struct MarketplaceProto {
    #[prost(int64, tag = "1")]
    pub id: i64,
    #[prost(string, tag = "2")]
    pub name: String,
    #[prost(string, tag = "3")]
    pub description: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct ListMarketplacePluginsResponse {
    #[prost(message, repeated, tag = "1")]
    pub plugins: Vec<MarketplacePluginProto>,
    #[prost(string, optional, tag = "2")]
    pub next_page_token: Option<String>,
    #[prost(bool, tag = "3")]
    pub has_more: bool,
}

#[derive(Clone, PartialEq, Message)]
pub struct MarketplacePluginProto {
    #[prost(int64, tag = "1")]
    pub id: i64,
    #[prost(string, tag = "2")]
    pub name: String,
    #[prost(string, tag = "3")]
    pub description: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct WriteGitBranchNameRequest {
    #[prost(string, tag = "1")]
    pub diffs: String,
    #[prost(string, optional, tag = "2")]
    pub context: Option<String>,
    #[prost(string, optional, tag = "3")]
    pub conversation_id: Option<String>,
}

#[derive(Clone, PartialEq, Message)]
pub struct WriteGitBranchNameResponse {
    #[prost(string, tag = "1")]
    pub branch_name: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct AutoContextRequest {
    #[prost(string, tag = "1")]
    pub text: String,
    #[prost(message, repeated, tag = "2")]
    pub candidate_files: Vec<AutoContextFileProto>,
}

#[derive(Clone, PartialEq, Message)]
pub struct AutoContextFileProto {
    #[prost(string, tag = "1")]
    pub relative_workspace_path: String,
    #[prost(string, tag = "2")]
    pub file_content: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct AutoContextResponse {
    #[prost(message, repeated, tag = "1")]
    pub ranked_files: Vec<AutoContextRankedFileProto>,
}

#[derive(Clone, PartialEq, Message)]
pub struct AutoContextRankedFileProto {
    #[prost(string, tag = "1")]
    pub relative_workspace_path: String,
    #[prost(float, tag = "2")]
    pub reranking_score: f32,
}

#[derive(Clone, PartialEq, Message)]
pub struct ContextRerankingRequest {
    #[prost(message, optional, tag = "1")]
    pub current_file: Option<crate::cursor::protocol::proto::tab::CurrentFile>,
    #[prost(message, repeated, tag = "2")]
    pub chat_conversation_history: Vec<RerankingMessage>,
    #[prost(message, repeated, tag = "3")]
    pub cpp_diff_trajectories:
        Vec<crate::cursor::protocol::proto::aiserver::v1::CppFileDiffHistory>,
    #[prost(message, repeated, tag = "4")]
    pub candidate_files: Vec<ContextRerankingCandidateFileProto>,
}

#[derive(Clone, PartialEq, Message)]
pub struct RerankingMessage {
    #[prost(string, tag = "1")]
    pub text: String,
    #[prost(int32, tag = "2")]
    pub message_type: i32,
}

#[derive(Clone, PartialEq, Message)]
pub struct ContextRerankingCandidateFileProto {
    #[prost(string, tag = "1")]
    pub file_name: String,
    #[prost(string, tag = "2")]
    pub file_content: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct ContextRerankingResponse {
    #[prost(float, repeated, tag = "1")]
    pub reranking_scores: Vec<f32>,
}

#[derive(Clone, PartialEq, Message)]
pub struct CreateTranscriptOverviewRequest {
    #[prost(string, tag = "1")]
    pub formatted_conversation: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct CreateTranscriptOverviewResponse {
    #[prost(string, tag = "1")]
    pub overview: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct RerankCmdKContextRequest {
    #[prost(message, repeated, tag = "1")]
    pub context_items: Vec<PotentiallyCachedContextItemProto>,
}

#[derive(Clone, PartialEq, Message)]
pub struct PotentiallyCachedContextItemProto {
    #[prost(string, tag = "2")]
    pub context_item_hash: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct RerankCmdKContextResponse {
    #[prost(message, optional, tag = "1")]
    pub context_status_update: Option<ContextStatusUpdateProto>,
    #[prost(bool, optional, tag = "3")]
    pub did_call: Option<bool>,
}

#[derive(Clone, PartialEq, Message)]
pub struct ContextStatusUpdateProto {
    #[prost(message, repeated, tag = "1")]
    pub context_item_statuses: Vec<ContextItemStatusProto>,
}

#[derive(Clone, PartialEq, Message)]
pub struct ContextItemStatusProto {
    #[prost(string, tag = "1")]
    pub context_item_hash: String,
    #[prost(bool, tag = "2")]
    pub shown_to_the_model: bool,
    #[prost(float, tag = "3")]
    pub score: f32,
    #[prost(float, tag = "4")]
    pub percentage_of_available_space: f32,
}

#[derive(Clone, PartialEq, Message)]
pub struct BugConfigRequest {
    #[prost(bool, tag = "1")]
    pub telem_enabled: bool,
}

#[derive(Clone, PartialEq, Message)]
pub struct BugConfigResponse {
    #[prost(message, optional, tag = "1")]
    pub linter_strategy_v1: Option<LinterStrategyProto>,
    #[prost(message, optional, tag = "2")]
    pub bug_bot_v1: Option<BugBotV1Proto>,
}

#[derive(Clone, PartialEq, Message)]
pub struct LinterStrategyProto {
    #[prost(bool, tag = "1")]
    pub enabled: bool,
    #[prost(bool, tag = "2")]
    pub try_trigger_on_save: bool,
}

#[derive(Clone, PartialEq, Message)]
pub struct BugBotV1Proto {
    #[prost(bool, tag = "1")]
    pub enabled: bool,
    #[prost(bool, tag = "2")]
    pub is_subsidized: bool,
    #[prost(int32, tag = "3")]
    pub background_call_frequency_ms: i32,
}

#[derive(Clone, PartialEq, Message)]
pub struct ReviewRequestV2 {
    #[prost(message, repeated, tag = "1")]
    pub file_diffs: Vec<ReviewFileDiffProto>,
    #[prost(string, optional, tag = "2")]
    pub linter_rules: Option<String>,
}

#[derive(Clone, PartialEq, Message)]
pub struct ReviewFileDiffProto {
    #[prost(message, optional, tag = "1")]
    pub file: Option<ReviewFileProto>,
    #[prost(message, repeated, tag = "2")]
    pub chunk_diffs: Vec<ReviewChunkDiffProto>,
}

#[derive(Clone, PartialEq, Message)]
pub struct ReviewFileProto {
    #[prost(string, tag = "1")]
    pub relative_workspace_path: String,
    #[prost(string, tag = "2")]
    pub contents: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct ReviewChunkDiffProto {
    #[prost(string, tag = "1")]
    pub diff_string: String,
    #[prost(int32, tag = "2")]
    pub old_start: i32,
    #[prost(int32, tag = "3")]
    pub new_start: i32,
    #[prost(int32, tag = "4")]
    pub old_lines: i32,
    #[prost(int32, tag = "5")]
    pub new_lines: i32,
}

#[derive(Clone, PartialEq, Message)]
pub struct ReviewResponseV2 {
    #[prost(message, optional, tag = "1")]
    pub bug: Option<ReviewBugV2Proto>,
}

#[derive(Clone, PartialEq, Message)]
pub struct ReviewBugV2Proto {
    #[prost(string, tag = "1")]
    pub id: String,
    #[prost(string, tag = "2")]
    pub chunk_id: String,
    #[prost(string, tag = "3")]
    pub relative_workspace_path: String,
    #[prost(int32, tag = "4")]
    pub start_line: i32,
    #[prost(int32, tag = "5")]
    pub end_line: i32,
    #[prost(string, tag = "6")]
    pub description: String,
    #[prost(int32, tag = "7")]
    pub severity: i32,
    #[prost(string, tag = "8")]
    pub tldr: String,
    #[prost(string, tag = "9")]
    pub diff: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct CursorPredictionConfigResponse {
    #[prost(string, tag = "2")]
    pub default_model: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct WarmApplyResponse {}

#[derive(Clone, PartialEq, Message)]
pub struct ReportEditFateResponse {}

#[derive(Clone, PartialEq, Message)]
pub struct LintExplanationResponse2 {
    #[prost(string, tag = "1")]
    pub orig_line: String,
    #[prost(string, tag = "2")]
    pub new_line: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct NameAgentRequest {
    #[prost(string, tag = "1")]
    pub user_message: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct NameAgentResponse {
    #[prost(string, tag = "1")]
    pub name: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct GetAllowedModelIntentsResponse {
    #[prost(string, repeated, tag = "1")]
    pub model_intents: Vec<String>,
}

#[derive(Clone, PartialEq, Message)]
pub struct LintFileResponse {
    #[prost(message, repeated, tag = "1")]
    pub tokens: Vec<LintTokenProto>,
}

#[derive(Clone, PartialEq, Message)]
pub struct LintTokenProto {}

#[derive(Clone, PartialEq, Message)]
pub struct LintChunkResponse {
    #[prost(message, repeated, tag = "1")]
    pub chunk_tokens: Vec<LintTokenProto>,
}

#[derive(Clone, PartialEq, Message)]
pub struct ShouldTurnOnCppOnboardingResponse {
    #[prost(bool, tag = "1")]
    pub should_turn_on_cpp_onboarding: bool,
}

#[derive(Clone, PartialEq, Message)]
pub struct IsGitGraphEnabledResponse {
    #[prost(bool, tag = "1")]
    pub is_enabled: bool,
}

#[derive(Clone, PartialEq, Message)]
pub struct GetGitGraphStatusResponse {
    #[prost(int32, tag = "1")]
    pub num_successful_commits: i32,
    #[prost(int32, tag = "2")]
    pub num_failed_commits: i32,
}

#[derive(Clone, PartialEq, Message)]
pub struct GetGitGraphRelatedFilesResponse {
    #[prost(message, repeated, tag = "1")]
    pub related_files: Vec<GitGraphRelatedFileProto>,
    #[prost(float, tag = "2")]
    pub time_taken_ms: f32,
}

#[derive(Clone, PartialEq, Message)]
pub struct GitGraphRelatedFileProto {
    #[prost(string, tag = "1")]
    pub encrypted_relative_path: String,
    #[prost(double, tag = "2")]
    pub weight: f64,
}

#[derive(Clone, PartialEq, Message)]
pub struct GetFullSelfDrivingConfigResponse {
    #[prost(bool, tag = "1")]
    pub has_config: bool,
    #[prost(bool, tag = "2")]
    pub enabled: bool,
}

#[derive(Clone, PartialEq, Message)]
pub struct CheckFeaturesStatusRequest {
    #[prost(string, repeated, tag = "1")]
    pub feature_names: Vec<String>,
}

#[derive(Clone, PartialEq, Message)]
pub struct CheckFeaturesStatusResponse {
    #[prost(message, repeated, tag = "1")]
    pub feature_statuses: Vec<FeatureStatusProto>,
}

#[derive(Clone, PartialEq, Message)]
pub struct FeatureStatusProto {
    #[prost(string, tag = "1")]
    pub feature_name: String,
    #[prost(bool, tag = "2")]
    pub enabled: bool,
}

#[derive(Clone, PartialEq, Message)]
pub struct CheckFeatureStatusRequest {
    #[prost(string, tag = "1")]
    pub feature_name: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct CheckFeatureStatusResponse {
    #[prost(bool, tag = "1")]
    pub enabled: bool,
}

#[derive(Clone, PartialEq, Message)]
pub struct WarmComposerCacheResponse {
    #[prost(bool, tag = "1")]
    pub did_warm_cache: bool,
}

#[derive(Clone, PartialEq, Message)]
pub struct KeepComposerCacheWarmResponse {
    #[prost(bool, tag = "1")]
    pub did_keep_warm: bool,
}

#[derive(Clone, PartialEq, Message)]
pub struct CountTokensRequest {
    #[prost(string, tag = "2")]
    pub model_name: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct CountTokensResponse {
    #[prost(int32, tag = "1")]
    pub count: i32,
}

#[derive(Clone, Copy, PartialEq, Message)]
struct Empty {}

pub async fn get_email(
    State(_registry): State<TransportRegistry>,
    Extension(upstream): Extension<proxy::CursorProxy>,
    request: Request<Body>,
) -> Result<Response<Body>> {
    if local_app::request_uses_local_cursor_token(request.headers()) {
        consume_body(request).await?;
        return proto(GetEmailResponse {
            email: LOCAL_EMAIL.into(),
            sign_up_type: 3,
        });
    }
    proxy::forward(Extension(upstream), request).await
}

pub async fn get_user_meta(
    State(_registry): State<TransportRegistry>,
    Extension(upstream): Extension<proxy::CursorProxy>,
    request: Request<Body>,
) -> Result<Response<Body>> {
    if local_app::request_uses_local_cursor_token(request.headers()) {
        consume_body(request).await?;
        return proto(GetUserMetaResponse {
            email: LOCAL_EMAIL.into(),
            sign_up_type: 3,
            user_id: 1,
            workos_id: None,
            profile_picture_url: None,
        });
    }
    proxy::forward(Extension(upstream), request).await
}

pub async fn get_me(
    State(_registry): State<TransportRegistry>,
    Extension(upstream): Extension<proxy::CursorProxy>,
    request: Request<Body>,
) -> Result<Response<Body>> {
    if local_app::request_uses_local_cursor_token(request.headers()) {
        consume_body(request).await?;
        return proto(GetMeResponse {
            auth_id: LOCAL_AUTH_ID.into(),
            user_id: 1,
            email: Some(LOCAL_EMAIL.into()),
            first_name: Some("Nexusor".into()),
            last_name: Some("Pro".into()),
            created_at: Some(chrono::Utc::now().to_rfc3339()),
            is_enterprise_user: Some(true),
            email_domain_type: Some("enterprise".into()),
            country: Some("US".into()),
            profile_picture_url: None,
        });
    }
    proxy::forward(Extension(upstream), request).await
}

pub async fn get_teams(
    State(registry): State<TransportRegistry>,
    Extension(upstream): Extension<proxy::CursorProxy>,
    request: Request<Body>,
) -> Result<Response<Body>> {
    local_or_takeover_or_forward(&registry, upstream, request, || proto(Empty {})).await
}

pub async fn get_user_profile(
    State(registry): State<TransportRegistry>,
    Extension(upstream): Extension<proxy::CursorProxy>,
    request: Request<Body>,
) -> Result<Response<Body>> {
    local_or_takeover_or_forward(&registry, upstream, request, || {
        proto(GetUserProfileResponse {
            public_visibility_allowed: Some(true),
            max_visibility: Some("PUBLIC".into()),
        })
    })
    .await
}

pub async fn current_period_usage(
    State(registry): State<TransportRegistry>,
    Extension(upstream): Extension<proxy::CursorProxy>,
    request: Request<Body>,
) -> Result<Response<Body>> {
    local_or_takeover_or_forward(&registry, upstream, request, || {
        let now = chrono::Utc::now();
        proto(GetCurrentPeriodUsageResponse {
            billing_cycle_start: (now - chrono::Duration::days(30)).timestamp_millis(),
            billing_cycle_end: (now + chrono::Duration::days(10 * 365)).timestamp_millis(),
            plan_usage: Some(PlanUsage {
                total_spend: 0,
                included_spend: LOCAL_ULTRA_PLAN_INCLUDED_CENTS,
                remaining: LOCAL_ULTRA_PLAN_INCLUDED_CENTS,
                limit: LOCAL_ULTRA_PLAN_INCLUDED_CENTS,
                remaining_bonus: Some(false),
                bonus_tooltip: Some("Ultra local account mock is active.".into()),
                auto_spend: Some(0),
                api_spend: Some(0),
                auto_percent_used: Some(0.0),
                api_percent_used: Some(0.0),
                total_percent_used: Some(0.0),
            }),
            spend_limit_usage: Some(SpendLimitUsage {
                limit_type: "user".into(),
            }),
            display_threshold: Some(99_999_999),
            enabled: true,
            display_message: "Ultra plan active".into(),
            auto_model_selected_display_message: Some("Ultra plan active".into()),
            named_model_selected_display_message: Some("Ultra plan active".into()),
        })
    })
    .await
}

pub async fn usage_limit_status(
    State(registry): State<TransportRegistry>,
    Extension(upstream): Extension<proxy::CursorProxy>,
    request: Request<Body>,
) -> Result<Response<Body>> {
    local_or_takeover_or_forward(&registry, upstream, request, || {
        proto(GetUsageLimitStatusAndActiveGrantsResponse {
            usage_limit_policy_status: Some(UsageLimitPolicyStatus {
                is_in_slow_pool: false,
                features: Default::default(),
                can_configure_spend_limit: true,
                has_pending_request: false,
                allowed_model_ids: Vec::new(),
                allowed_model_tags: Vec::new(),
            }),
        })
    })
    .await
}

pub async fn stripe_profile(
    Extension(upstream): Extension<proxy::CursorProxy>,
    request: Request<Body>,
) -> Result<Response<Body>> {
    let origin = request.headers().get(header::ORIGIN).cloned();
    if local_app::request_uses_local_cursor_token(request.headers()) {
        consume_body(request).await?;
        return local_stripe_profile(origin);
    }
    // Forward to official upstream to preserve real account session and payment ID
    let response = proxy::forward(Extension(upstream), request).await?;
    if !response.status().is_success() {
        return Ok(response);
    }
    let (mut parts, body) = response.into_parts();
    let bytes = match to_bytes(body, 2 * 1024 * 1024).await {
        Ok(bytes) => bytes,
        Err(_) => return Ok(Response::from_parts(parts, Body::empty())),
    };
    if let Ok(mut json_val) = serde_json::from_slice::<Value>(&bytes) {
        if let Some(obj) = json_val.as_object_mut() {
            obj.insert("membershipType".into(), Value::String("ultra".into()));
            obj.insert("individualMembershipType".into(), Value::String("ultra".into()));
            obj.insert("subscriptionStatus".into(), Value::String("active".into()));
            if let Ok(modified_bytes) = serde_json::to_vec(&json_val) {
                let len = modified_bytes.len();
                parts.headers.remove(header::ETAG);
                parts.headers.remove(header::CONTENT_LENGTH);
                if let Ok(val) = len.to_string().parse() {
                    parts.headers.insert(header::CONTENT_LENGTH, val);
                }
                return Ok(Response::from_parts(parts, Body::from(modified_bytes)));
            }
        }
    }
    Ok(Response::from_parts(parts, Body::from(bytes)))
}

pub async fn get_plan_info(
    State(registry): State<TransportRegistry>,
    Extension(upstream): Extension<proxy::CursorProxy>,
    request: Request<Body>,
) -> Result<Response<Body>> {
    local_or_takeover_or_forward(&registry, upstream, request, || {
        let now = chrono::Utc::now();
        proto(GetPlanInfoResponse {
            plan_info: Some(PlanInfoProto {
                plan_name: "Ultra".into(),
                included_amount_cents: LOCAL_ULTRA_PLAN_INCLUDED_CENTS,
                price: Some("$200/mo".into()),
                billing_cycle_end: Some(
                    (now + chrono::Duration::days(10 * 365)).timestamp_millis(),
                ),
                plan_owner: 1, // PLAN_OWNER_STRIPE
            }),
            next_upgrade: None,
        })
    })
    .await
}

pub async fn is_on_new_pricing(
    State(registry): State<TransportRegistry>,
    Extension(upstream): Extension<proxy::CursorProxy>,
    request: Request<Body>,
) -> Result<Response<Body>> {
    local_or_takeover_or_forward(&registry, upstream, request, || {
        proto(IsOnNewPricingResponse {
            is_on_new_pricing: true,
            is_opted_out: false,
            has_auto_spillover: true,
            dashboard_user_id: Some(1),
            has_tiered_self_serve_team_spillover: false,
        })
    })
    .await
}

pub async fn has_valid_payment_method(
    State(registry): State<TransportRegistry>,
    Extension(upstream): Extension<proxy::CursorProxy>,
    request: Request<Body>,
) -> Result<Response<Body>> {
    local_or_takeover_or_forward(&registry, upstream, request, || {
        json(serde_json::json!({
            "has_valid_payment_method": true,
            "has_payment_method": true
        }))
    })
    .await
}

pub async fn get_managed_skills(
    State(registry): State<TransportRegistry>,
    Extension(upstream): Extension<proxy::CursorProxy>,
    request: Request<Body>,
) -> Result<Response<Body>> {
    local_or_takeover_or_forward(&registry, upstream, request, || {
        proto(GetManagedSkillsResponse { skills: Vec::new() })
    })
    .await
}

pub async fn get_available_mcp_servers(
    State(registry): State<TransportRegistry>,
    Extension(upstream): Extension<proxy::CursorProxy>,
    request: Request<Body>,
) -> Result<Response<Body>> {
    local_or_takeover_or_forward(&registry, upstream, request, || {
        proto(GetAvailableMcpServersResponse {
            servers: Vec::new(),
        })
    })
    .await
}

pub async fn get_known_servers(
    State(registry): State<TransportRegistry>,
    Extension(upstream): Extension<proxy::CursorProxy>,
    request: Request<Body>,
) -> Result<Response<Body>> {
    local_or_takeover_or_forward(&registry, upstream, request, || {
        proto(GetKnownServersResponse {
            servers: Vec::new(),
        })
    })
    .await
}

pub async fn get_cli_download_url(
    State(registry): State<TransportRegistry>,
    Extension(upstream): Extension<proxy::CursorProxy>,
    request: Request<Body>,
) -> Result<Response<Body>> {
    local_or_takeover_or_forward(&registry, upstream, request, || {
        proto(GetCliDownloadUrlResponse {
            url: "https://cursor.com".into(),
            version: "1.0.0".into(),
        })
    })
    .await
}

pub async fn count_tokens(request: Request<Body>) -> Result<Response<Body>> {
    let bytes = to_bytes(request.into_body(), usize::MAX)
        .await
        .map_err(|e| crate::Error::Protocol(format!("cannot read CountTokens body: {e}")))?;
    let token_count = (bytes.len() as f64 / 4.0).ceil() as i32;
    proto(CountTokensResponse {
        count: token_count.max(1),
    })
}

pub async fn warm_composer_cache(_request: Request<Body>) -> Result<Response<Body>> {
    proto(WarmComposerCacheResponse {
        did_warm_cache: true,
    })
}

pub async fn keep_composer_cache_warm(_request: Request<Body>) -> Result<Response<Body>> {
    proto(KeepComposerCacheWarmResponse {
        did_keep_warm: true,
    })
}

pub async fn check_features_status(
    Extension(upstream): Extension<proxy::CursorProxy>,
    request: Request<Body>,
) -> Result<Response<Body>> {
    if !local_app::request_uses_local_cursor_token(request.headers()) {
        return proxy::forward(Extension(upstream), request).await;
    }
    let bytes = to_bytes(request.into_body(), usize::MAX)
        .await
        .map_err(|e| {
            crate::Error::Protocol(format!("cannot read CheckFeaturesStatus body: {e}"))
        })?;
    let mut statuses = Vec::new();
    if let Ok(req) = CheckFeaturesStatusRequest::decode(bytes.as_ref()) {
        for name in req.feature_names {
            statuses.push(FeatureStatusProto {
                feature_name: name,
                enabled: true,
            });
        }
    }
    proto(CheckFeaturesStatusResponse {
        feature_statuses: statuses,
    })
}

pub async fn check_feature_status(
    Extension(upstream): Extension<proxy::CursorProxy>,
    request: Request<Body>,
) -> Result<Response<Body>> {
    if !local_app::request_uses_local_cursor_token(request.headers()) {
        return proxy::forward(Extension(upstream), request).await;
    }
    consume_body(request).await?;
    proto(CheckFeatureStatusResponse { enabled: true })
}

pub async fn check_http_mcp_status(
    State(registry): State<TransportRegistry>,
    Extension(upstream): Extension<proxy::CursorProxy>,
    request: Request<Body>,
) -> Result<Response<Body>> {
    local_or_takeover_or_forward(&registry, upstream, request, || {
        proto(CheckHttpMcpStatusResponse {
            statuses: Vec::new(),
        })
    })
    .await
}

pub async fn name_tab(request: Request<Body>) -> Result<Response<Body>> {
    let bytes = to_bytes(request.into_body(), usize::MAX)
        .await
        .map_err(|e| crate::Error::Protocol(format!("cannot read NameTab body: {e}")))?;
    let mut title = "Chat Session".to_string();
    if let Ok(text) = std::str::from_utf8(&bytes) {
        let clean = text
            .chars()
            .filter(|c| c.is_alphanumeric() || c.is_whitespace())
            .collect::<String>();
        let words: Vec<&str> = clean
            .split_whitespace()
            .filter(|w| w.len() > 2)
            .take(4)
            .collect();
        if !words.is_empty() {
            title = words.join(" ");
        }
    }
    proto(NameTabResponse {
        name: title,
        reason: "local".into(),
        icon: "chat".into(),
    })
}

pub async fn auto_context(request: Request<Body>) -> Result<Response<Body>> {
    let bytes = to_bytes(
        request.into_body(),
        crate::api::cursor::CURSOR_MAX_BODY_BYTES,
    )
    .await
    .map_err(|e| crate::Error::Protocol(format!("cannot read AutoContext body: {e}")))?;
    let req: AutoContextRequest = crate::cursor::protocol::connect::decode_unary(&bytes)?;
    let scores = super::context_ranking::scores(
        &req.text,
        req.candidate_files.iter().map(|file| {
            (
                file.relative_workspace_path.as_str(),
                file.file_content.as_str(),
            )
        }),
    );
    let mut ranked: Vec<_> = req
        .candidate_files
        .into_iter()
        .zip(scores)
        .map(|(file, score)| AutoContextRankedFileProto {
            relative_workspace_path: file.relative_workspace_path,
            reranking_score: score,
        })
        .collect();
    ranked.sort_by(|a, b| b.reranking_score.total_cmp(&a.reranking_score));
    proto(AutoContextResponse {
        ranked_files: ranked,
    })
}

pub async fn context_reranking(request: Request<Body>) -> Result<Response<Body>> {
    let bytes = to_bytes(
        request.into_body(),
        crate::api::cursor::CURSOR_MAX_BODY_BYTES,
    )
    .await
    .map_err(|e| crate::Error::Protocol(format!("cannot read ContextReranking body: {e}")))?;
    let req: ContextRerankingRequest = crate::cursor::protocol::connect::decode_unary(&bytes)?;
    let mut query = req
        .chat_conversation_history
        .iter()
        .rev()
        .find(|message| message.message_type == 1)
        .map(|message| message.text.clone())
        .unwrap_or_default();
    if let Some(file) = req.current_file {
        query.push_str(&format!(
            "\n{}\n{}",
            file.relative_workspace_path, file.contents
        ));
    }
    for diff in req.cpp_diff_trajectories.iter().rev().take(3) {
        query.push_str(&format!(
            "\n{}\n{}",
            diff.file_name,
            diff.diff_history.join("\n")
        ));
    }
    let scores = super::context_ranking::scores(
        &query,
        req.candidate_files
            .iter()
            .map(|file| (file.file_name.as_str(), file.file_content.as_str())),
    );
    proto(ContextRerankingResponse {
        reranking_scores: scores,
    })
}

pub async fn rerank_cmdk_context(request: Request<Body>) -> Result<Response<Body>> {
    let bytes = to_bytes(
        request.into_body(),
        crate::api::cursor::CURSOR_MAX_BODY_BYTES,
    )
    .await
    .map_err(|e| crate::Error::Protocol(format!("cannot read RerankCmdKContext body: {e}")))?;
    let req: RerankCmdKContextRequest = crate::cursor::protocol::connect::decode_unary(&bytes)?;
    let statuses: Vec<ContextItemStatusProto> = req
        .context_items
        .into_iter()
        .map(|item| ContextItemStatusProto {
            context_item_hash: item.context_item_hash,
            shown_to_the_model: true,
            score: 1.0,
            percentage_of_available_space: 0.1,
        })
        .collect();
    proto(RerankCmdKContextResponse {
        context_status_update: Some(ContextStatusUpdateProto {
            context_item_statuses: statuses,
        }),
        did_call: Some(true),
    })
}

pub async fn create_transcript_overview(
    State(registry): State<TransportRegistry>,
    request: Request<Body>,
) -> Result<Response<Body>> {
    let bytes = to_bytes(
        request.into_body(),
        crate::api::cursor::CURSOR_MAX_BODY_BYTES,
    )
    .await
    .map_err(|e| crate::Error::Protocol(format!("cannot read CreateTranscriptOverview body: {e}")))?;
    let req: CreateTranscriptOverviewRequest = crate::cursor::protocol::connect::decode_unary(&bytes)?;
    
    // Generate overview using fast model from AutoRouter / Provider
    let prompt = format!(
        "Summarize the following developer conversation in 1-2 concise, high-density sentences describing what was accomplished:\n\n{}",
        req.formatted_conversation
    );
    let mut overview = String::new();
    
    // We execute via provider router with fast intent
    let call_id = format!("overview-{}", uuid::Uuid::new_v4());
    let invocation = crate::model::ModelInvocation {
        call_id: call_id.clone(),
        run_id: call_id.clone(),
        conversation_id: call_id.clone(),
        provider_call_index: 0,
        request: crate::model::ModelRequest {
            prompt: crate::model::PromptSpec {
                instructions: "You are a concise engineering activity summarizer.".into(),
                tools: Vec::new(),
            },
            model: crate::model::ModelSpec::new(crate::store::AutoRouterConfig::SUBAGENT_ROUTE),
            history: vec![crate::model::ProjectedMessage {
                message_id: "overview-req".into(),
                role: crate::model::Role::User,
                content: crate::model::ProjectedContent::Parts(vec![crate::model::ContentPart::Text {
                    text: prompt,
                }]),
            }],
        },
        slot_account_ids: None,
        slot_strategy: None,
    };
    
    let stream = registry.provider().stream(invocation, tokio_util::sync::CancellationToken::new());
    futures_util::pin_mut!(stream);
    while let Some(Ok(event)) = stream.next().await {
        if let crate::provider::ModelEvent::TextDelta(delta) = event {
            overview.push_str(&delta);
        }
    }
    
    if overview.trim().is_empty() {
        overview = "Session completed with local project updates.".to_string();
    }
    
    proto(CreateTranscriptOverviewResponse {
        overview: overview.trim().to_string(),
    })
}

pub async fn bug_config(_request: Request<Body>) -> Result<Response<Body>> {
    proto(BugConfigResponse {
        linter_strategy_v1: Some(LinterStrategyProto {
            enabled: true,
            try_trigger_on_save: true,
        }),
        bug_bot_v1: Some(BugBotV1Proto {
            enabled: true,
            is_subsidized: true,
            background_call_frequency_ms: 60_000,
        }),
    })
}

pub async fn stream_review(
    State(registry): State<TransportRegistry>,
    request: Request<Body>,
) -> Result<Response<Body>> {
    let bytes = to_bytes(
        request.into_body(),
        crate::api::cursor::CURSOR_MAX_BODY_BYTES,
    )
    .await
    .map_err(|e| crate::Error::Protocol(format!("cannot read StreamReview body: {e}")))?;
    
    let req: ReviewRequestV2 = match crate::cursor::protocol::connect::decode_unary(&bytes) {
        Ok(r) => r,
        Err(_) => {
            let empty_res = ReviewResponseV2 { bug: None };
            return proto(empty_res);
        }
    };

    let mut diff_summary = String::new();
    for f in &req.file_diffs {
        if let Some(file) = &f.file {
            diff_summary.push_str(&format!("\nFile: {}\n", file.relative_workspace_path));
        }
        for chunk in &f.chunk_diffs {
            diff_summary.push_str(&chunk.diff_string);
            diff_summary.push('\n');
        }
    }

    if diff_summary.trim().is_empty() {
        return proto(ReviewResponseV2 { bug: None });
    }

    // Call dynamic coding/reasoning model for review
    let prompt = format!(
        "Perform a thorough code review of the following changes. Identify any obvious bugs, security issues, or regressions:\n\n{}",
        diff_summary
    );
    
    let call_id = format!("review-{}", uuid::Uuid::new_v4());
    let invocation = crate::model::ModelInvocation {
        call_id: call_id.clone(),
        run_id: call_id.clone(),
        conversation_id: call_id.clone(),
        provider_call_index: 0,
        request: crate::model::ModelRequest {
            prompt: crate::model::PromptSpec {
                instructions: "You are a senior code review assistant.".into(),
                tools: Vec::new(),
            },
            model: crate::model::ModelSpec::new(crate::store::AutoRouterConfig::SUBAGENT_ROUTE),
            history: vec![crate::model::ProjectedMessage {
                message_id: "review-req".into(),
                role: crate::model::Role::User,
                content: crate::model::ProjectedContent::Parts(vec![crate::model::ContentPart::Text {
                    text: prompt,
                }]),
            }],
        },
        slot_account_ids: None,
        slot_strategy: None,
    };

    let mut findings = String::new();
    let stream = registry.provider().stream(invocation, tokio_util::sync::CancellationToken::new());
    futures_util::pin_mut!(stream);
    while let Some(Ok(event)) = stream.next().await {
        if let crate::provider::ModelEvent::TextDelta(delta) = event {
            findings.push_str(&delta);
        }
    }

    let first_file = req
        .file_diffs
        .first()
        .and_then(|f| f.file.as_ref())
        .map(|f| f.relative_workspace_path.clone())
        .unwrap_or_else(|| "workspace".into());

    let response_bug = ReviewResponseV2 {
        bug: Some(ReviewBugV2Proto {
            id: format!("bug-{}", uuid::Uuid::new_v4()),
            chunk_id: "chunk-0".into(),
            relative_workspace_path: first_file,
            start_line: 1,
            end_line: 10,
            description: findings.trim().to_string(),
            severity: 1, // INFO/WARNING
            tldr: "Code review findings from Nexusor BYOK".into(),
            diff: String::new(),
        }),
    };

    proto(response_bug)
}

pub async fn cursor_prediction_config() -> Result<Response<Body>> {
    proto(CursorPredictionConfigResponse {
        default_model: "cursor-fast".into(),
    })
}

pub async fn warm_apply() -> Result<Response<Body>> {
    proto(WarmApplyResponse {})
}

pub async fn report_edit_fate() -> Result<Response<Body>> {
    proto(ReportEditFateResponse {})
}

pub async fn lint_explanation2() -> Result<Response<Body>> {
    proto(LintExplanationResponse2 {
        orig_line: String::new(),
        new_line: String::new(),
    })
}

pub async fn name_agent(request: Request<Body>) -> Result<Response<Body>> {
    let bytes = to_bytes(
        request.into_body(),
        crate::api::cursor::CURSOR_MAX_BODY_BYTES,
    )
    .await
    .map_err(|e| crate::Error::Protocol(format!("cannot read NameAgent body: {e}")))?;
    
    let mut name = "Agent Task".to_string();
    if let Ok(req) = NameAgentRequest::decode(bytes.as_ref()) {
        let clean: String = req
            .user_message
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == ' ' || *c == '-')
            .collect();
        let words: Vec<&str> = clean.split_whitespace().take(4).collect();
        if !words.is_empty() {
            name = words.join(" ");
        }
    }
    proto(NameAgentResponse { name })
}

pub async fn get_allowed_model_intents() -> Result<Response<Body>> {
    proto(GetAllowedModelIntentsResponse {
        model_intents: vec![
            "agent".into(),
            "chat".into(),
            "cmd_k".into(),
            "terminal_cmd_k".into(),
            "fast".into(),
            "reasoning".into(),
            "review".into(),
            "subagent".into(),
        ],
    })
}

pub async fn lint_file() -> Result<Response<Body>> {
    proto(LintFileResponse { tokens: Vec::new() })
}

pub async fn lint_chunk() -> Result<Response<Body>> {
    proto(LintChunkResponse { chunk_tokens: Vec::new() })
}

pub async fn should_turn_on_cpp_onboarding() -> Result<Response<Body>> {
    proto(ShouldTurnOnCppOnboardingResponse {
        should_turn_on_cpp_onboarding: false,
    })
}

pub async fn is_git_graph_enabled() -> Result<Response<Body>> {
    proto(IsGitGraphEnabledResponse {
        is_enabled: true,
    })
}

pub async fn get_git_graph_status() -> Result<Response<Body>> {
    proto(GetGitGraphStatusResponse {
        num_successful_commits: 1,
        num_failed_commits: 0,
    })
}

pub async fn get_git_graph_related_files() -> Result<Response<Body>> {
    proto(GetGitGraphRelatedFilesResponse {
        related_files: Vec::new(),
        time_taken_ms: 1.0,
    })
}

pub async fn write_git_branch_name(request: Request<Body>) -> Result<Response<Body>> {
    let bytes = to_bytes(request.into_body(), usize::MAX)
        .await
        .map_err(|e| crate::Error::Protocol(format!("cannot read WriteGitBranchName body: {e}")))?;
    let mut branch = "feat/update".to_string();
    if let Ok(req) = WriteGitBranchNameRequest::decode(bytes.as_ref()) {
        let hint = req
            .context
            .as_deref()
            .or_else(|| (!req.diffs.is_empty()).then_some(req.diffs.as_str()))
            .unwrap_or("");
        let clean: String = hint
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == ' ' || *c == '-')
            .collect();
        let words: Vec<&str> = clean
            .split_whitespace()
            .filter(|w| w.len() > 2)
            .take(3)
            .collect();
        if !words.is_empty() {
            branch = format!("feat/{}", words.join("-").to_ascii_lowercase());
        }
    }
    proto(WriteGitBranchNameResponse {
        branch_name: branch,
    })
}

pub async fn get_full_self_driving_config() -> Result<Response<Body>> {
    proto(GetFullSelfDrivingConfigResponse {
        has_config: true,
        enabled: true,
    })
}

pub async fn server_time() -> Result<Response<Body>> {
    let now = chrono::Utc::now().timestamp_millis() as f64 / 1000.0;
    proto(ServerTimeResponse {
        receive_timestamp: now,
        transmit_timestamp: now,
    })
}

pub async fn list_marketplaces(
    State(_registry): State<TransportRegistry>,
    Extension(upstream): Extension<proxy::CursorProxy>,
    request: Request<Body>,
) -> Result<Response<Body>> {
    // Marketplace catalogs must always reach official upstream for real accounts!
    if local_app::request_uses_local_cursor_token(request.headers()) {
        consume_body(request).await?;
        return proto(ListMarketplacesResponse {
            marketplaces: Vec::new(),
        });
    }
    proxy::forward(Extension(upstream), request).await
}

pub async fn list_marketplace_plugins(
    State(_registry): State<TransportRegistry>,
    Extension(upstream): Extension<proxy::CursorProxy>,
    request: Request<Body>,
) -> Result<Response<Body>> {
    // Marketplace catalogs must always reach official upstream for real accounts!
    if local_app::request_uses_local_cursor_token(request.headers()) {
        consume_body(request).await?;
        return proto(ListMarketplacePluginsResponse {
            plugins: Vec::new(),
            next_page_token: None,
            has_more: false,
        });
    }
    proxy::forward(Extension(upstream), request).await
}

async fn local_or_takeover_or_forward(
    _registry: &TransportRegistry,
    upstream: proxy::CursorProxy,
    request: Request<Body>,
    local: impl FnOnce() -> Result<Response<Body>>,
) -> Result<Response<Body>> {
    // Enabling model routing must not replace an official account's cloud services.
    if local_app::request_uses_local_cursor_token(request.headers()) {
        consume_body(request).await?;
        return local();
    }
    proxy::forward(Extension(upstream), request).await
}

fn local_stripe_profile(origin: Option<HeaderValue>) -> Result<Response<Body>> {
    let mut response = json(ultra_profile())?;
    if let Some(origin) = origin {
        response
            .headers_mut()
            .insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin);
        response.headers_mut().insert(
            header::ACCESS_CONTROL_ALLOW_CREDENTIALS,
            HeaderValue::from_static("true"),
        );
        response
            .headers_mut()
            .insert(header::VARY, HeaderValue::from_static("Origin"));
    }
    Ok(response)
}

async fn consume_body(request: Request<Body>) -> Result<()> {
    to_bytes(request.into_body(), usize::MAX)
        .await
        .map_err(|error| crate::Error::Protocol(format!("cannot read request body: {error}")))?;
    Ok(())
}

fn proto(message: impl Message) -> Result<Response<Body>> {
    response("application/proto", message.encode_to_vec())
}

fn json(value: Value) -> Result<Response<Body>> {
    response("application/json", serde_json::to_vec(&value)?)
}

fn response(content_type: &'static str, body: Vec<u8>) -> Result<Response<Body>> {
    let length = body.len();
    let mut response = Response::new(Body::from(body));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        axum::http::HeaderValue::from_static(content_type),
    );
    response.headers_mut().insert(
        header::CONTENT_LENGTH,
        length
            .to_string()
            .parse()
            .expect("body length is always a valid header value"),
    );
    Ok(response)
}

fn ultra_profile() -> Value {
    serde_json::json!({
        "membershipType": "ultra",
        "individualMembershipType": "ultra",
        "subscriptionStatus": "active",
        "lastPaymentFailed": false,
        "pendingCancellationDate": null,
        "daysRemainingOnTrial": 0,
        "paymentId": LOCAL_AUTH_ID,
        "isTeamMember": false
    })
}

#[cfg(test)]
mod tests {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };

    use axum::body::Bytes;
    use futures_util::stream;

    use super::*;

    #[tokio::test]
    async fn local_account_response_consumes_request_body_before_replying() {
        let polled = Arc::new(AtomicBool::new(false));
        let observed = polled.clone();
        let body = Body::from_stream(stream::once(async move {
            observed.store(true, Ordering::SeqCst);
            Ok::<_, std::convert::Infallible>(Bytes::from_static(b"request"))
        }));

        consume_body(Request::new(body)).await.unwrap();

        assert!(polled.load(Ordering::SeqCst));
    }

    #[test]
    fn local_stripe_profile_allows_the_cursor_app_origin() {
        let origin = HeaderValue::from_static("vscode-file://vscode-app");
        let response = local_stripe_profile(Some(origin.clone())).unwrap();
        assert_eq!(
            response.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN),
            Some(&origin)
        );
        assert_eq!(
            response
                .headers()
                .get(header::ACCESS_CONTROL_ALLOW_CREDENTIALS),
            Some(&HeaderValue::from_static("true"))
        );
        assert_eq!(
            response.headers().get(header::VARY),
            Some(&HeaderValue::from_static("Origin"))
        );
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE),
            Some(&HeaderValue::from_static("application/json"))
        );
    }
}
