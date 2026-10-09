//! Exercises cloud-account routing through the real HTTP router with a loopback upstream.
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use axum::{
    body::{to_bytes, Body, Bytes},
    http::{header, Request, Response, StatusCode},
    Router,
};
use tower::ServiceExt;

use super::*;
use crate::{
    cursor::prompting::{PromptAssets, PromptCompiler},
    model::ModelInvocation,
    network::NetworkClients,
    provider::{Provider, ProviderStream},
    store::Store,
};

struct UnusedProvider;

impl Provider for UnusedProvider {
    fn stream(&self, _: ModelInvocation, _: tokio_util::sync::CancellationToken) -> ProviderStream {
        panic!("account services must not invoke a model")
    }
}

async fn test_router(upstream: String) -> (tempfile::TempDir, Router) {
    test_router_with_takeover(upstream, true).await
}

async fn test_router_with_takeover(upstream: String, enabled: bool) -> (tempfile::TempDir, Router) {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&format!(
        "sqlite://{}",
        directory.path().join("test.db").display()
    ))
    .await
    .unwrap();
    store.set_cursor_takeover_enabled(enabled).await.unwrap();
    let clients = NetworkClients::new(store.clone());
    let assets =
        PromptAssets::load(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("prompt/cursor"))
            .unwrap();
    let registry =
        TransportRegistry::new(store, Arc::new(UnusedProvider), PromptCompiler::new(assets));
    let knowledge = knowledge::KnowledgeService::with_root(directory.path().join("rules")).unwrap();
    let router = router_with_proxy(
        registry,
        CursorProxy::for_test(clients, upstream),
        knowledge,
    );
    (directory, router)
}

#[tokio::test]
async fn local_controls_do_not_create_runs_and_reject_cross_site_or_missing_targets() {
    let (_root, router) = test_router("http://127.0.0.1:0".into()).await;
    let response = router
        .clone()
        .oneshot(
            Request::get("/__byok-api__/api/local-runs")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        to_bytes(response.into_body(), 1024).await.unwrap().as_ref(),
        b"[]"
    );
    for origin in ["https://untrusted.example", "http://localhost"] {
        let response = router
            .clone()
            .oneshot(
                Request::post("/__byok-api__/api/local-runs/missing/control")
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::ORIGIN, origin)
                    .body(Body::from(
                        r#"{"run_id":"stale","action":"stop_tool","tool_call_id":"child"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(!response.status().is_success());
        let bytes = to_bytes(response.into_body(), 4096).await.unwrap();
        let text = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(
            text.contains(if origin.contains("untrusted") {
                "local control origins"
            } else {
                "missing"
            }),
            "{text}"
        );
    }
}

#[tokio::test]
async fn image_rpc_routes_byok_to_image_provider_without_creating_agent_runs() {
    use crate::cursor::services::image::{
        image_response::Outcome, ImageRequest, ImageResponse, ReferenceImage,
    };
    use crate::plugin::{PluginRegistry, ResourceRecord, ResourceState};
    use base64::{engine::general_purpose::STANDARD, Engine};
    use prost::Message;
    let root = tempfile::tempdir().unwrap();
    let store = Store::connect("sqlite::memory:").await.unwrap();
    store.set_cursor_takeover_enabled(true).await.unwrap();
    let plugins = PluginRegistry::for_test(store.clone(), root.path());
    plugins.enable_test_image_model().await;
    let now = crate::store::now_ms();
    plugins.restore_resources("dev.nexusor.plugins.antigravity-auth","google-account",vec![ResourceRecord {
        id:"fixture".into(),key:"fixture".into(),state:ResourceState::Ready,
        private_data:serde_json::json!({"accessToken":"fixture","projectId":"fixture","expiresAtMs":now+3_600_000}),
        created_at_ms:now,updated_at_ms:now,
    }]).await.unwrap();
    let mut png = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(2, 2)
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    let expected = png.into_inner();
    let encoded = STANDARD.encode(&expected);
    let count = Arc::new(AtomicUsize::new(0));
    let observed = count.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let provider=Router::new().fallback(move |axum::Json(body):axum::Json<serde_json::Value>| {
        let encoded=encoded.clone();let count=observed.clone();
        async move {
            count.fetch_add(1,Ordering::SeqCst);
            assert_eq!(body["model"],"gemini-3.1-flash-image");
            assert_eq!(body["request"]["contents"][0]["parts"][1]["inlineData"]["data"],encoded);
            let reply=serde_json::json!({"response":{"candidates":[{"content":{"parts":[{"inlineData":{"data":encoded,"mimeType":"image/png"}}]}}]}});
            ([("content-type","text/event-stream")],format!("data: {reply}\n\n"))
        }
    });
    let server = tokio::spawn(async move { axum::serve(listener, provider).await.unwrap() });
    plugins
        .set_antigravity_test_urls(vec![format!("http://{address}")])
        .await;
    let clients = NetworkClients::new(store.clone());
    let assets =
        PromptAssets::load(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("prompt/cursor"))
            .unwrap();
    let registry = TransportRegistry::with_plugins(
        store.clone(),
        Arc::new(UnusedProvider),
        PromptCompiler::new(assets),
        Default::default(),
        plugins,
        root.path().join("rules"),
    );
    let router = router_with_proxy(
        registry,
        CursorProxy::for_test(clients, "http://127.0.0.1:0".into()),
        knowledge::KnowledgeService::with_root(root.path().join("rules")).unwrap(),
    );
    let mut input = ImageRequest {
        description: "fixture".into(),
        model_id: "plugin:fixture/chat-model".into(),
        reference_images: vec![ReferenceImage {
            data: STANDARD.encode(&expected),
            mime_type: "image/png".into(),
        }],
        max_mode: true,
        aspect_ratio: Some("1:1".into()),
    };
    for invalid in [false, true] {
        if invalid {
            input.reference_images[0].mime_type = "image/jpeg".into();
        }
        let response = router
            .clone()
            .oneshot(
                Request::post("/aiserver.v1.AiService/RunGenerateImage")
                    .header(header::CONTENT_TYPE, "application/proto")
                    .body(Body::from(input.encode_to_vec()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let output =
            ImageResponse::decode(to_bytes(response.into_body(), 1024 * 1024).await.unwrap())
                .unwrap();
        match output.result.unwrap() {
            Outcome::Success(value) => {
                assert!(!invalid);
                assert_eq!(value.mime_type, "image/png");
                assert_eq!(STANDARD.decode(value.image_data).unwrap(), expected);
            }
            Outcome::Error(value) => {
                assert!(invalid);
                assert!(value.error.contains("MIME type"));
            }
        }
    }
    assert_eq!(
        count.load(Ordering::SeqCst),
        1,
        "invalid references must not reach provider"
    );
    let runs: i64 = sqlx::query_scalar("SELECT count(*) FROM runs")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(runs, 0);
    server.abort();
}

#[tokio::test]
async fn image_rpc_is_forwarded_without_starting_an_agent_or_changing_its_wire_payload() {
    use prost::Message;
    #[derive(Clone, PartialEq, Message)]
    struct ImageRequest {
        #[prost(string, tag = "1")]
        description: String,
        #[prost(string, tag = "3")]
        model_id: String,
        #[prost(bool, tag = "4")]
        max_mode: bool,
        #[prost(string, optional, tag = "5")]
        aspect_ratio: Option<String>,
    }
    let request_bytes = ImageRequest {
        description: "local fixture, no generation".into(),
        model_id: "cursor-official-image".into(),
        max_mode: true,
        aspect_ratio: Some("1:1".into()),
    }
    .encode_to_vec();
    let expected = request_bytes.clone();
    let count = Arc::new(AtomicUsize::new(0));
    let observed = count.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let upstream = Router::new().fallback(move |request: Request<Body>| {
        let expected = expected.clone();
        let observed = observed.clone();
        async move {
            assert_eq!(
                request.uri().path(),
                "/aiserver.v1.AiService/RunGenerateImage"
            );
            assert_eq!(
                request.headers()[header::AUTHORIZATION],
                "Bearer fixture-token"
            );
            let status: u16 = request.headers()["x-test-status"]
                .to_str()
                .unwrap()
                .parse()
                .unwrap();
            assert_eq!(
                to_bytes(request.into_body(), 4096).await.unwrap().as_ref(),
                expected
            );
            observed.fetch_add(1, Ordering::SeqCst);
            // success/image_data+mime_type or error/error, per installed Cursor schema.
            let body: &[u8] = if status == 200 {
                b"\x0a\x11\x0a\x04AAAA\x12\x09image/png"
            } else {
                b"\x12\x07\x0a\x05limit"
            };
            Response::builder()
                .status(status)
                .header(header::CONTENT_TYPE, "application/proto")
                .body(Body::from(body))
                .unwrap()
        }
    });
    let server = tokio::spawn(async move { axum::serve(listener, upstream).await.unwrap() });
    for takeover in [false, true] {
        let (_root, router) =
            test_router_with_takeover(format!("http://{address}"), takeover).await;
        for status in [200, 429, 503] {
            let response = router
                .clone()
                .oneshot(
                    Request::post("/aiserver.v1.AiService/RunGenerateImage")
                        .header(header::AUTHORIZATION, "Bearer fixture-token")
                        .header(header::CONTENT_TYPE, "application/proto")
                        .header("x-test-status", status.to_string())
                        .body(Body::from(request_bytes.clone()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status().as_u16(), status);
            assert_eq!(
                response.headers()[header::CONTENT_TYPE],
                "application/proto"
            );
            let expected: &[u8] = if status == 200 {
                b"\x0a\x11\x0a\x04AAAA\x12\x09image/png"
            } else {
                b"\x12\x07\x0a\x05limit"
            };
            assert_eq!(
                to_bytes(response.into_body(), 4096).await.unwrap().as_ref(),
                expected
            );
        }
    }
    assert_eq!(count.load(Ordering::SeqCst), 6);
    server.abort();
}

#[tokio::test]
async fn local_tab_context_config_preserves_unknown_fields_and_upstream_errors() {
    use crate::cursor::protocol::proto::tab::ConfigResponse;
    use prost::Message;
    let upstream = Router::new().fallback(|request: Request<Body>| async move {
        let status = if request.headers().contains_key("x-fail") {
            StatusCode::FORBIDDEN
        } else {
            StatusCode::OK
        };
        Response::builder()
            .status(status)
            .header(header::CONTENT_TYPE, "application/proto")
            .body(Body::from(vec![
                0x08, 0x3c, 0x50, 0, 0xb0, 1, 0, 0xa0, 6, 1,
            ]))
            .unwrap()
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move { axum::serve(listener, upstream).await.unwrap() });
    let (directory, router) = test_router(format!("http://{address}")).await;
    let store = Store::connect(&format!(
        "sqlite://{}",
        directory.path().join("test.db").display()
    ))
    .await
    .unwrap();
    for enabled in [false, true] {
        store
            .set_cmdk_settings(crate::store::CmdKSettings {
                tab_model_id: if enabled {
                    "plugin:test/tab".into()
                } else {
                    String::new()
                },
                ..Default::default()
            })
            .await
            .unwrap();
        for fail in [false, true] {
            let mut request = Request::post("/aiserver.v1.AiService/CppConfig");
            if fail {
                request = request.header("x-fail", "1");
            }
            let response = router
                .clone()
                .oneshot(request.body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                if fail {
                    StatusCode::FORBIDDEN
                } else {
                    StatusCode::OK
                }
            );
            let body = to_bytes(response.into_body(), 1024).await.unwrap();
            assert!(body.starts_with(&[0x08, 0x3c, 0x50, 0, 0xb0, 1, 0, 0xa0, 6, 1]));
            let config = ConfigResponse::decode(body).unwrap();
            assert_eq!(config.above_radius, Some(60));
            assert_eq!(config.enable_rvf_tracking, enabled && !fail);
            assert_eq!(config.should_fetch_rvf_text, enabled && !fail);
        }
    }
    task.abort();
}

#[tokio::test]
async fn online_services_preserve_upstream_responses_with_integration_enabled() {
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let upstream = Router::new().fallback(move |request: Request<Body>| {
        let calls = observed.clone();
        async move {
            calls.fetch_add(1, Ordering::SeqCst);
            assert_eq!(
                request.headers()[header::AUTHORIZATION],
                "Bearer official-test-token"
            );
            assert_eq!(request.headers()[header::COOKIE], "session=test-cookie");
            assert_eq!(request.uri().query(), Some("cursor_version=3.22.12"));
            let status = StatusCode::from_u16(
                request.headers()["x-test-status"]
                    .to_str()
                    .unwrap()
                    .parse()
                    .unwrap(),
            )
            .unwrap();
            let expected = if request.method() == axum::http::Method::GET {
                Bytes::new()
            } else {
                Bytes::from_static(b"\x0a\x04test")
            };
            let body = to_bytes(request.into_body(), 1024).await.unwrap();
            assert_eq!(body, expected);
            Response::builder()
                .status(status)
                .header(header::CONTENT_TYPE, "application/proto")
                .header("x-upstream-marker", "preserved")
                .body(Body::from(Bytes::from_static(b"\x0a\x05cloud\xa0\x06\x01")))
                .unwrap()
        }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move { axum::serve(listener, upstream).await.unwrap() });
    let (_directory, router) = test_router(format!("http://{address}")).await;
    let routes = [
        "/aiserver.v1.AuthService/GetUserMeta",
        "/aiserver.v1.DashboardService/GetMe",
        "/aiserver.v1.DashboardService/GetTeams",
        "/aiserver.v1.DashboardService/GetPlanInfo",
        "/aiserver.v1.DashboardService/GetCurrentPeriodUsage",
        "/aiserver.v1.DashboardService/GetUsageLimitStatusAndActiveGrants",
        "/aiserver.v1.DashboardService/GetAvailableMcpServers",
        "/aiserver.v1.DashboardService/CheckHttpMcpStatus",
        "/aiserver.v1.MCPRegistryService/GetKnownServers",
        "/aiserver.v1.DashboardService/ListMarketplaces",
        "/aiserver.v1.DashboardService/ListMarketplacePlugins",
        "/aiserver.v1.DashboardService/GetManagedSkills",
        "/aiserver.v1.DashboardService/GetEffectiveUserPlugins",
        "/aiserver.v1.DashboardService/GetUserPrivacyMode",
        "/aiserver.v1.AiService/AvailableDocs",
        "/aiserver.v1.AiService/StreamTerminalAutocomplete",
        "/aiserver.v1.AiService/GetServerConfig",
        "/aiserver.v1.ServerConfigService/GetServerConfig",
        "/aiserver.v1.AnalyticsService/BootstrapStatsig",
        "/aiserver.v1.AiService/CheckFeatureStatus",
        "/aiserver.v1.AiService/CheckFeaturesStatus",
        "/aiserver.v1.DashboardService/GetGlobalCommands",
        "/aiserver.v1.DashboardService/GetEffectiveUserAgentStoreSkillsSettings",
        "/aiserver.v1.BackgroundComposerService/ListEnvironments",
        "/auth/stripe_profile",
        "/auth/full_stripe_profile",
        "/auth/has_valid_payment_method",
    ];
    for route in routes {
        for status in [200, 401, 403, 503] {
            let request = Request::builder()
                .method(if route.starts_with("/auth/") {
                    "GET"
                } else {
                    "POST"
                })
                .uri(format!("{route}?cursor_version=3.22.12"))
                .header(header::AUTHORIZATION, "Bearer official-test-token")
                .header(header::COOKIE, "session=test-cookie")
                .header(header::CONTENT_TYPE, "application/proto")
                .header("x-test-status", status.to_string())
                .body(if route.starts_with("/auth/") {
                    Body::empty()
                } else {
                    Body::from(Bytes::from_static(b"\x0a\x04test"))
                })
                .unwrap();
            let response = router.clone().oneshot(request).await.unwrap();
            assert_eq!(response.status().as_u16(), status, "{route}");
            assert_eq!(
                response.headers()["x-upstream-marker"],
                "preserved",
                "{route}"
            );
            let expected: &[u8] = if status == 200 && route.ends_with("/GetServerConfig") {
                b"\x0a\x05cloud\xa0\x06\x01\x38\x01"
            } else {
                b"\x0a\x05cloud\xa0\x06\x01"
            };
            assert_eq!(
                to_bytes(response.into_body(), 1024).await.unwrap().as_ref(),
                expected,
                "{route}"
            );
        }
    }
    assert_eq!(calls.load(Ordering::SeqCst), routes.len() * 4);
    task.abort();
}

#[tokio::test]
async fn local_account_services_work_without_upstream_and_cookie_sessions_forward() {
    let (_directory, router) = test_router("http://127.0.0.1:0".into()).await;
    let token = crate::local_app::local_cursor_authorization();
    let mut headers = HeaderMap::new();
    headers.insert(header::AUTHORIZATION, token.parse().unwrap());
    assert!(crate::local_app::request_uses_local_cursor_token(&headers));
    for route in [
        "/aiserver.v1.DashboardService/GetAvailableMcpServers",
        "/aiserver.v1.DashboardService/GetEffectiveUserPlugins",
        "/aiserver.v1.AnalyticsService/BootstrapStatsig",
        "/aiserver.v1.AiService/GetServerConfig",
        "/auth/stripe_profile",
    ] {
        let request = Request::builder()
            .method(if route.starts_with("/auth/") {
                "GET"
            } else {
                "POST"
            })
            .uri(route)
            .header(header::AUTHORIZATION, &token)
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            router.clone().oneshot(request).await.unwrap().status(),
            StatusCode::OK,
            "{route}"
        );
    }
    // Missing or cookie-only authentication must not silently become a local account.
    for cookie in [None, Some("session=official-cookie")] {
        let mut request = Request::builder().uri("/auth/stripe_profile");
        if let Some(cookie) = cookie {
            request = request.header(header::COOKIE, cookie);
        }
        let response = router
            .clone()
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert!(!response.status().is_success());
    }
}

#[tokio::test]
async fn transport_overrides_preserve_cloud_config_and_follow_takeover_setting() {
    use prost::Message;
    #[derive(Clone, PartialEq, Message)]
    struct Statsig {
        #[prost(string, tag = "1")]
        config: String,
        #[prost(uint64, tag = "2")]
        timestamp: u64,
    }
    #[derive(Clone, PartialEq, Message)]
    struct Config {
        #[prost(string, tag = "6")]
        version: String,
        #[prost(int32, tag = "7")]
        transport: i32,
    }
    let original_json = serde_json::json!({
        "hash_used": "none",
        "feature_gates": {
            "nal_websocket_client": {"value": true, "rule_id": "official"},
            "mcp_tools_enabled": {"value": true}
        },
        "user": {"userID": "official-user"},
        "dynamic_configs": {"cloud": {"value": 42}}
    });
    let mut statsig = Statsig {
        config: original_json.to_string(),
        timestamp: 123,
    }
    .encode_to_vec();
    let mut config = Config {
        version: "official".into(),
        transport: 3,
    }
    .encode_to_vec();
    // Unknown field 100 must survive both transformations byte-for-byte.
    statsig.extend_from_slice(b"\xa0\x06\x01");
    config.extend_from_slice(b"\xa0\x06\x01");
    let originals = (statsig.clone(), config.clone());
    let upstream = Router::new().fallback(move |request: Request<Body>| {
        let body = if request.uri().path().ends_with("BootstrapStatsig") {
            statsig.clone()
        } else {
            config.clone()
        };
        async move {
            Response::builder()
                .header(header::CONTENT_TYPE, "application/proto")
                .header("x-upstream-marker", "preserved")
                .body(Body::from(body))
                .unwrap()
        }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move { axum::serve(listener, upstream).await.unwrap() });
    for enabled in [false, true] {
        let (_directory, router) =
            test_router_with_takeover(format!("http://{address}"), enabled).await;
        for (path, original) in [
            (
                "/aiserver.v1.AnalyticsService/BootstrapStatsig",
                &originals.0,
            ),
            ("/aiserver.v1.AiService/GetServerConfig", &originals.1),
            (
                "/aiserver.v1.ServerConfigService/GetServerConfig",
                &originals.1,
            ),
        ] {
            let response = router
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(path)
                        .header(header::AUTHORIZATION, "Bearer official-test-token")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(response.headers()["x-upstream-marker"], "preserved");
            let length = response.headers().get(header::CONTENT_LENGTH).cloned();
            let body = to_bytes(response.into_body(), 16384).await.unwrap();
            if let Some(length) = length {
                assert_eq!(
                    length.to_str().unwrap().parse::<usize>().unwrap(),
                    body.len()
                );
            }
            assert!(body.starts_with(original));
            if !enabled {
                assert_eq!(body.as_ref(), original);
            }
            if path.ends_with("BootstrapStatsig") {
                let decoded = Statsig::decode(body).unwrap();
                assert_eq!(decoded.timestamp, 123);
                let mut expected = original_json.clone();
                if enabled {
                    expected["feature_gates"]["nal_websocket_client"]["value"] = false.into();
                }
                assert_eq!(
                    serde_json::from_str::<serde_json::Value>(&decoded.config).unwrap(),
                    expected
                );
            } else {
                let decoded = Config::decode(body).unwrap();
                assert_eq!(decoded.version, "official");
                assert_eq!(decoded.transport, if enabled { 1 } else { 3 });
            }
        }
    }
    task.abort();
}
