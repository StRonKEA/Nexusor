//! Configures the local application proxy.
use std::{net::SocketAddr, sync::Arc};

use hudsucker::{
    certificate_authority::RcgenAuthority,
    hyper::{header, http::StatusCode, Method, Request, Response, Uri},
    rustls::crypto::aws_lc_rs,
    Body, HttpContext, HttpHandler, Proxy, RequestOrResponse,
};
use tokio::{net::TcpListener, sync::oneshot, task::JoinHandle};

use parking_lot::RwLock;

use crate::{
    api::cursor::proxy::UPSTREAM_URL_HEADER, cursor::services::tab::is_tab_path, store::TabMode,
    Error, Result,
};

use super::ca::LoadedCa;

#[derive(Default)]
pub struct ProxyRuntime {
    url: Option<String>,
    port: Option<u16>,
    stop: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<()>>,
}

impl ProxyRuntime {
    pub fn running(&self) -> bool {
        self.task.as_ref().is_some_and(|task| !task.is_finished())
    }
    pub fn url(&self) -> Option<String> {
        self.running().then(|| self.url.clone()).flatten()
    }
    pub fn port(&self) -> Option<u16> {
        if self.running() {
            self.port
        } else {
            None
        }
    }

    pub async fn start(
        &mut self,
        backend: SocketAddr,
        ca: LoadedCa,
        requested_port: u16,
        tab_mode: Arc<RwLock<TabMode>>,
    ) -> Result<(String, u16)> {
        if let Some(url) = self.url() {
            return Ok((url, self.port.unwrap_or_default()));
        }
        let listener = bind_proxy_listener(requested_port).await?;
        let address = listener.local_addr()?;
        let (stop, done) = oneshot::channel();
        let authority = RcgenAuthority::new(ca.issuer, 1_000, aws_lc_rs::default_provider());
        let proxy = Proxy::builder()
            .with_listener(listener)
            .with_ca(authority)
            .with_rustls_connector(aws_lc_rs::default_provider())
            .with_http_handler(CursorRelay { backend, tab_mode })
            .with_graceful_shutdown(async move {
                let _ = done.await;
            })
            .build()
            .map_err(|error| Error::Store(format!("build Cursor proxy: {error}")))?;
        let url = format!("http://{address}");
        self.stop = Some(stop);
        self.url = Some(url.clone());
        self.port = Some(address.port());
        self.task = Some(tokio::spawn(async move {
            if let Err(error) = proxy.start().await {
                tracing::error!(%error, "Cursor proxy stopped unexpectedly");
            }
        }));
        Ok((url, address.port()))
    }

    pub async fn stop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(task) = self.task.take() {
            let _ = tokio::time::timeout(std::time::Duration::from_secs(5), task).await;
        }
        self.url = None;
        self.port = None;
    }
}

async fn bind_proxy_listener(requested_port: u16) -> Result<TcpListener> {
    let requested = SocketAddr::from(([127, 0, 0, 1], requested_port));
    match TcpListener::bind(requested).await {
        Ok(listener) => Ok(listener),
        Err(error) if requested_port != 0 => {
            tracing::warn!(%requested, %error, "configured proxy port unavailable; selecting a random port");
            Ok(TcpListener::bind("127.0.0.1:0").await?)
        }
        Err(error) => Err(error.into()),
    }
}

#[derive(Clone)]
struct CursorRelay {
    backend: SocketAddr,
    tab_mode: Arc<RwLock<TabMode>>,
}

impl HttpHandler for CursorRelay {
    async fn handle_request(
        &mut self,
        _ctx: &HttpContext,
        mut request: Request<Body>,
    ) -> RequestOrResponse {
        let original = request.uri().clone();
        // CONNECT is inspected only to decide whether TLS MITM is enabled.
        // Rewriting its authority would prevent interception.
        if request.method() == Method::CONNECT {
            return request.into();
        }
        let host = original
            .host()
            .or_else(|| {
                request
                    .headers()
                    .get(header::HOST)
                    .and_then(|h| h.to_str().ok())
                    .and_then(|h| h.split(':').next())
            })
            .unwrap_or_default();
        let local_account = crate::local_app::request_uses_local_cursor_token(request.headers());
        if let Some(status) = is_cursor_host(host)
            .then(|| local_stub_status(original.path(), local_account))
            .flatten()
        {
            let mut response = Response::new(Body::empty());
            *response.status_mut() = status;
            response.headers_mut().insert(
                header::CONTENT_LENGTH,
                header::HeaderValue::from_static("0"),
            );
            if original.path().starts_with("/aiserver.v1.")
                || original.path().starts_with("/agent.v1.")
            {
                response.headers_mut().insert(
                    header::CONTENT_TYPE,
                    header::HeaderValue::from_static("application/proto"),
                );
            }
            return RequestOrResponse::Response(response);
        }
        let route_locally = is_cursor_host(host)
            && (should_route_locally(original.path(), *self.tab_mode.read())
                || !is_tab_path(original.path()));
        if route_locally {
            if let Ok(value) = original.to_string().parse() {
                request.headers_mut().insert(UPSTREAM_URL_HEADER, value);
            }
            let path = original
                .path_and_query()
                .map(|value| value.as_str())
                .unwrap_or("/");
            if let Ok(uri) = format!("http://{}{}", self.backend, path).parse::<Uri>() {
                *request.uri_mut() = uri;
            }
        }
        request.into()
    }

    async fn should_intercept_connect(
        &mut self,
        _ctx: &HttpContext,
        request: &Request<Body>,
    ) -> bool {
        request
            .uri()
            .authority()
            .is_some_and(|authority| is_cursor_host(authority.host()))
    }

    async fn should_intercept_tls(
        &mut self,
        _ctx: &HttpContext,
        hello: hudsucker::rustls::server::ClientHello<'_>,
    ) -> bool {
        hello.server_name().is_some_and(is_cursor_host)
    }
}

pub fn is_cursor_host(host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    matches!(
        host.as_str(),
        "api2.cursor.sh" | "api3.cursor.sh" | "api4.cursor.sh"
    ) || host.ends_with(".cursor.sh")
        || host.ends_with(".cursorapi.com")
        || host == "cursorapi.com"
}

fn local_stub_status(path: &str, local_account: bool) -> Option<StatusCode> {
    // Sentry, Datadog and Telemetry endpoints should be blocked locally to prevent data leaks.
    if path.contains("/envelope/") || (path.starts_with("/api/") && path.contains("sentry")) {
        return Some(StatusCode::OK);
    }
    if path.starts_with("/tev1/") {
        return Some(StatusCode::ACCEPTED);
    }
    if (path.starts_with("/aiserver.v1.AnalyticsService/")
        && path != "/aiserver.v1.AnalyticsService/BootstrapStatsig")
        || path == "/aiserver.v1.AiService/ReportClientNumericMetrics"
        || path == "/aiserver.v1.AiService/ReportAiCodeChangeMetrics"
        || path.starts_with("/aiserver.v1.CiMetricsService/")
        || path.starts_with("/aiserver.v1.ClientLoggerService/")
        || path.starts_with("/aiserver.v1.PerformanceEventService/")
        || path.starts_with("/aiserver.v1.MetricsService/")
    {
        return Some(StatusCode::OK);
    }

    // Official sessions need real service responses, including feature discovery.
    if !local_account {
        return None;
    }
    let status = match path {
        "/agent/v1/run" | "/ws-reachability-probe" => StatusCode::NOT_FOUND,
        "/agent.v1.AgentService/ListLocalSubscriptionTools"
        | "/aiserver.v1.DashboardService/GetTeamCommands"
        | "/aiserver.v1.DashboardService/GetGlobalCommands"
        | "/aiserver.v1.DashboardService/GetTeamAdminSettingsOrEmptyIfNotInTeam"
        | "/aiserver.v1.DashboardService/GetEffectiveUserAgentStoreSkillsSettings"
        | "/aiserver.v1.DashboardService/GetSandAccessStatus"
        | "/aiserver.v1.DashboardService/GetP2PReferralStatus"
        | "/aiserver.v1.BackgroundComposerService/GetBackgroundComposerUserSettings"
        | "/aiserver.v1.OnboardingChecklistService/ListOnboardingChecklistItems"
        | "/aiserver.v1.DashboardService/GetTeamReposOrEmptyIfNotInTeam"
        | "/aiserver.v1.DashboardService/GetRepoSourcePreference"
        | "/aiserver.v1.DashboardService/GetGithubInstallations"
        | "/aiserver.v1.DashboardService/ListGithubEnterpriseApps"
        | "/aiserver.v1.DashboardService/ListGitlabEnterpriseInstances"
        | "/aiserver.v1.AutomationsService/ListWorkflowTemplates"
        | "/aiserver.v1.AutomationsService/ListAutomations"
        | "/aiserver.v1.DashboardService/ClientAction"
        | "/agent.v1.AgentService/GetNewChatNudgeParameterizedModelPicker" => StatusCode::OK,
        "/aiserver.v1.BackgroundComposerService/MintAgentStoreToken"
        | "/aiserver.v1.BackgroundComposerService/ListPrivateWorkers"
        | "/aiserver.v1.BackgroundComposerService/ListEnvironments"
        | "/aiserver.v1.BackgroundComposerService/ListBackgroundComposers" => {
            StatusCode::BAD_REQUEST
        }
        _ => return None,
    };
    Some(status)
}

fn is_local_path(path: &str) -> bool {
    matches!(
        path,
        "/agent.v1.AgentService/RunSSE"
            | "/aiserver.v1.CmdKService/StreamCmdK"
            | "/aiserver.v1.CmdKService/StreamTerminalCmdK"
            | "/aiserver.v1.AiService/StreamCpp"
            | "/aiserver.v1.BidiService/BidiAppend"
            | "/aiserver.v1.AiService/AvailableDocs"
            | "/aiserver.v1.DashboardService/GetEffectiveUserPlugins"
            | "/aiserver.v1.DashboardService/GetUserPrivacyMode"
            | "/agent.v1.AgentService/UpdateConversationMetadata"
            | "/aiserver.v1.AiService/GetServerConfig"
            | "/aiserver.v1.ServerConfigService/GetServerConfig"
            | "/aiserver.v1.AiService/AvailableModels"
            | "/agent.v1.AgentService/GetUsableModels"
            | "/aiserver.v1.AiService/GetUsableModels"
            | "/agent.v1.AgentService/GetDefaultModelForCli"
            | "/aiserver.v1.AiService/GetDefaultModelForCli"
            | "/aiserver.v1.AiService/GetDefaultModel"
            | "/aiserver.v1.AiService/GetDefaultModelNudgeData"
            | "/aiserver.v1.AuthService/GetEmail"
            | "/aiserver.v1.AuthService/GetUserMeta"
            | "/aiserver.v1.DashboardService/GetMe"
            | "/aiserver.v1.DashboardService/GetTeams"
            | "/aiserver.v1.DashboardService/GetUserProfile"
            | "/aiserver.v1.DashboardService/GetPlanInfo"
            | "/aiserver.v1.DashboardService/IsOnNewPricing"
            | "/aiserver.v1.DashboardService/GetCurrentPeriodUsage"
            | "/aiserver.v1.DashboardService/GetUsageLimitStatusAndActiveGrants"
            | "/aiserver.v1.DashboardService/GetManagedSkills"
            | "/aiserver.v1.DashboardService/GetAvailableMcpServers"
            | "/aiserver.v1.DashboardService/GetCliDownloadUrl"
            | "/aiserver.v1.DashboardService/CheckHttpMcpStatus"
            | "/aiserver.v1.AiService/NameTab"
            | "/aiserver.v1.AiService/ServerTime"
            | "/aiserver.v1.DashboardService/ListMarketplaces"
            | "/aiserver.v1.DashboardService/ListMarketplacePlugins"
            | "/aiserver.v1.MCPRegistryService/GetKnownServers"
            | "/aiserver.v1.AiService/KnowledgeBaseAdd"
            | "/aiserver.v1.AiService/KnowledgeBaseList"
            | "/aiserver.v1.AiService/KnowledgeBaseUpdate"
            | "/aiserver.v1.AiService/KnowledgeBaseRemove"
            | "/aiserver.v1.AiService/WriteGitCommitMessage"
            | "/aiserver.v1.AiService/WriteGitBranchName"
            | "/aiserver.v1.AiService/AutoContext"
            | "/aiserver.v1.AiService/ContextReranking"
            | "/aiserver.v1.AiService/CheckFeaturesStatus"
            | "/aiserver.v1.AiService/CheckFeatureStatus"
            | "/aiserver.v1.AiService/CheckFeatureStatusUnauthenticated"
            | "/aiserver.v1.AiService/WarmComposerCache"
            | "/aiserver.v1.AiService/KeepComposerCacheWarm"
            | "/aiserver.v1.AiService/CountTokens"
            | "/aiserver.v1.AiService/StreamTerminalAutocomplete"
            | "/aiserver.v1.NetworkService/IsConnected"
            | "/aiserver.v1.AnalyticsService/BootstrapStatsig"
            | "/aiserver.v1.FullSelfDrivingService/GetFullSelfDrivingConfig"
            | "/aiserver.v1.AiService/ShouldTurnOnCppOnboarding"
            | "/aiserver.v1.GitGraphService/IsGitGraphEnabled"
            | "/aiserver.v1.GitGraphService/GetGitGraphStatus"
            | "/aiserver.v1.GitGraphService/GetGitGraphRelatedFiles"
            | "/agent.v1.AgentService/NameAgent"
            | "/agent.v1.AgentService/GetAllowedModelIntents"
            | "/aiserver.v1.LinterService/LintFile"
            | "/aiserver.v1.LinterService/LintChunk"
            | "/aiserver.v1.CursorPredictionService/CursorPredictionConfig"
            | "/aiserver.v1.FastApplyService/WarmApply"
            | "/aiserver.v1.FastApplyService/ReportEditFate"
            | "/aiserver.v1.LinterService/LintExplanation2"
            | "/aiserver.v1.ReviewService/BugConfig"
            | "/aiserver.v1.ReviewService/StreamReview"
            | "/aiserver.v1.ReviewService/StreamSlowReview"
            | "/aiserver.v1.AiService/StreamBugBotAgentic"
            | "/aiserver.v1.CmdKService/RerankCmdKContext"
            | "/aiserver.v1.CmdKService/RerankTerminalCmdKContext"
            | "/agent.v1.AgentService/CreateTranscriptOverview"
            | "/auth/full_stripe_profile"
            | "/auth/stripe_profile"
            | "/auth/has_valid_payment_method"
    )
}

fn should_route_locally(path: &str, tab_mode: TabMode) -> bool {
    is_local_path(path) || (is_tab_path(path) && tab_mode != TabMode::Direct)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn official_services_are_never_stubbed() {
        for path in [
            "/aiserver.v1.DashboardService/GetGlobalCommands",
            "/aiserver.v1.DashboardService/GetTeamCommands",
            "/aiserver.v1.DashboardService/GetEffectiveUserAgentStoreSkillsSettings",
            "/aiserver.v1.BackgroundComposerService/MintAgentStoreToken",
            "/aiserver.v1.BackgroundComposerService/ListEnvironments",
            "/aiserver.v1.AutomationsService/ListAutomations",
            "/agent/v1/run",
        ] {
            assert_eq!(local_stub_status(path, false), None, "{path}");
            assert!(local_stub_status(path, true).is_some(), "{path}");
        }
    }

    #[test]
    fn cursor_cli_transport_and_model_metadata_routes_stay_local() {
        for path in [
            "/aiserver.v1.AiService/GetServerConfig",
            "/aiserver.v1.ServerConfigService/GetServerConfig",
            "/agent.v1.AgentService/GetDefaultModelForCli",
            "/aiserver.v1.AiService/GetDefaultModelForCli",
            "/aiserver.v1.AiService/GetDefaultModel",
            "/aiserver.v1.AiService/GetDefaultModelNudgeData",
            "/aiserver.v1.AiService/AvailableDocs",
            "/aiserver.v1.DashboardService/GetEffectiveUserPlugins",
            "/aiserver.v1.DashboardService/GetUserPrivacyMode",
            "/aiserver.v1.AuthService/GetUserMeta",
            "/agent.v1.AgentService/UpdateConversationMetadata",
            "/auth/full_stripe_profile",
            "/auth/stripe_profile",
        ] {
            assert!(is_local_path(path), "{path} must not reach Cursor upstream");
        }
    }
}
