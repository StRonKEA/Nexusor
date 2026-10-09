use super::*;

#[cfg(test)]
mod failover_tests {
    use super::PluginRegistry;
    use crate::plugin::state::{PoolStrategy, ResourceDraft, ResourceState};
    use crate::store::Store;

    #[tokio::test]
    async fn registry_http_quota_failover_completes_same_invocation_with_distinct_records() {
        use crate::{
            model::{
                ContentPart, ModelInvocation, ModelRequest, ModelSpec, NewLlmCall,
                ProjectedContent, ProjectedMessage, PromptSpec, ProviderType, Role,
            },
            plugin::ResourceRecord,
            provider::{CallRecorder, FinishReason, ModelEvent},
        };
        use axum::{
            http::{HeaderMap, StatusCode},
            response::IntoResponse,
            routing::post,
            Json, Router,
        };
        use futures_util::StreamExt;
        use std::sync::Arc;
        use tokio::sync::Mutex;
        let received = Arc::new(Mutex::new(Vec::new()));
        let seen = received.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(listener, Router::new().route("/", post(move |headers: HeaderMap, Json(body): Json<serde_json::Value>| {
                let seen = seen.clone();
                async move {
                    let auth = headers.get("authorization").unwrap().to_str().unwrap().to_owned();
                    seen.lock().await.push((auth.clone(), body));
                    match auth.as_str() {
                        "Bearer fixture-a" => (StatusCode::TOO_MANY_REQUESTS,
                            r#"{"error":{"status":"RESOURCE_EXHAUSTED","details":[{"reason":"QUOTA_EXHAUSTED"},{"@type":"type.googleapis.com/google.rpc.RetryInfo","retryDelay":"300s"}]}}"#).into_response(),
                        "Bearer fixture-b" => ([("content-type", "text/event-stream")],
                            "data: {\"response\":{\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"REGISTRY_FAILOVER_OK\"}]},\"finishReason\":\"STOP\"}]}}\n\n").into_response(),
                        _ => StatusCode::UNAUTHORIZED.into_response(),
                    }
                }
            }))).await.unwrap();
        });
        let root = tempfile::tempdir().unwrap();
        let store = Store::connect("sqlite::memory:").await.unwrap();
        store.set_detailed_logging(true).await.unwrap();
        let registry = PluginRegistry::for_test(store.clone(), root.path());
        *registry.inner.antigravity_test_urls.write().await = Some(vec![url.clone()]);
        let plugin = "dev.nexusor.plugins.antigravity-auth";
        let now = super::now_ms();
        registry.restore_resources(plugin, "google-account", ["a", "b"].into_iter().map(|id| ResourceRecord {
            id: id.into(), key: id.into(), state: ResourceState::Ready,
            private_data: serde_json::json!({"accessToken":format!("fixture-{id}"),"projectId":"fixture",
                "expiresAtMs":now+3_600_000,"quota":{"claude_weekly":{"remaining_percent":90,"reset_at_ms":now+600_000}}}),
            created_at_ms:now,updated_at_ms:now,
        }).collect()).await.unwrap();
        let model = "plugin:dev.nexusor.plugins.antigravity-auth/antigravity/claude-sonnet-4-6";
        let invocation = ModelInvocation {
            call_id: "registry-quota".into(),
            run_id: "single-run".into(),
            conversation_id: "single-conversation".into(),
            provider_call_index: 0,
            slot_account_ids: Some(vec!["a".into(), "b".into()]),
            slot_strategy: Some(PoolStrategy::Failover),
            request: ModelRequest {
                model: ModelSpec::new(model),
                prompt: PromptSpec {
                    instructions: String::new(),
                    tools: vec![],
                },
                history: vec![ProjectedMessage {
                    message_id: "user".into(),
                    role: Role::User,
                    content: ProjectedContent::Parts(vec![ContentPart::Text {
                        text: "return marker".into(),
                    }]),
                }],
            },
        };
        let recorder = CallRecorder::start(
            store.clone(),
            NewLlmCall {
                call_id: invocation.call_id.clone(),
                run_id: invocation.run_id.clone(),
                conversation_id: invocation.conversation_id.clone(),
                provider_call_index: 0,
                model_hash: model.into(),
                provider_type: ProviderType::OpenAiChat,
                provider_url: url.clone(),
                request_type: ProviderType::OpenAiChat,
                request_url: url,
                model_id: "claude-sonnet-4-6".into(),
                display_name: "Fixture".into(),
                reasoning_effort: None,
                fast: false,
                message_count: 1,
                tool_count: 0,
                detailed: true,
            },
        )
        .await
        .unwrap();
        let mut stream = registry.stream_model(
            invocation,
            tokio_util::sync::CancellationToken::new(),
            recorder.clone(),
        );
        let mut text = String::new();
        let mut done = false;
        while let Some(event) =
            tokio::time::timeout(std::time::Duration::from_secs(5), stream.next())
                .await
                .unwrap()
        {
            let event = event.expect("registry must recover without exposing first account error");
            recorder.event(&event).await.unwrap();
            match event {
                ModelEvent::TextDelta(value) => text.push_str(&value),
                ModelEvent::Done(FinishReason::Stop) => done = true,
                _ => {}
            }
        }
        assert!(done);
        assert_eq!(text, "REGISTRY_FAILOVER_OK");
        recorder.completed(FinishReason::Stop).await.unwrap();
        let requests = received.lock().await;
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].0, "Bearer fixture-a");
        assert_eq!(requests[1].0, "Bearer fixture-b");
        assert_eq!(
            requests[0].1["request"]["contents"],
            requests[1].1["request"]["contents"]
        );
        let rows: Vec<(String, String, String, i64)> = sqlx::query_as(
            "SELECT call_id,run_id,status,http_status FROM llm_calls ORDER BY call_id",
        )
        .fetch_all(store.pool())
        .await
        .unwrap();
        assert_eq!(
            rows,
            vec![
                (
                    "registry-quota".into(),
                    "single-run".into(),
                    "error".into(),
                    429
                ),
                (
                    "registry-quota:account-2".into(),
                    "single-run".into(),
                    "completed".into(),
                    200
                )
            ]
        );
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM llm_call_requests")
            .fetch_one(store.pool())
            .await
            .unwrap();
        assert_eq!(count, 2);
        let records = registry.resources(plugin, "google-account").await.unwrap();
        let first = records.iter().find(|r| r.id == "a").unwrap();
        assert!(matches!(first.state, ResourceState::Ready));
        assert!(
            first.private_data["modelFamilyCooldowns"]["claude"]
                .as_i64()
                .unwrap()
                >= now + 360_000
        );
        assert!(first.private_data["modelFamilyCooldowns"]["gemini"].is_null());
        assert!(
            records.iter().find(|r| r.id == "b").unwrap().private_data["modelFamilyCooldowns"]
                .is_null()
        );
        let reloaded = PluginRegistry::for_test(store, root.path());
        assert_eq!(
            reloaded.resources(plugin, "google-account").await.unwrap()[0].private_data,
            records[0].private_data
        );
        server.abort();
    }

    struct Fixture {
        _root: tempfile::TempDir,
        registry: PluginRegistry,
        plugin_id: &'static str,
    }

    impl Fixture {
        async fn new(plugin_id: &'static str) -> Self {
            let root = tempfile::tempdir().unwrap();
            let store = Store::connect("sqlite::memory:").await.unwrap();
            let registry = PluginRegistry::for_test(store, root.path());
            Self {
                _root: root,
                registry,
                plugin_id,
            }
        }

        async fn seed(&self, keys: &[&str]) {
            self.registry
                .inner
                .state
                .upsert_resources(
                    self.plugin_id,
                    "account",
                    keys.iter()
                        .map(|key| ResourceDraft {
                            key: (*key).into(),
                            private_data: serde_json::json!({}),
                            state: None,
                        })
                        .collect(),
                )
                .await
                .unwrap();
        }

        async fn cool(&self, resource_id: &str, secs: u64) {
            self.registry
                .cool_resource(
                    self.plugin_id,
                    "account",
                    resource_id,
                    std::time::Duration::from_secs(secs),
                    "upstream quota exhausted",
                )
                .await
                .unwrap();
        }

        async fn records(&self) -> Vec<crate::plugin::state::ResourceRecord> {
            self.registry
                .inner
                .state
                .resources(self.plugin_id, "account")
                .await
                .unwrap()
        }
    }

    #[tokio::test]
    async fn cooling_marks_only_the_failing_account() {
        let plugin_id: &'static str =
            Box::leak(format!("plugin-cool-only-{}", uuid::Uuid::new_v4()).into_boxed_str());
        let fx = Fixture::new(plugin_id).await;
        fx.seed(&["a", "b"]).await;
        let failing = fx
            .registry
            .select_resource(fx.plugin_id, "account", &[])
            .await
            .unwrap();
        fx.cool(&failing.id, 60).await;
        let records = fx.records().await;
        assert_eq!(records.len(), 2);
        for record in &records {
            if record.id == failing.id {
                assert!(
                    matches!(record.state, ResourceState::Cooling { .. }),
                    "failing account must be cooling"
                );
            } else {
                assert!(
                    matches!(record.state, ResourceState::Ready),
                    "healthy account must stay ready"
                );
            }
        }
        let next = fx
            .registry
            .select_resource(fx.plugin_id, "account", &[])
            .await
            .unwrap();
        assert_ne!(
            next.id, failing.id,
            "pool must fail over to the ready account"
        );
    }

    #[tokio::test]
    async fn antigravity_family_cooldown_persists_without_blocking_other_models() {
        use crate::plugin::ResourceRecord;
        use crate::provider::providers::quota::{
            antigravity_model_is_cooling, is_account_quota_error, quota_cooldown,
        };
        let fx = Fixture::new("dev.nexusor.plugins.antigravity-auth").await;
        let now = super::now_ms();
        let records = ["a", "b"]
            .into_iter()
            .map(|id| ResourceRecord {
                id: id.into(),
                key: id.into(),
                state: ResourceState::Ready,
                private_data: serde_json::json!({"quota": {
                    "gemini_weekly": {"remaining_percent": 0, "reset_at_ms": now + 600_000_000},
                    "claude": {"remaining_percent": 80, "reset_at_ms": now + 18_000_000}
                }}),
                created_at_ms: now,
                updated_at_ms: now,
            })
            .collect();
        fx.registry
            .restore_resources(fx.plugin_id, "google-account", records)
            .await
            .unwrap();
        let error = r#"HTTP 429 {"error":{"status":"RESOURCE_EXHAUSTED","details":[{"reason":"QUOTA_EXHAUSTED"},{"@type":"type.googleapis.com/google.rpc.RetryInfo","retryDelay":"300s"}]}}"#;
        assert!(is_account_quota_error("antigravity", error));
        fx.registry
            .cool_model_resource(
                fx.plugin_id,
                "google-account",
                "a",
                "claude-sonnet-4-6",
                quota_cooldown("antigravity", error),
            )
            .await
            .unwrap();
        let reloaded = PluginRegistry::for_test(
            Store::connect("sqlite::memory:").await.unwrap(),
            fx._root.path(),
        );
        let records = reloaded
            .resources(fx.plugin_id, "google-account")
            .await
            .unwrap();
        let first = &records[0];
        assert!(matches!(first.state, ResourceState::Ready));
        assert!(antigravity_model_is_cooling(
            &first.private_data,
            "claude-opus-4-6",
            now
        ));
        assert!(!antigravity_model_is_cooling(
            &first.private_data,
            "gemini-3.8-flash",
            now
        ));
        assert!(!antigravity_model_is_cooling(
            &records[1].private_data,
            "claude-sonnet-4-6",
            now
        ));
        let retry = first.private_data["modelFamilyCooldowns"]["claude"]
            .as_i64()
            .unwrap();
        assert!(
            retry >= now + 360_000 && retry < now + 370_000,
            "must use retry delay, not healthy Claude or exhausted Gemini reset"
        );
        assert!(!antigravity_model_is_cooling(
            &first.private_data,
            "claude-sonnet-4-6",
            retry
        ));
        // The next model request uses the persisted exclusion before choosing an account.
        let excluded = records
            .iter()
            .filter(|r| antigravity_model_is_cooling(&r.private_data, "claude-sonnet-4-6", now))
            .map(|r| r.id.clone())
            .collect::<Vec<_>>();
        let chosen = reloaded
            .select_resource_with_strategy(
                fx.plugin_id,
                "google-account",
                &excluded,
                None,
                None,
                Some(PoolStrategy::Failover),
            )
            .await
            .unwrap();
        assert_eq!(chosen.id, "b");
        fx.registry
            .cool_model_resource(
                fx.plugin_id,
                "google-account",
                "a",
                "claude-opus-4-6",
                std::time::Duration::from_secs(10),
            )
            .await
            .unwrap();
        let records = fx
            .registry
            .resources(fx.plugin_id, "google-account")
            .await
            .unwrap();
        assert_eq!(
            records[0].private_data["modelFamilyCooldowns"]["claude"],
            retry
        );
    }

    #[tokio::test]
    async fn antigravity_family_cooldown_honors_own_reset_and_disabled_state() {
        use crate::plugin::ResourceRecord;
        let fx = Fixture::new("dev.nexusor.plugins.antigravity-auth").await;
        let now = super::now_ms();
        let data = serde_json::json!({"quota": {
            "claude_weekly": {"remaining_percent": 0, "reset_at_ms": now + 600_000},
            "gemini_weekly": {"remaining_percent": 0, "reset_at_ms": now + 900_000}
        }});
        fx.registry
            .restore_resources(
                fx.plugin_id,
                "google-account",
                vec![
                    ResourceRecord {
                        id: "a".into(),
                        key: "a".into(),
                        state: ResourceState::Ready,
                        private_data: data.clone(),
                        created_at_ms: now,
                        updated_at_ms: now,
                    },
                    ResourceRecord {
                        id: "disabled".into(),
                        key: "disabled".into(),
                        state: ResourceState::Disabled,
                        private_data: data.clone(),
                        created_at_ms: now,
                        updated_at_ms: now,
                    },
                ],
            )
            .await
            .unwrap();
        for id in ["a", "disabled"] {
            fx.registry
                .cool_model_resource(
                    fx.plugin_id,
                    "google-account",
                    id,
                    "claude-sonnet-4-6",
                    std::time::Duration::from_secs(10),
                )
                .await
                .unwrap();
        }
        let records = fx
            .registry
            .resources(fx.plugin_id, "google-account")
            .await
            .unwrap();
        assert_eq!(
            records[0].private_data["modelFamilyCooldowns"]["claude"],
            now + 660_000
        );
        assert_eq!(records[0].private_data["quota"], data["quota"]);
        assert_eq!(records[1].private_data, data);
        assert!(matches!(records[1].state, ResourceState::Disabled));
    }

    #[tokio::test]
    async fn cooldown_preserves_disabled_accounts() {
        let fx = Fixture::new("disabled-account-test").await;
        fx.seed(&["a"]).await;
        let record = fx.records().await.remove(0);
        fx.registry
            .inner
            .state
            .apply_patch(
                fx.plugin_id,
                "account",
                &record.id,
                crate::plugin::ResourcePatch {
                    private_data: None,
                    state: Some(crate::plugin::ResourceStateInput::Disabled),
                },
            )
            .await
            .unwrap();
        fx.cool(&record.id, 60).await;
        assert_eq!(fx.records().await[0].state, ResourceState::Disabled);
        assert!(fx
            .registry
            .select_resource(fx.plugin_id, "account", &[])
            .await
            .is_err());
    }

    #[tokio::test]
    async fn slot_strategy_overrides_pool_and_conversation_affinity() {
        let fx = Fixture::new("slot-strategy-test").await;
        fx.seed(&["a", "b"]).await;
        fx.registry
            .set_pool_strategy(fx.plugin_id, PoolStrategy::Single)
            .await
            .unwrap();
        let first = fx
            .registry
            .select_resource_with_strategy(
                fx.plugin_id,
                "account",
                &[],
                None,
                Some("conversation"),
                Some(PoolStrategy::RoundRobin),
            )
            .await
            .unwrap();
        let second = fx
            .registry
            .select_resource_with_strategy(
                fx.plugin_id,
                "account",
                &[],
                None,
                Some("conversation"),
                Some(PoolStrategy::RoundRobin),
            )
            .await
            .unwrap();
        assert_ne!(first.id, second.id);
        let allowed = vec![first.id.clone()];
        let restricted = fx
            .registry
            .select_resource_with_strategy(
                fx.plugin_id,
                "account",
                &[],
                Some(&allowed),
                Some("conversation"),
                Some(PoolStrategy::RoundRobin),
            )
            .await
            .unwrap();
        assert_eq!(restricted.id, first.id);
    }

    #[tokio::test]
    async fn cooling_never_shortens_a_live_cooldown() {
        let plugin_id: &'static str =
            Box::leak(format!("plugin-cool-keep-{}", uuid::Uuid::new_v4()).into_boxed_str());
        let fx = Fixture::new(plugin_id).await;
        fx.seed(&["a"]).await;
        let record = fx
            .registry
            .select_resource(fx.plugin_id, "account", &[])
            .await
            .unwrap();
        fx.cool(&record.id, 3600).await;
        let before = fx.records().await;
        fx.cool(&record.id, 10).await;
        let after = fx.records().await;
        let retry_at = |state: &ResourceState| match state {
            ResourceState::Cooling { retry_at_ms, .. } => *retry_at_ms,
            _ => None,
        };
        assert_eq!(retry_at(&before[0].state), retry_at(&after[0].state));
    }

    #[tokio::test]
    async fn excluded_accounts_are_not_reselected() {
        let plugin_id: &'static str =
            Box::leak(format!("plugin-exclude-{}", uuid::Uuid::new_v4()).into_boxed_str());
        let fx = Fixture::new(plugin_id).await;
        fx.registry
            .set_pool_strategy(fx.plugin_id, PoolStrategy::Failover)
            .await
            .unwrap();
        fx.seed(&["a", "b"]).await;
        let first = fx
            .registry
            .select_resource(fx.plugin_id, "account", &[])
            .await
            .unwrap();
        let second = fx
            .registry
            .select_resource(fx.plugin_id, "account", std::slice::from_ref(&first.id))
            .await
            .unwrap();
        assert_ne!(first.id, second.id);
        assert!(
            fx.registry
                .select_resource(fx.plugin_id, "account", &[first.id, second.id])
                .await
                .is_err(),
            "all accounts excluded: no reselect"
        );
    }

    #[tokio::test]
    async fn exact_quota_reset_with_60s_safety_buffer_is_used() {
        use crate::plugin::state::{now_ms, ResourcePatch};
        let plugin_id: &'static str =
            Box::leak(format!("plugin-quota-reset-{}", uuid::Uuid::new_v4()).into_boxed_str());
        let fx = Fixture::new(plugin_id).await;
        fx.seed(&["a"]).await;
        let record = fx
            .registry
            .select_resource(fx.plugin_id, "account", &[])
            .await
            .unwrap();

        // Inject quota with reset_at_ms 5 hours in the future
        let now = now_ms();
        let reset_at_ms = now + (5 * 3600 * 1000);
        fx.registry
            .inner
            .state
            .apply_patch(
                fx.plugin_id,
                "account",
                &record.id,
                ResourcePatch {
                    private_data: Some(serde_json::json!({
                        "quota": {
                            "gemini": {
                                "remaining_percent": 0.0,
                                "reset_at_ms": reset_at_ms,
                            }
                        }
                    })),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        // Cool the resource
        fx.registry
            .cool_resource(
                fx.plugin_id,
                "account",
                &record.id,
                std::time::Duration::from_secs(60),
                "upstream quota exhausted",
            )
            .await
            .unwrap();

        let records = fx.records().await;
        let state = &records[0].state;
        match state {
            ResourceState::Cooling { retry_at_ms, .. } => {
                let expected = reset_at_ms + 60_000;
                assert_eq!(
                    *retry_at_ms,
                    Some(expected),
                    "must match upstream reset_at_ms + 60s safety buffer"
                );
            }
            _ => panic!("account must be in Cooling state"),
        }

        // While cooling, select_resource must fail with friendly cooling message
        let err = fx
            .registry
            .select_resource(fx.plugin_id, "account", &[])
            .await
            .unwrap_err();
        assert!(err.to_string().contains("cooling down"));
    }
}
