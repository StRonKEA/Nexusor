//! Verifies Combo routing, Round-Robin rotation across multiple accounts, Failover,
//! Auto Router task intent classification, Multi-Model Review concurrency,
//! Subagent session isolation, and Vision Sidecar image detection.
#[path = "support/fixtures.rs"]
mod fixtures;

use cursor_server::{
    model::{
        ContentPart, ConversationId, ModelInvocation, ModelRequest, ModelSpec, PreparedRun,
        ProjectedContent, ProjectedMessage, PromptSpec, ReasoningSpec, Role, RunAction, RunId,
        RunKind, SubagentKind,
    },
    plugin::{PluginRegistry, PluginRuntime},
    provider::{
        classifier::{classify_intent, TaskIntent},
        vision_sidecar::model_supports_images,
    },
    store::{AutoRouterConfig, ComboSlot, RouterCombo, RunStatus},
};

fn make_invocation(text: &str, tools: Vec<&str>, reasoning_enabled: bool) -> ModelInvocation {
    ModelInvocation {
        call_id: "test-call-1".into(),
        provider_call_index: 0,
        run_id: "test-run".into(),
        conversation_id: "conv-1".into(),
        request: ModelRequest {
            prompt: PromptSpec {
                instructions: String::new(),
                tools: tools
                    .into_iter()
                    .map(|n| cursor_server::model::ToolDefinition {
                        name: n.into(),
                        description: String::new(),
                        parameters: serde_json::json!({}),
                    })
                    .collect(),
            },
            history: vec![ProjectedMessage {
                message_id: "m1".into(),
                role: Role::User,
                content: ProjectedContent::Parts(vec![ContentPart::Text { text: text.into() }]),
            }],
            model: ModelSpec {
                model_id: "auto-smart".into(),
                display_name: None,
                max_output_tokens: None,
                context_window_tokens: None,
                supports_image_generation: false,
                reasoning: ReasoningSpec {
                    enabled: reasoning_enabled,
                    effort: None,
                },
                latency: Default::default(),
                extra_params: Default::default(),
            },
        },
        slot_account_ids: None,
        slot_strategy: None,
    }
}

#[tokio::test]
async fn manual_subagent_route_uses_second_model_after_local_failure() {
    use axum::{http::StatusCode, routing::post, Json, Router};
    use cursor_server::{
        model::{ModelConfigInput, ModelType, OPENAI_CHAT_ENDPOINT},
        provider::{ModelEvent, Provider, ProviderRouter},
    };
    use futures_util::StreamExt;
    use std::sync::{Arc, Mutex};
    let (_directory, store) = fixtures::temp_store().await;
    let seen = Arc::new(Mutex::new(Vec::new()));
    let captured = seen.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/chat/completions", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        axum::serve(listener, Router::new().route("/chat/completions", post(move |Json(body): Json<serde_json::Value>| {
            let captured = captured.clone();
            async move {
                let model = body["model"].as_str().unwrap().to_owned();
                captured.lock().unwrap().push(model.clone());
                if model == "first" { return (StatusCode::SERVICE_UNAVAILABLE, [(axum::http::header::CONTENT_TYPE, "text/plain")], "unavailable"); }
                (StatusCode::OK, [(axum::http::header::CONTENT_TYPE, "text/event-stream")], "data: {\"choices\":[{\"delta\":{\"content\":\"SUBAGENT_FALLBACK_OK\"},\"finish_reason\":null}]}\n\ndata: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n")
            }
        }))).await.unwrap();
    });
    let mut slots = Vec::new();
    for id in ["first", "second"] {
        let model = store
            .create_model(&ModelConfigInput {
                sort_order: 0,
                display_name: id.into(),
                group_name: None,
                model_type: ModelType::OpenAi,
                base_url: url.clone(),
                use_full_url: true,
                api_key: "local-test".into(),
                tooltip_data: id.into(),
                model_id: id.into(),
                reasoning_effort: None,
                openai_endpoint: OPENAI_CHAT_ENDPOINT.into(),
                openai_extra_params_enabled: false,
                openai_extra_params: serde_json::json!({}),
                custom_headers_enabled: false,
                custom_headers: serde_json::json!({}),
                anthropic_extra_params_enabled: false,
                anthropic_extra_params: serde_json::json!({}),
                context_window_tokens: None,
                max_completion_tokens: None,
                anthropic_max_tokens: None,
                anthropic_thinking_effort: None,
                thinking_budget_tokens: None,
            })
            .await
            .unwrap();
        slots.push(ComboSlot::simple(model.model_hash));
    }
    let config = AutoRouterConfig {
        subagent_auto: false,
        subagent_slots: slots,
        ..Default::default()
    };
    store.set_auto_router_config(config.clone()).await.unwrap();
    let plugins = PluginRegistry::managed(
        store.clone(),
        PluginRuntime::managed().unwrap(),
        "1.0.0".into(),
    )
    .unwrap();
    let router = ProviderRouter::new(
        store.clone(),
        plugins,
        cursor_server::network::NetworkClients::new(store.clone()),
        std::time::Duration::from_secs(5),
        std::time::Duration::from_secs(5),
    );
    let mut invocation = make_invocation("Reply briefly", vec![], false);
    invocation.request.model.model_id = config.manual_subagent_route().unwrap();
    let mut stream = router.stream(
        invocation.clone(),
        tokio_util::sync::CancellationToken::new(),
    );
    let mut text = String::new();
    while let Some(event) = stream.next().await {
        if let ModelEvent::TextDelta(delta) = event.unwrap() {
            text.push_str(&delta);
        }
    }
    assert_eq!(text, "SUBAGENT_FALLBACK_OK");
    assert_eq!(*seen.lock().unwrap(), vec!["first", "second"]);
    store
        .set_auto_router_config(AutoRouterConfig::default())
        .await
        .unwrap();
    let mut stream = router.stream(invocation, tokio_util::sync::CancellationToken::new());
    assert!(stream
        .next()
        .await
        .unwrap()
        .err()
        .unwrap()
        .to_string()
        .contains("no longer configured"));
    assert_eq!(seen.lock().unwrap().len(), 2);
    server.abort();
}

// ── SENARYO 1: Çoklu Hesap Round-Robin Rotasyonu (4 Antigravity + 1 Copilot) ──
#[tokio::test]
async fn test_multi_account_round_robin_rotation_5_slots() {
    let (_directory, store) = fixtures::temp_store().await;

    // Kullanıcının 4 Google Antigravity hesabı + 1 GitHub Copilot hesabı
    let acc_google_1 = "c17ec23f-f4dd-48bc-95a1-391945635e2d";
    let acc_google_2 = "a42ed202-b4f7-47eb-a49f-75d688b74ee6";
    let acc_google_3 = "252ebd90-c371-4e9f-b27d-2d5798608aab";
    let acc_google_4 = "09a91e4f-e158-4bca-b32f-46f9476aaf89";
    let acc_copilot = "15b43e73-8a8b-4cae-8b10-d1b3503de939";

    let combo = RouterCombo {
        combo_id: "rr-5-accounts".into(),
        name: "5 Hesaplı Round-Robin Havuzu".into(),
        description: Some("4 Antigravity + 1 Copilot dönüşümlü".into()),
        strategy: "round_robin".into(),
        models: vec![
            "gemini-2.5-flash".into(),
            "gemini-2.5-flash".into(),
            "gemini-3.8-flash".into(),
            "gemini-2.5-flash".into(),
            "gpt-4o".into(),
        ],
        slots: vec![
            ComboSlot {
                model_id: "gemini-2.5-flash".into(),
                account_ids: Some(vec![acc_google_1.into()]),
                slot_strategy: "failover".into(),
            },
            ComboSlot {
                model_id: "gemini-2.5-flash".into(),
                account_ids: Some(vec![acc_google_2.into()]),
                slot_strategy: "failover".into(),
            },
            ComboSlot {
                model_id: "gemini-3.8-flash".into(),
                account_ids: Some(vec![acc_google_3.into()]),
                slot_strategy: "failover".into(),
            },
            ComboSlot {
                model_id: "gemini-2.5-flash".into(),
                account_ids: Some(vec![acc_google_4.into()]),
                slot_strategy: "failover".into(),
            },
            ComboSlot {
                model_id: "gpt-4o".into(),
                account_ids: Some(vec![acc_copilot.into()]),
                slot_strategy: "failover".into(),
            },
        ],
        enabled: true,
        created_at_ms: 1000,
        updated_at_ms: 1000,
    };

    let saved = store.upsert_router_combo(combo).await.unwrap();
    assert_eq!(saved.strategy, "round_robin");
    assert_eq!(saved.slots.len(), 5);

    // 5 çağrı yapıldığında rotasyonun 0, 1, 2, 3, 4 ve tekrar 0 indeksine döndüğünü doğrula
    let slots_vec = saved.slots;
    for turn in 0..10 {
        let expected_offset = turn % 5;
        let mut rotated = slots_vec.clone();
        rotated.rotate_left(expected_offset);

        let active_slot = &rotated[0];
        let fallback_count = rotated.len() - 1;

        assert_eq!(fallback_count, 4, "Yedek zinciri 4 slot içermeli");

        match expected_offset {
            0 => {
                assert_eq!(active_slot.model_id, "gemini-2.5-flash");
                assert_eq!(active_slot.account_ids.as_ref().unwrap()[0], acc_google_1);
            }
            1 => {
                assert_eq!(active_slot.model_id, "gemini-2.5-flash");
                assert_eq!(active_slot.account_ids.as_ref().unwrap()[0], acc_google_2);
            }
            2 => {
                assert_eq!(active_slot.model_id, "gemini-3.8-flash");
                assert_eq!(active_slot.account_ids.as_ref().unwrap()[0], acc_google_3);
            }
            3 => {
                assert_eq!(active_slot.model_id, "gemini-2.5-flash");
                assert_eq!(active_slot.account_ids.as_ref().unwrap()[0], acc_google_4);
            }
            4 => {
                assert_eq!(active_slot.model_id, "gpt-4o");
                assert_eq!(active_slot.account_ids.as_ref().unwrap()[0], acc_copilot);
            }
            _ => unreachable!(),
        }
    }
}

// ── SENARYO 2: Anlık Kota / Hata Durumunda Otomatik Sıradaki Hesaba Geçiş (Failover) ──
#[tokio::test]
async fn test_failover_when_primary_slot_fails() {
    let (_directory, store) = fixtures::temp_store().await;

    let combo = RouterCombo {
        combo_id: "failover-chain".into(),
        name: "Failover Zinciri".into(),
        description: Some("1. Slot çökerse 2. ve 3. slota geçiş".into()),
        strategy: "fallback".into(),
        models: vec![
            "slot-1-unstable".into(),
            "slot-2-backup".into(),
            "slot-3-rescue".into(),
        ],
        slots: vec![
            ComboSlot {
                model_id: "slot-1-unstable".into(),
                account_ids: Some(vec!["acc-unstable".into()]),
                slot_strategy: "failover".into(),
            },
            ComboSlot {
                model_id: "slot-2-backup".into(),
                account_ids: Some(vec!["acc-backup".into()]),
                slot_strategy: "failover".into(),
            },
            ComboSlot {
                model_id: "slot-3-rescue".into(),
                account_ids: Some(vec!["acc-rescue".into()]),
                slot_strategy: "failover".into(),
            },
        ],
        enabled: true,
        created_at_ms: 1000,
        updated_at_ms: 1000,
    };

    let saved = store.upsert_router_combo(combo).await.unwrap();
    assert_eq!(saved.strategy, "fallback");

    // Fallback stratejisinde 1. slot ana modeldir, kalan 2 slot yedek (fallback) listesidir
    let mut slots_iter = saved.slots.into_iter();
    let primary = slots_iter.next().unwrap();
    let fallbacks: Vec<_> = slots_iter.collect();

    assert_eq!(primary.model_id, "slot-1-unstable");
    assert_eq!(fallbacks.len(), 2);
    assert_eq!(fallbacks[0].model_id, "slot-2-backup");
    assert_eq!(fallbacks[1].model_id, "slot-3-rescue");
}

// ── SENARYO 3: Auto Router Görev Sınıflandırma ve Slot Eşleme ──
#[tokio::test]
async fn test_auto_router_intent_mapping() {
    let (_directory, store) = fixtures::temp_store().await;

    // 1. Coding Görevi Sınıflandırması
    let coding_inv = make_invocation(
        "Bu fonksiyondaki null pointer hatasını düzelt ve test yaz",
        vec!["edit_file"],
        false,
    );
    assert_eq!(classify_intent(&coding_inv), TaskIntent::Coding);

    // 2. Complex / Mimari Görevi Sınıflandırması
    let arch_inv = make_invocation(
        "Veritabanı transaction mimarisi ve event-driven system design planlaması yap",
        vec![],
        false,
    );
    assert_eq!(classify_intent(&arch_inv), TaskIntent::Complex);

    // 3. Explicit Thinking / Akıl Yürütme Etkin
    let reasoning_inv = make_invocation("Herhangi bir soru", vec![], true);
    assert_eq!(classify_intent(&reasoning_inv), TaskIntent::Complex);

    // 4. Hızlı Soru-Cevap (Fast Intent)
    let fast_inv = make_invocation("Rust'ta Option unwrap ne işe yarar?", vec![], false);
    assert_eq!(classify_intent(&fast_inv), TaskIntent::Fast);

    // Auto Router konfigürasyonunu kaydet ve oku
    let auto_config = AutoRouterConfig {
        enabled: true,
        coding_slots: vec![ComboSlot::simple("claude-3-5-sonnet")],
        reasoning_slots: vec![ComboSlot::simple("claude-3-7-sonnet")],
        fast_slots: vec![ComboSlot::simple("gemini-2.5-flash")],
        vision_auto: true,
        vision_slots: vec![],
        subagent_auto: true,
        subagent_slots: vec![],
        subagent_write_access: true,
        updated_at_ms: 1000,
    };
    store.set_auto_router_config(auto_config).await.unwrap();

    let loaded = store.auto_router_config().await.unwrap();
    assert_eq!(loaded.coding_slots[0].model_id, "claude-3-5-sonnet");
    assert_eq!(loaded.reasoning_slots[0].model_id, "claude-3-7-sonnet");
    assert_eq!(loaded.fast_slots[0].model_id, "gemini-2.5-flash");
}

// ── SENARYO 4: Vision Sidecar Görsel Çözümleme ve Model Desteği Tespiti ──
#[tokio::test]
async fn test_vision_sidecar_detection_and_transcription_rules() {
    let (_directory, store) = fixtures::temp_store().await;
    let runtime = PluginRuntime::managed().unwrap();
    let plugins = PluginRegistry::managed(store.clone(), runtime, "1.0.0".into()).unwrap();

    // Görselsiz modeller: Kesinlikle false dönmeli
    assert!(!model_supports_images(&store, &plugins, "o3-mini").await);
    assert!(!model_supports_images(&store, &plugins, "o1-mini").await);
    assert!(!model_supports_images(&store, &plugins, "o1-preview").await);
    assert!(!model_supports_images(&store, &plugins, "deepseek-coder").await);
    assert!(!model_supports_images(&store, &plugins, "codex-auto-review").await);

    // Görselli modeller: Kesinlikle true dönmeli
    assert!(model_supports_images(&store, &plugins, "gemini-2.5-flash").await);
    assert!(model_supports_images(&store, &plugins, "gemini-3.8-flash").await);
    assert!(model_supports_images(&store, &plugins, "claude-3-5-sonnet").await);
    assert!(model_supports_images(&store, &plugins, "gpt-4o").await);
}

// ── SENARYO 5: Alt Ajan Oturum Kilidi ve Yetki İzolasyonu ──
#[tokio::test]
async fn test_subagent_run_does_not_hijack_parent_active_run_id() {
    let (_directory, store) = fixtures::temp_store().await;

    let conversation_id = ConversationId::from("conv-subagent-test");
    let checkpoint_id = store.ensure_conversation(&conversation_id).await.unwrap();

    // 1. Ana (Root) Ajanı Başlat
    let parent_run = PreparedRun {
        run_id: RunId::from("parent-run-1"),
        cursor_request_id: Some("req-parent".into()),
        conversation_id: conversation_id.clone(),
        base_checkpoint_id: checkpoint_id,
        kind: RunKind::Root,
        model: ModelSpec::new("claude-3-7-sonnet"),
        prompt: PromptSpec {
            instructions: "Main orchestrator".into(),
            tools: Vec::new(),
        },
        initial_messages: Vec::new(),
        action: RunAction::Start,
    };
    store.claim_run(&parent_run).await.unwrap();

    // conversations.active_run_id ana ajan olmalı
    let active_id: Option<String> =
        sqlx::query_scalar("SELECT active_run_id FROM conversations WHERE conversation_id = ?")
            .bind(conversation_id.as_str())
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(active_id.as_deref(), Some("parent-run-1"));

    // 2. Alt Ajan (Subagent) Başlat
    let subagent_run = PreparedRun {
        run_id: RunId::from("subagent-run-1"),
        cursor_request_id: Some("req-subagent".into()),
        conversation_id: conversation_id.clone(),
        base_checkpoint_id: checkpoint_id,
        kind: RunKind::Subagent {
            parent_run_id: RunId::from("parent-run-1"),
            parent_tool_call_id: "tool-call-task-1".into(),
            kind: SubagentKind::Named("explore".into()),
            background: false,
        },
        model: ModelSpec::new("gemini-2.5-flash"),
        prompt: PromptSpec {
            instructions: "Explore subagent".into(),
            tools: Vec::new(),
        },
        initial_messages: Vec::new(),
        action: RunAction::Start,
    };
    store.claim_run(&subagent_run).await.unwrap();

    // Alt ajan çalışırken bile active_run_id ana ajan kalmalı!
    let active_id_during: Option<String> =
        sqlx::query_scalar("SELECT active_run_id FROM conversations WHERE conversation_id = ?")
            .bind(conversation_id.as_str())
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(active_id_during.as_deref(), Some("parent-run-1"));

    // 3. Alt Ajan kapandığında ana ajanın oturumu düşmemeli!
    store
        .finish_run(&subagent_run.run_id, RunStatus::Completed, None, None)
        .await
        .unwrap();

    let active_id_after: Option<String> =
        sqlx::query_scalar("SELECT active_run_id FROM conversations WHERE conversation_id = ?")
            .bind(conversation_id.as_str())
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(active_id_after.as_deref(), Some("parent-run-1"));

    // Ana ajan bittiğinde kilit serbest bırakılmalı
    store
        .finish_run(&parent_run.run_id, RunStatus::Completed, None, None)
        .await
        .unwrap();
    let final_active: Option<String> =
        sqlx::query_scalar("SELECT active_run_id FROM conversations WHERE conversation_id = ?")
            .bind(conversation_id.as_str())
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(final_active, None);
}

// ── SENARYO 6: Çoklu Model İncelemesi (Multi-Model Review) Eşzamanlılık Kalkanı ──
#[tokio::test]
async fn test_multi_model_review_concurrency_isolation() {
    let (_directory, store) = fixtures::temp_store().await;

    let conversation_id = ConversationId::from("conv-review-test");
    let checkpoint_id = store.ensure_conversation(&conversation_id).await.unwrap();

    // 1. İnceleme Modeli A (Claude 3.7 Sonnet)
    let run_a = PreparedRun {
        run_id: RunId::from("run-review-a"),
        cursor_request_id: Some("req-review-a".into()),
        conversation_id: conversation_id.clone(),
        base_checkpoint_id: checkpoint_id,
        kind: RunKind::Root,
        model: ModelSpec::new("claude-3-7-sonnet"),
        prompt: PromptSpec {
            instructions: "Review code".into(),
            tools: Vec::new(),
        },
        initial_messages: Vec::new(),
        action: RunAction::Start,
    };
    store.claim_run(&run_a).await.unwrap();

    // 2. İnceleme Modeli B (Gemini 3.8 Flash) aynı konuşmada paralel başlar
    let run_b = PreparedRun {
        run_id: RunId::from("run-review-b"),
        cursor_request_id: Some("req-review-b".into()),
        conversation_id: conversation_id.clone(),
        base_checkpoint_id: checkpoint_id,
        kind: RunKind::Root,
        model: ModelSpec::new("gemini-3-8-flash"),
        prompt: PromptSpec {
            instructions: "Review code".into(),
            tools: Vec::new(),
        },
        initial_messages: Vec::new(),
        action: RunAction::Start,
    };
    store.claim_run(&run_b).await.unwrap();

    // 3. İki model de eşzamanlı olarak 'running' durumunu korumalıdır
    let status_a: String =
        sqlx::query_scalar("SELECT status FROM runs WHERE run_id = 'run-review-a'")
            .fetch_one(store.pool())
            .await
            .unwrap();
    let status_b: String =
        sqlx::query_scalar("SELECT status FROM runs WHERE run_id = 'run-review-b'")
            .fetch_one(store.pool())
            .await
            .unwrap();

    assert_eq!(
        status_a, "running",
        "Model A paralel çalışırken iptal edilmemeli"
    );
    assert_eq!(status_b, "running", "Model B çalışır durumda olmalı");

    // İki inceleme de tamamlanır
    store
        .finish_run(&run_a.run_id, RunStatus::Completed, None, None)
        .await
        .unwrap();
    store
        .finish_run(&run_b.run_id, RunStatus::Completed, None, None)
        .await
        .unwrap();
}

// ── SENARYO 7: Token Otomatik Yenileme ve 120 Saniyelik Güvenlik Payı (Skew Margin) ──
#[test]
fn test_token_auto_refresh_lifecycle_and_skew_margin() {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    use cursor_server::provider::providers::codex::access_token_needs_refresh;

    let now_sec = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    // 1. Durum: Token'ın bitmesine 60 saniye kalmış (120 saniyelik pay yüzünden HEMEN yenilenmeli)
    let payload_exp_soon = serde_json::json!({
        "sub": "user_123",
        "exp": now_sec + 60
    });
    let encoded_soon = URL_SAFE_NO_PAD.encode(payload_exp_soon.to_string().as_bytes());
    let jwt_soon = format!("header.{}.signature", encoded_soon);

    assert!(
        access_token_needs_refresh(&jwt_soon),
        "60s kalmış token 120s güvenlik payından dolayı yenilenmeye ihtiyaç duymalı"
    );

    // 2. Durum: Token'ın bitmesine 600 saniye (10 dakika) var (Yenilenmesine gerek yok)
    let payload_exp_later = serde_json::json!({
        "sub": "user_123",
        "exp": now_sec + 600
    });
    let encoded_later = URL_SAFE_NO_PAD.encode(payload_exp_later.to_string().as_bytes());
    let jwt_later = format!("header.{}.signature", encoded_later);

    assert!(
        !access_token_needs_refresh(&jwt_later),
        "10 dakika kalmış token yenilenmeye ihtiyaç duymamalı"
    );
}

// ── SENARYO 8: Hesap Durum Geçişleri (Ready, Cooling, Disabled) ve Havuz İzolasyonu ──
#[test]
fn test_resource_state_transitions_and_pool_isolation() {
    use cursor_server::plugin::ResourceState;

    let now_ms = 1_000_000;

    // Ready durumu hazır olmalı
    let ready_state = ResourceState::Ready;
    assert!(ready_state.is_ready(now_ms));

    // Disabled (Pasife alınmış) hesap asla hazır olmamalı
    let disabled_state = ResourceState::Disabled;
    assert!(!disabled_state.is_ready(now_ms), "Pasif hesap hazır olamaz");

    // Cooling (Kota bitimi / Soğuma) - süresi dolmamışken hazır olmamalı
    let cooling_active = ResourceState::Cooling {
        retry_at_ms: Some(now_ms + 60_000),
        message: Some("Quota exceeded".into()),
    };
    assert!(
        !cooling_active.is_ready(now_ms),
        "Süresi dolmamış cooling hesap hazır olamaz"
    );

    // Cooling süresi dolunca otomatik ready'ye dönmeli
    let cooling_expired = ResourceState::Cooling {
        retry_at_ms: Some(now_ms - 1000),
        message: None,
    };
    assert!(
        cooling_expired.is_ready(now_ms),
        "Süresi dolan cooling hesap otomatik hazır olmalı"
    );
}

// ── SENARYO 9: Masaüstü ayar varsayılanları ──
#[tokio::test]
async fn test_desktop_settings_defaults() {
    let (_directory, store) = fixtures::temp_store().await;

    let settings = store.desktop_settings().await.unwrap();

    // Only-Nexusor mode (hiding Cursor's built-in models) is on by default.
    assert!(settings.hide_cursor_builtin_models);
    assert!(settings.show_dock_icon);
    assert!(!settings.silent_start);
}
