//! Core-owned loopback callback transport for plugin OAuth authorization-code flows.
use std::{net::SocketAddr, sync::Arc, time::Duration};

use axum::{
    extract::{Query, State},
    http::HeaderMap,
    response::Html,
    routing::get,
    Router,
};
use serde::Deserialize;
use tokio::sync::{oneshot, Mutex};
use tokio_util::sync::CancellationToken;

use crate::{Error, Result};

const CALLBACK_RESPONSE_TIMEOUT: Duration = Duration::from_secs(120);

pub(super) struct CallbackRequest {
    pub result: std::result::Result<String, String>,
    pub response: oneshot::Sender<CallbackOutcome>,
}

#[derive(Debug)]
pub(super) struct CallbackOutcome {
    pub success: bool,
    pub message: Option<String>,
}

pub(super) struct CallbackHandle {
    pub redirect_uri: String,
    pub receiver: oneshot::Receiver<CallbackRequest>,
    shutdown: CancellationToken,
}

impl Drop for CallbackHandle {
    fn drop(&mut self) {
        self.shutdown.cancel();
    }
}

#[derive(Clone)]
struct CallbackState {
    expected_state: String,
    plugin_name: String,
    plugin_icon: String,
    resource_name: serde_json::Value,
    sender: Arc<Mutex<Option<oneshot::Sender<CallbackRequest>>>>,
    shutdown: CancellationToken,
}

#[derive(Deserialize)]
struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

pub(super) async fn bind(
    port: Option<u16>,
    path: &str,
    expected_state: String,
    plugin_name: String,
    plugin_icon: String,
    resource_name: serde_json::Value,
) -> Result<CallbackHandle> {
    let address = SocketAddr::from(([127, 0, 0, 1], port.unwrap_or(0)));
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .map_err(|error| {
            Error::Config(format!(
                "cannot bind plugin OAuth callback at {address}: {error}"
            ))
        })?;
    let local_address = listener.local_addr()?;
    let redirect_uri = format!("http://127.0.0.1:{}{path}", local_address.port());
    let (sender, receiver) = oneshot::channel();
    let shutdown = CancellationToken::new();
    let state = CallbackState {
        expected_state,
        plugin_name,
        plugin_icon,
        resource_name,
        sender: Arc::new(Mutex::new(Some(sender))),
        shutdown: shutdown.clone(),
    };
    let router = Router::new()
        .route(path, get(handle_callback))
        .with_state(state);
    let graceful = shutdown.clone();
    tokio::spawn(async move {
        if let Err(error) = axum::serve(listener, router)
            .with_graceful_shutdown(async move { graceful.cancelled().await })
            .await
        {
            tracing::debug!(%error, "plugin OAuth callback server stopped");
        }
    });
    Ok(CallbackHandle {
        redirect_uri,
        receiver,
        shutdown,
    })
}

async fn handle_callback(
    State(state): State<CallbackState>,
    headers: HeaderMap,
    Query(query): Query<CallbackQuery>,
) -> Html<String> {
    let locale = callback_locale(&headers);
    if query.state.as_deref() != Some(state.expected_state.as_str()) {
        return Html(render_page(
            &state,
            locale,
            false,
            Some(localized(
                locale,
                "Authorization state did not match. Return to the app and try again.",
                "Yetkilendirme durumu eşleşmedi. Lütfen uygulamaya dönüp tekrar deneyin.",
                "授权状态不匹配，请返回应用后重试。",
            )),
        ));
    }

    let result = match query.code.filter(|code| !code.trim().is_empty()) {
        Some(code) => Ok(code),
        None => Err(query.error_description.or(query.error).unwrap_or_else(|| {
            localized(
                locale,
                "Authorization was cancelled.",
                "Yetkilendirme iptal edildi.",
                "授权被取消。",
            )
            .to_owned()
        })),
    };
    let Some(sender) = state.sender.lock().await.take() else {
        return Html(render_page(
            &state,
            locale,
            false,
            Some(localized(
                locale,
                "This authorization callback has already been used.",
                "Bu yetkilendirme geri çağrısı zaten kullanıldı.",
                "该授权回调已被使用。",
            )),
        ));
    };
    let (response, completion) = oneshot::channel();
    if sender.send(CallbackRequest { result, response }).is_err() {
        return Html(render_page(
            &state,
            locale,
            false,
            Some(localized(
                locale,
                "The authorization session has ended.",
                "Yetkilendirme oturumu sonlandı.",
                "授权会话已结束。",
            )),
        ));
    }

    let outcome = tokio::time::timeout(CALLBACK_RESPONSE_TIMEOUT, completion).await;
    state.shutdown.cancel();
    match outcome {
        Ok(Ok(outcome)) => Html(render_page(
            &state,
            locale,
            outcome.success,
            outcome.message.as_deref(),
        )),
        _ => Html(render_page(
            &state,
            locale,
            false,
            Some(localized(
                locale,
                "Adding the resource timed out. Return to the app and try again.",
                "Kaynak ekleme zaman aşımına uğradı. Uygulamaya dönüp tekrar deneyin.",
                "添加资源超时，请返回应用后重试。",
            )),
        )),
    }
}

fn callback_locale(headers: &HeaderMap) -> &'static str {
    headers
        .get(axum::http::header::ACCEPT_LANGUAGE)
        .and_then(|value| value.to_str().ok())
        .map(|value| {
            let lower = value.to_ascii_lowercase();
            if lower.starts_with("tr") {
                "tr-TR"
            } else if lower.starts_with("zh") {
                "zh-CN"
            } else if lower.starts_with("pt") {
                "pt-BR"
            } else {
                "en-US"
            }
        })
        .unwrap_or("en-US")
}

fn localized<'a>(locale: &str, english: &'a str, turkish: &'a str, chinese: &'a str) -> &'a str {
    match locale {
        "tr-TR" => turkish,
        "zh-CN" => chinese,
        _ => english,
    }
}

fn localized_value(value: &serde_json::Value, locale: &str) -> String {
    value
        .as_str()
        .map(str::to_owned)
        .or_else(|| {
            value
                .get(locale)
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .or_else(|| {
            value
                .get("en-US")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .or_else(|| {
            value
                .as_object()?
                .values()
                .find_map(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .unwrap_or_else(|| localized(locale, "resource", "kaynak", "资源").to_owned())
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn render_page(
    state: &CallbackState,
    locale: &str,
    success: bool,
    message: Option<&str>,
) -> String {
    let plugin_name = escape_html(&state.plugin_name);
    let plugin_icon = escape_html(&state.plugin_icon);
    let resource_name = escape_html(&localized_value(&state.resource_name, locale));
    let title = if success {
        localized(locale, "Resource added", "Kaynak eklendi", "资源添加成功")
    } else {
        localized(
            locale,
            "Could not add resource",
            "Kaynak eklenemedi",
            "资源添加失败",
        )
    };
    let detail = message.map(escape_html).unwrap_or_else(|| {
        if success {
            localized(
                locale,
                "A resource has been added for this plugin.",
                "Bu eklenti için kaynak eklendi.",
                "已为该插件添加资源。",
            )
            .to_owned()
        } else {
            localized(
                locale,
                "Return to the app and try again.",
                "Lütfen uygulamaya dönüp tekrar deneyin.",
                "请返回应用后重试。",
            )
            .to_owned()
        }
    });
    let close = localized(
        locale,
        "You can now close this page and return to Nexusor.",
        "Artık bu sayfayı kapatıp Nexusor'a dönebilirsiniz.",
        "您现在可以关闭本页面并返回 Nexusor。",
    );

    let icon_html = if state.plugin_icon.trim().is_empty() {
        if success {
            r#"<div style="width:56px;height:56px;margin:0 auto 16px;display:flex;align-items:center;justify-content:center;background:rgba(94,106,210,0.15);border:1px solid rgba(94,106,210,0.3);border-radius:14px;color:#5e6ad2;"><svg width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><polyline points="20 6 9 17 4 12"></polyline></svg></div>"#.to_string()
        } else {
            r#"<div style="width:56px;height:56px;margin:0 auto 16px;display:flex;align-items:center;justify-content:center;background:rgba(239,68,68,0.15);border:1px solid rgba(239,68,68,0.3);border-radius:14px;color:#ef4444;"><svg width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><line x1="18" y1="6" x2="6" y2="18"></line><line x1="6" y1="6" x2="18" y2="18"></line></svg></div>"#.to_string()
        }
    } else {
        format!(
            r#"<img src="{plugin_icon}" alt="" style="width:56px;height:56px;object-fit:contain;margin:0 auto 16px;border-radius:12px;">"#
        )
    };

    format!(
        r#"<!doctype html>
<html lang="{locale}"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; img-src data:; style-src 'unsafe-inline'">
<title>{title}</title><style>
body{{margin:0;min-height:100vh;display:grid;place-items:center;font:15px -apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,sans-serif;background:#0b0c0e;color:#e0e0e0}}
main{{width:min(420px,calc(100vw - 48px));text-align:center;background:#17181c;border:1px solid #222;border-radius:12px;padding:32px 24px;box-sizing:border-box}}
h1{{font-size:20px;font-weight:600;margin:0 0 8px;color:#ffffff}}
.plugin{{font-size:13px;color:#8a8f98;margin-bottom:20px}}
.resource{{font-size:14px;font-weight:600;color:#5e6ad2;margin:8px 0}}
.detail{{font-size:13px;color:#a0a4ab;line-height:1.6}}
.close{{font-size:12px;color:#6b7280;margin-top:24px;padding-top:16px;border-top:1px solid #222}}
</style></head><body><main>{icon_html}<h1>{title}</h1><div class="plugin">{plugin_name}</div><div class="resource">{resource_name}</div><div class="detail">{detail}</div><div class="close">{close}</div></main></body></html>"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_plugin_content_in_callback_page() {
        let state = CallbackState {
            expected_state: "state".into(),
            plugin_name: "<plugin>".into(),
            plugin_icon: "data:image/svg+xml;base64,abc".into(),
            resource_name: serde_json::json!({"en-US": "Accounts & keys"}),
            sender: Arc::new(Mutex::new(None)),
            shutdown: CancellationToken::new(),
        };
        let page = render_page(&state, "en-US", true, None);
        assert!(page.contains("&lt;plugin&gt;"));
        assert!(page.contains("Accounts &amp; keys"));
        assert!(!page.contains("<plugin>"));
    }

    #[tokio::test]
    async fn callback_rejects_wrong_state_then_delivers_code() {
        let mut callback = bind(
            None,
            "/oauth-callback",
            "expected".into(),
            "Plugin".into(),
            "data:image/svg+xml;base64,abc".into(),
            serde_json::json!("Account"),
        )
        .await
        .unwrap();
        let client = reqwest::Client::new();
        let rejected = client
            .get(format!(
                "{}?state=wrong&code=ignored",
                callback.redirect_uri
            ))
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        assert!(rejected.contains("Authorization state did not match"));

        let client = client.clone();
        let redirect_uri = callback.redirect_uri.clone();
        let browser = tokio::spawn(async move {
            client
                .get(format!("{redirect_uri}?state=expected&code=accepted"))
                .send()
                .await
                .unwrap()
                .text()
                .await
                .unwrap()
        });
        let request = (&mut callback.receiver).await.unwrap();
        assert_eq!(request.result.unwrap(), "accepted");
        request
            .response
            .send(CallbackOutcome {
                success: true,
                message: None,
            })
            .unwrap();
        assert!(browser.await.unwrap().contains("Resource added"));
    }
}
