use super::*;

impl PluginRegistry {
    pub async fn oauth_begin(
        &self,
        plugin_id: &str,
        resource_type: &str,
        method_id: &str,
    ) -> Result<OAuthBeginResponse> {
        let client = self.client().await?;
        match (plugin_id, method_id) {
            (
                "dev.nexusor.examples.codex-auth" | "dev.cursorbyok.examples.codex-auth",
                "chatgpt-device",
            ) => {
                let res =
                    crate::provider::providers::codex::oauth::begin_device_flow(&client).await?;
                let session_id = uuid::Uuid::new_v4().to_string();
                self.inner.oauth_sessions.lock().await.insert(
                    session_id.clone(),
                    OAuthSession {
                        plugin_id: plugin_id.into(),
                        resource_type: resource_type.into(),
                        session: res.session,
                        poll_interval_ms: res.poll_interval_ms as i64,
                        flow: OAuthFlow::DeviceCode,
                    },
                );
                Ok(OAuthBeginResponse {
                    session_id,
                    user_code: Some(res.user_code),
                    verification_url: Some(res.verification_url),
                    verification_url_complete: None,
                    expires_at_ms: res.expires_at_ms,
                    poll_interval_ms: res.poll_interval_ms as i64,
                })
            }
            ("dev.nexusor.plugins.github-copilot", "github-copilot-device") => {
                let res =
                    crate::provider::providers::copilot::oauth::begin_device_flow(&client).await?;
                let session_id = uuid::Uuid::new_v4().to_string();
                self.inner.oauth_sessions.lock().await.insert(
                    session_id.clone(),
                    OAuthSession {
                        plugin_id: plugin_id.into(),
                        resource_type: resource_type.into(),
                        session: res.session,
                        poll_interval_ms: 5000,
                        flow: OAuthFlow::DeviceCode,
                    },
                );
                Ok(OAuthBeginResponse {
                    session_id,
                    user_code: Some(res.user_code),
                    verification_url: Some(res.url),
                    verification_url_complete: None,
                    expires_at_ms: chrono::Utc::now().timestamp_millis() + 900_000,
                    poll_interval_ms: 5000,
                })
            }
            ("dev.nexusor.plugins.kimi-auth", "kimi-device") => {
                let res =
                    crate::provider::providers::kimi::oauth::begin_device_flow(&client).await?;
                let session_id = uuid::Uuid::new_v4().to_string();
                self.inner.oauth_sessions.lock().await.insert(
                    session_id.clone(),
                    OAuthSession {
                        plugin_id: plugin_id.into(),
                        resource_type: resource_type.into(),
                        session: res.session,
                        poll_interval_ms: res.poll_interval_ms as i64,
                        flow: OAuthFlow::DeviceCode,
                    },
                );
                Ok(OAuthBeginResponse {
                    session_id,
                    user_code: Some(res.user_code),
                    verification_url: Some(res.verification_url),
                    verification_url_complete: res.verification_url_complete,
                    expires_at_ms: res.expires_at_ms,
                    poll_interval_ms: res.poll_interval_ms as i64,
                })
            }
            (
                "dev.nexusor.examples.grok-auth" | "dev.cursorbyok.examples.grok-auth",
                "grok-device",
            ) => {
                let res =
                    crate::provider::providers::grok::oauth::begin_device_flow(&client).await?;
                let session_id = uuid::Uuid::new_v4().to_string();
                self.inner.oauth_sessions.lock().await.insert(
                    session_id.clone(),
                    OAuthSession {
                        plugin_id: plugin_id.into(),
                        resource_type: resource_type.into(),
                        session: res.session,
                        poll_interval_ms: res.poll_interval_ms as i64,
                        flow: OAuthFlow::DeviceCode,
                    },
                );
                Ok(OAuthBeginResponse {
                    session_id,
                    user_code: Some(res.user_code),
                    verification_url: Some(res.verification_url),
                    verification_url_complete: res.verification_url_complete,
                    expires_at_ms: res.expires_at_ms,
                    poll_interval_ms: res.poll_interval_ms as i64,
                })
            }
            ("dev.nexusor.plugins.claude-code", "claude-code-oauth") => {
                let state = crate::provider::providers::antigravity::pkce::generate_state();
                let (verifier, challenge) =
                    crate::provider::providers::antigravity::pkce::generate_pkce();
                let claude_icon = format!(
                    "data:image/svg+xml;base64,{}",
                    base64::engine::general_purpose::STANDARD
                        .encode(include_bytes!("../../../icons/claude-code.svg"))
                );
                let callback = oauth_callback::bind(
                    Some(54545),
                    "/callback",
                    state.clone(),
                    "Claude Code".into(),
                    claude_icon,
                    serde_json::json!("Claude Account"),
                )
                .await?;

                let redirect_uri = callback.redirect_uri.clone();
                let auth_url =
                    crate::provider::providers::claude_code::oauth::build_authorization_url(
                        &redirect_uri,
                        &state,
                        &challenge,
                    );
                let session_id = uuid::Uuid::new_v4().to_string();
                self.inner.oauth_sessions.lock().await.insert(
                    session_id.clone(),
                    OAuthSession {
                        plugin_id: plugin_id.into(),
                        resource_type: resource_type.into(),
                        session: serde_json::json!({
                            "redirectUri": redirect_uri,
                            "codeVerifier": verifier,
                        }),
                        poll_interval_ms: 1000,
                        flow: OAuthFlow::AuthorizationCode {
                            redirect_uri,
                            code_verifier: verifier,
                            callback,
                        },
                    },
                );
                Ok(OAuthBeginResponse {
                    session_id,
                    user_code: None,
                    verification_url: Some(auth_url),
                    verification_url_complete: None,
                    expires_at_ms: chrono::Utc::now().timestamp_millis() + 900_000,
                    poll_interval_ms: 1000,
                })
            }
            (
                "dev.nexusor.plugins.antigravity-auth" | "dev.cursorbyok.plugins.antigravity-auth",
                "google-oauth",
            ) => {
                let state = crate::provider::providers::antigravity::pkce::generate_state();
                let (verifier, challenge) =
                    crate::provider::providers::antigravity::pkce::generate_pkce();
                let antigravity_icon = format!(
                    "data:image/svg+xml;base64,{}",
                    base64::engine::general_purpose::STANDARD
                        .encode(include_bytes!("../../../icons/antigravity.svg"))
                );
                let callback = oauth_callback::bind(
                    None,
                    "/callback",
                    state.clone(),
                    "Google Antigravity".into(),
                    antigravity_icon,
                    serde_json::json!("Google Account"),
                )
                .await?;

                let redirect_uri = callback.redirect_uri.clone();
                let auth_url =
                    crate::provider::providers::antigravity::oauth::build_authorization_url(
                        &redirect_uri,
                        &state,
                        &challenge,
                    );

                let session_id = uuid::Uuid::new_v4().to_string();
                let expires_at_ms = now_ms() + 300_000;
                let poll_interval_ms = 1000;

                self.inner.oauth_sessions.lock().await.insert(
                    session_id.clone(),
                    OAuthSession {
                        plugin_id: plugin_id.into(),
                        resource_type: resource_type.into(),
                        session: serde_json::json!({ "state": state }),
                        poll_interval_ms,
                        flow: OAuthFlow::AuthorizationCode {
                            redirect_uri,
                            code_verifier: verifier,
                            callback,
                        },
                    },
                );

                Ok(OAuthBeginResponse {
                    session_id,
                    user_code: None,
                    verification_url: Some(auth_url),
                    verification_url_complete: None,
                    expires_at_ms,
                    poll_interval_ms,
                })
            }
            _ => Err(Error::Config(format!(
                "unsupported OAuth add method: {plugin_id}/{method_id}"
            ))),
        }
    }

    pub async fn oauth_poll(&self, session_id: &str) -> Result<OAuthPollResponse> {
        let mut sessions = self.inner.oauth_sessions.lock().await;
        let Some(session) = sessions.get_mut(session_id) else {
            return Err(Error::RunNotFound(format!("OAuth session {session_id}")));
        };

        let client = self.client().await?;
        let plugin_id = session.plugin_id.clone();
        let resource_type = session.resource_type.clone();
        let poll_interval_ms = session.poll_interval_ms.max(1000);

        match &mut session.flow {
            OAuthFlow::DeviceCode => {
                let sess_val = session.session.clone();
                drop(sessions);

                if plugin_id == "dev.nexusor.examples.codex-auth"
                    || plugin_id == "dev.cursorbyok.examples.codex-auth"
                {
                    let sess: crate::provider::providers::codex::oauth::DeviceSession =
                        serde_json::from_value(sess_val)?;
                    let status =
                        crate::provider::providers::codex::oauth::poll_device_flow(&client, &sess)
                            .await?;
                    match status {
                        crate::provider::providers::codex::oauth::DevicePollStatus::Pending => {
                            Ok(oauth_poll_status("pending", None, poll_interval_ms))
                        }
                        crate::provider::providers::codex::oauth::DevicePollStatus::SlowDown => Ok(
                            oauth_poll_status("slow-down", None, (poll_interval_ms * 2).max(5_000)),
                        ),
                        crate::provider::providers::codex::oauth::DevicePollStatus::Denied(msg) => {
                            self.inner.oauth_sessions.lock().await.remove(session_id);
                            Ok(oauth_poll_status("denied", msg, poll_interval_ms))
                        }
                        crate::provider::providers::codex::oauth::DevicePollStatus::Failed(msg) => {
                            self.inner.oauth_sessions.lock().await.remove(session_id);
                            Ok(oauth_poll_status("failed", Some(msg), poll_interval_ms))
                        }
                        crate::provider::providers::codex::oauth::DevicePollStatus::Success {
                            access_token,
                            refresh_token,
                            account_id,
                            email,
                        } => {
                            self.inner.oauth_sessions.lock().await.remove(session_id);
                            let (key, display_name) =
                                crate::provider::providers::codex::resources::account_identity(
                                    &access_token,
                                    account_id.as_deref(),
                                    email.as_deref(),
                                );
                            let quota = crate::provider::providers::codex::usage::query_usage(
                                &client,
                                &access_token,
                                account_id.as_deref(),
                            )
                            .await
                            .ok();
                            let data = serde_json::json!({
                                "accessToken": access_token,
                                "refreshToken": refresh_token,
                                "accountId": account_id,
                                "displayName": display_name,
                                "quota": quota,
                            });
                            self.inner
                                .state
                                .upsert_resources(
                                    &plugin_id,
                                    &resource_type,
                                    vec![ResourceDraft {
                                        key,
                                        private_data: data,
                                        state: None,
                                    }],
                                )
                                .await?;
                            let model_sync_error = self
                                .sync_models(&plugin_id, "codex")
                                .await
                                .err()
                                .map(|error| error.to_string());
                            Ok(OAuthPollResponse {
                                status: "completed".into(),
                                message: None,
                                model_sync_error,
                                poll_interval_ms,
                            })
                        }
                    }
                } else if plugin_id == "dev.nexusor.examples.grok-auth"
                    || plugin_id == "dev.cursorbyok.examples.grok-auth"
                {
                    let sess: crate::provider::providers::grok::oauth::GrokDeviceSession =
                        serde_json::from_value(sess_val)?;
                    let status =
                        crate::provider::providers::grok::oauth::poll_device_flow(&client, &sess)
                            .await?;
                    match status {
                        crate::provider::providers::grok::oauth::GrokPollStatus::Pending => {
                            Ok(oauth_poll_status("pending", None, poll_interval_ms))
                        }
                        crate::provider::providers::grok::oauth::GrokPollStatus::SlowDown => Ok(
                            oauth_poll_status("slow-down", None, (poll_interval_ms * 2).max(5_000)),
                        ),
                        crate::provider::providers::grok::oauth::GrokPollStatus::Denied(msg) => {
                            self.inner.oauth_sessions.lock().await.remove(session_id);
                            Ok(oauth_poll_status("denied", msg, poll_interval_ms))
                        }
                        crate::provider::providers::grok::oauth::GrokPollStatus::Failed(msg) => {
                            self.inner.oauth_sessions.lock().await.remove(session_id);
                            Ok(oauth_poll_status("failed", Some(msg), poll_interval_ms))
                        }
                        crate::provider::providers::grok::oauth::GrokPollStatus::Success {
                            access_token,
                            refresh_token,
                            email,
                        } => {
                            self.inner.oauth_sessions.lock().await.remove(session_id);
                            let email = match email {
                                Some(value) if !value.is_empty() => Some(value),
                                _ => {
                                    crate::provider::providers::grok::resources::fetch_user_email(
                                        &client,
                                        &access_token,
                                    )
                                    .await
                                }
                            };
                            let (key, display_name) =
                                crate::provider::providers::grok::resources::account_identity(
                                    email.as_deref(),
                                    &access_token,
                                );
                            let quota = crate::provider::providers::grok::usage::query_usage(
                                &client,
                                &access_token,
                            )
                            .await
                            .ok();
                            let data = serde_json::json!({
                                "accessToken": access_token,
                                "refreshToken": refresh_token,
                                "displayName": display_name,
                                "quota": quota,
                            });
                            self.inner
                                .state
                                .upsert_resources(
                                    &plugin_id,
                                    &resource_type,
                                    vec![ResourceDraft {
                                        key,
                                        private_data: data,
                                        state: None,
                                    }],
                                )
                                .await?;
                            let model_sync_error = self
                                .sync_models(&plugin_id, "grok")
                                .await
                                .err()
                                .map(|error| error.to_string());
                            Ok(OAuthPollResponse {
                                status: "completed".into(),
                                message: None,
                                model_sync_error,
                                poll_interval_ms,
                            })
                        }
                    }
                } else if plugin_id == "dev.nexusor.plugins.github-copilot" {
                    let session: crate::provider::providers::copilot::oauth::CopilotDeviceSession =
                        serde_json::from_value(sess_val.clone())?;
                    match crate::provider::providers::copilot::oauth::poll_device_flow(&client, &session).await? {
                        crate::provider::providers::copilot::oauth::CopilotPollStatus::Pending => {
                            Ok(oauth_poll_status("pending", None, poll_interval_ms))
                        }
                        crate::provider::providers::copilot::oauth::CopilotPollStatus::SlowDown => {
                            Ok(oauth_poll_status("slow_down", None, 10000))
                        }
                        crate::provider::providers::copilot::oauth::CopilotPollStatus::Denied => {
                            self.inner.oauth_sessions.lock().await.remove(session_id);
                            Ok(oauth_poll_status("denied", None, 0))
                        }
                        crate::provider::providers::copilot::oauth::CopilotPollStatus::Failed(error) => {
                            self.inner.oauth_sessions.lock().await.remove(session_id);
                            Ok(oauth_poll_status("failed", Some(error), 0))
                        }
                        crate::provider::providers::copilot::oauth::CopilotPollStatus::Success {
                            github_token,
                            copilot_token,
                            copilot_expires_at_ms,
                            login,
                        } => {
                            self.inner.oauth_sessions.lock().await.remove(session_id);
                            let key = format!("copilot:{login}");
                            let display_name = format!("GitHub ({login})");
                            let data = serde_json::json!({
                                "githubToken": github_token,
                                "copilotToken": copilot_token,
                                "copilotExpiresAtMs": copilot_expires_at_ms,
                                "displayName": display_name,
                                "login": login,
                            });
                            self.inner
                                .state
                                .upsert_resources(
                                    &plugin_id,
                                    &resource_type,
                                    vec![ResourceDraft {
                                        key,
                                        private_data: data,
                                        state: None,
                                    }],
                                )
                                .await?;
                            let model_sync_error = self
                                .sync_models(&plugin_id, "copilot")
                                .await
                                .err()
                                .map(|error| error.to_string());
                            Ok(OAuthPollResponse {
                                status: "completed".into(),
                                message: None,
                                model_sync_error,
                                poll_interval_ms,
                            })
                        }
                    }
                } else if plugin_id == "dev.nexusor.plugins.kimi-auth" {
                    let session: crate::provider::providers::kimi::oauth::KimiDeviceSession =
                        serde_json::from_value(sess_val.clone())?;
                    match crate::provider::providers::kimi::oauth::poll_device_flow(
                        &client, &session,
                    )
                    .await?
                    {
                        crate::provider::providers::kimi::oauth::KimiPollStatus::Pending => {
                            Ok(oauth_poll_status("pending", None, poll_interval_ms))
                        }
                        crate::provider::providers::kimi::oauth::KimiPollStatus::SlowDown => Ok(
                            oauth_poll_status("slow_down", None, (poll_interval_ms * 2).max(5000)),
                        ),
                        crate::provider::providers::kimi::oauth::KimiPollStatus::Denied => {
                            self.inner.oauth_sessions.lock().await.remove(session_id);
                            Ok(oauth_poll_status("denied", None, 0))
                        }
                        crate::provider::providers::kimi::oauth::KimiPollStatus::Failed(error) => {
                            self.inner.oauth_sessions.lock().await.remove(session_id);
                            Ok(oauth_poll_status("failed", Some(error), 0))
                        }
                        crate::provider::providers::kimi::oauth::KimiPollStatus::Success {
                            access_token,
                            refresh_token,
                            expires_at_ms,
                            user_id,
                            email,
                        } => {
                            self.inner.oauth_sessions.lock().await.remove(session_id);
                            let id_part = email
                                .clone()
                                .or_else(|| user_id.clone())
                                .unwrap_or_else(|| "user".into());
                            let key = format!("kimi:{id_part}");
                            let display_name = format!("Kimi ({id_part})");
                            let quota = crate::provider::providers::kimi::usage::query_usage(
                                &client,
                                &access_token,
                            )
                            .await
                            .ok();
                            let data = serde_json::json!({
                                "accessToken": access_token,
                                "refreshToken": refresh_token,
                                "expiresAtMs": expires_at_ms,
                                "displayName": display_name,
                                "userId": user_id,
                                "email": email,
                                "quota": quota,
                            });
                            self.inner
                                .state
                                .upsert_resources(
                                    &plugin_id,
                                    &resource_type,
                                    vec![ResourceDraft {
                                        key,
                                        private_data: data,
                                        state: None,
                                    }],
                                )
                                .await?;
                            let model_sync_error = self
                                .sync_models(&plugin_id, "kimi")
                                .await
                                .err()
                                .map(|error| error.to_string());
                            Ok(OAuthPollResponse {
                                status: "completed".into(),
                                message: None,
                                model_sync_error,
                                poll_interval_ms,
                            })
                        }
                    }
                } else {
                    Err(Error::Config(format!(
                        "unsupported OAuth poll: {plugin_id}"
                    )))
                }
            }
            OAuthFlow::AuthorizationCode {
                redirect_uri,
                code_verifier,
                callback,
            } => match callback.receiver.try_recv() {
                Ok(request) => {
                    let redirect_uri = redirect_uri.clone();
                    let code_verifier = code_verifier.clone();
                    drop(sessions);

                    match request.result {
                        Ok(code) => {
                            let (access_token, refresh_token, expires_in) =
                                crate::provider::providers::antigravity::oauth::exchange_code(
                                    &client,
                                    &code,
                                    &code_verifier,
                                    &redirect_uri,
                                )
                                .await?;

                            let email = crate::provider::providers::antigravity::userinfo::fetch_user_email(
                                    &client,
                                    &access_token,
                                )
                                .await
                                .or_else(|| {
                                    crate::provider::providers::antigravity::jwt::extract_email_from_jwt(
                                        &access_token,
                                    )
                                });
                            let (key, display_name) =
                                    crate::provider::providers::antigravity::resources::account_identity(
                                        email.as_deref(),
                                        &access_token,
                                    );
                            let quota =
                                crate::provider::providers::antigravity::usage::query_usage(
                                    &client,
                                    &access_token,
                                )
                                .await
                                .ok();
                            let expires_at_ms =
                                expires_in.filter(|seconds| *seconds > 0).map(|seconds| {
                                    std::time::SystemTime::now()
                                        .duration_since(std::time::UNIX_EPOCH)
                                        .map(|duration| {
                                            duration.as_millis() as i64 + seconds * 1000
                                        })
                                        .unwrap_or(0)
                                });
                            let data = serde_json::json!({
                                "accessToken": access_token,
                                "refreshToken": refresh_token,
                                "projectId": quota.as_ref().map(|q| q.project_id.clone()),
                                "displayName": display_name,
                                "expiresAtMs": expires_at_ms,
                                "quota": quota,
                            });

                            self.inner
                                .state
                                .upsert_resources(
                                    &plugin_id,
                                    &resource_type,
                                    vec![ResourceDraft {
                                        key,
                                        private_data: data,
                                        state: None,
                                    }],
                                )
                                .await?;
                            let model_sync_error = self
                                .sync_models(&plugin_id, "antigravity")
                                .await
                                .err()
                                .map(|error| error.to_string());

                            let _ = request.response.send(oauth_callback::CallbackOutcome {
                                success: true,
                                message: None,
                            });

                            self.inner.oauth_sessions.lock().await.remove(session_id);
                            Ok(OAuthPollResponse {
                                status: "completed".into(),
                                message: None,
                                model_sync_error,
                                poll_interval_ms,
                            })
                        }
                        Err(error_msg) => {
                            let _ = request.response.send(oauth_callback::CallbackOutcome {
                                success: false,
                                message: Some(error_msg.clone()),
                            });
                            self.inner.oauth_sessions.lock().await.remove(session_id);
                            Ok(oauth_poll_status(
                                "failed",
                                Some(error_msg),
                                poll_interval_ms,
                            ))
                        }
                    }
                }
                Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {
                    Ok(oauth_poll_status("pending", None, poll_interval_ms))
                }
                Err(tokio::sync::oneshot::error::TryRecvError::Closed) => {
                    self.inner.oauth_sessions.lock().await.remove(session_id);
                    Ok(oauth_poll_status(
                        "failed",
                        Some("Callback closed".into()),
                        poll_interval_ms,
                    ))
                }
            },
        }
    }
}
