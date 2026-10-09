//! Bounded local image-generation tool using the configured Antigravity account pool.
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use base64::{engine::general_purpose::STANDARD, Engine};
use futures_util::StreamExt;
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

use super::providers::{antigravity, quota};
use crate::{plugin::PluginRegistry, Error, Result};

const PLUGIN: &str = "dev.nexusor.plugins.antigravity-auth";
const MODEL: &str = "gemini-3.1-flash-image";
const MODEL_ID: &str =
    "plugin:dev.nexusor.plugins.antigravity-auth/antigravity/gemini-3.1-flash-image";
const MAX_RESPONSE: usize = 24 * 1024 * 1024;

pub(crate) async fn available(plugins: &PluginRegistry) -> bool {
    plugins
        .configured_models()
        .await
        .iter()
        .any(|model| model.id == MODEL_ID)
}

pub(crate) struct GeneratedImage {
    pub path: String,
    pub data: Vec<u8>,
}

pub(crate) fn destination(requested: Option<&str>) -> Result<PathBuf> {
    if let Some(path) = requested.filter(|path| !path.trim().is_empty()) {
        if path.starts_with("\\\\") || path.starts_with("//") {
            return Err(Error::Protocol(
                "GenerateImage requires a local destination, not a network share".into(),
            ));
        }
        let mut path = PathBuf::from(path);
        if !path.is_absolute() {
            if path.components().count() != 1
                || !matches!(
                    path.components().next(),
                    Some(std::path::Component::Normal(_))
                )
                || path.to_string_lossy().contains([':', '/', '\\'])
            {
                return Err(Error::Protocol(
                    "GenerateImage filename must not contain a directory path".into(),
                ));
            }
            let root = crate::config::managed_data_dir()?.join("generated-images");
            std::fs::create_dir_all(&root)?;
            path = root.join(path);
        }
        if path
            .extension()
            .and_then(|s| s.to_str())
            .is_none_or(|s| !s.eq_ignore_ascii_case("png"))
        {
            return Err(Error::Protocol(
                "GenerateImage file_path must be an absolute local .png path".into(),
            ));
        }
        if path.exists() {
            return Err(Error::Protocol(
                "GenerateImage refuses to overwrite an existing file".into(),
            ));
        }
        if !path.parent().is_some_and(Path::is_dir) {
            return Err(Error::Protocol(
                "GenerateImage destination directory does not exist".into(),
            ));
        }
        return Ok(path);
    }
    let root = crate::config::managed_data_dir()?.join("generated-images");
    std::fs::create_dir_all(&root)?;
    Ok(root.join(format!("{}.png", uuid::Uuid::new_v4())))
}

/// Native Cursor RPC supplies reference bytes and owns file persistence itself.
pub(crate) async fn generate_bytes(
    plugins: &PluginRegistry,
    description: &str,
    references: &[(Vec<u8>, String)],
    aspect: Option<&str>,
    cancellation: &CancellationToken,
) -> Result<Vec<u8>> {
    if description.trim().is_empty() || description.len() > 16_000 {
        return Err(Error::Protocol(
            "GenerateImage description must contain 1–16000 bytes".into(),
        ));
    }
    if references.len() > 4 {
        return Err(Error::Protocol(
            "GenerateImage accepts at most four reference images".into(),
        ));
    }
    let aspect = aspect.unwrap_or("1:1");
    if ![
        "1:1", "2:3", "3:2", "3:4", "4:3", "4:5", "5:4", "9:16", "16:9", "21:9",
    ]
    .contains(&aspect)
    {
        return Err(Error::Protocol(
            "GenerateImage aspect_ratio is unsupported".into(),
        ));
    }
    let mut parts = vec![json!({"text":description})];
    for (data, mime) in references {
        if data.len() > 8 * 1024 * 1024 {
            return Err(Error::Protocol("Reference image exceeds 8 MiB".into()));
        }
        let part = reference_part(data)?;
        if part["inlineData"]["mimeType"].as_str() != Some(mime.as_str()) {
            return Err(Error::Protocol(
                "Reference image MIME type does not match its bytes".into(),
            ));
        }
        parts.push(part);
    }
    tokio::select! {
        biased;
        _ = cancellation.cancelled() => Err(Error::Cancelled),
        result = tokio::time::timeout(Duration::from_secs(120), generate_parts(plugins, &parts, aspect, cancellation)) => {
            result.map_err(|_| Error::Provider("image generation exceeded 120s".into()))?
        }
    }
}

async fn generate_parts(
    plugins: &PluginRegistry,
    parts: &[Value],
    aspect: &str,
    cancellation: &CancellationToken,
) -> Result<Vec<u8>> {
    if !available(plugins).await {
        return Err(Error::Provider(
            "Enable the configured Antigravity gemini-3.1-flash-image model first".into(),
        ));
    }
    let excluded = image_excluded_accounts(plugins).await?;
    generate_from_pool(plugins, parts, aspect, excluded, cancellation).await
}

async fn image_excluded_accounts(plugins: &PluginRegistry) -> Result<Vec<String>> {
    let now = crate::store::now_ms();
    Ok(plugins
        .resources(PLUGIN, antigravity::RESOURCE_TYPE)
        .await?
        .into_iter()
        .filter(|record| {
            quota::antigravity_model_is_cooling(&record.private_data, MODEL, now)
                || ["gemini", "gemini_weekly"].iter().any(|window| {
                    let q = &record.private_data["quota"][window];
                    let threshold = if *window == "gemini_weekly" { 1.0 } else { 0.0 };
                    q["remaining_percent"]
                        .as_f64()
                        .is_some_and(|v| v <= threshold)
                        && q["reset_at_ms"].as_i64().unwrap_or(i64::MAX) > now
                })
        })
        .map(|record| record.id)
        .collect())
}

async fn generate_from_pool(
    plugins: &PluginRegistry,
    parts: &[Value],
    aspect: &str,
    mut excluded: Vec<String>,
    cancellation: &CancellationToken,
) -> Result<Vec<u8>> {
    let strategy = plugins.pool_strategy(PLUGIN).await?;
    let client = plugins.client().await?;
    let urls = antigravity::provider::stream_urls();
    #[cfg(test)]
    let urls = plugins.antigravity_test_urls().await.unwrap_or(urls);
    let mut previous_error = None;
    loop {
        if cancellation.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let record = match plugins
            .select_resource(PLUGIN, antigravity::RESOURCE_TYPE, &excluded)
            .await
        {
            Ok(record) => record,
            Err(error) => return Err(previous_error.unwrap_or(error)),
        };
        let mut account = antigravity::AntigravityAccountData::from_record(&record)
            .ok_or_else(|| Error::Provider("invalid image-generation account".into()))?;
        if antigravity::ensure_fresh_account(&client, &mut account).await? {
            plugins
                .persist_antigravity_account(
                    PLUGIN,
                    antigravity::RESOURCE_TYPE,
                    &record.id,
                    &account,
                )
                .await?;
        }
        let project = account
            .project_id
            .as_deref()
            .filter(|p| !p.trim().is_empty())
            .ok_or_else(|| {
                Error::Provider("image-generation account needs project refresh".into())
            })?;
        let body = json!({"project":project,"model":MODEL,"request":{
            "contents":[{"role":"user","parts":parts}],
            "generationConfig":{"responseModalities":["TEXT","IMAGE"],"imageConfig":{"aspectRatio":aspect,"imageSize":"512"}}
        }});
        tracing::info!(account_id = %record.id, model = MODEL, "image generation account selected");
        let mut primary_error = None;
        let mut last_error = None;
        for (index, url) in urls.iter().enumerate() {
            if cancellation.is_cancelled() {
                return Err(Error::Cancelled);
            }
            let mut request = client.post(url).json(&body);
            for (name, value) in antigravity::provider::request_headers(&account.access_token) {
                request = request.header(name, value);
            }
            // The outer 120s budget still bounds all endpoints/accounts combined.
            let sent = tokio::select! {
                biased;
                _ = cancellation.cancelled() => return Err(Error::Cancelled),
                result = tokio::time::timeout(Duration::from_secs(120), request.send()) => result,
            };
            let response = match sent {
                Ok(Ok(response)) => response,
                // A timeout/disconnect does not prove that generation was rejected.
                // Do not issue a duplicate generation on an ambiguous transport failure.
                Ok(Err(error)) => return Err(Error::Http(error)),
                Err(_) => {
                    return Err(Error::Provider(
                        "image response headers timed out after 120s".into(),
                    ))
                }
            };
            if response.status().is_success() {
                // Once accepted, body/decode errors are terminal, not a new generation.
                let status = response.status();
                let content_type = response
                    .headers()
                    .get(reqwest::header::CONTENT_TYPE)
                    .and_then(|value| value.to_str().ok())
                    .unwrap_or("missing")
                    .chars()
                    .take(128)
                    .collect::<String>();
                let bytes = read_image_response(response, cancellation).await?;
                let result = decode_image(&bytes);
                if result.is_err() {
                    tracing::warn!(account_id = %record.id, endpoint = %url, %status, %content_type,
                    response_bytes = bytes.len(), summary = %image_response_summary(&bytes),
                    "image response could not be decoded");
                }
                return result;
            }
            let status = response.status();
            let bytes = read_image_response(response, cancellation).await?;
            let error = Error::Provider(format!(
                "image generation {status}: {}",
                String::from_utf8_lossy(&bytes)
            ));
            tracing::warn!(account_id = %record.id, endpoint = %url, %status, "image endpoint rejected generation");
            if index == 0 && image_account_failure(&error) {
                primary_error = Some(error);
            } else {
                last_error = Some(error);
            }
        }
        let error = primary_error
            .or(last_error)
            .unwrap_or_else(|| Error::Provider("image generation has no endpoint".into()));
        if strategy == crate::plugin::PoolStrategy::Single || !image_account_failure(&error) {
            return Err(error);
        }
        if quota::is_account_quota_error("antigravity", &error.to_string()) {
            plugins
                .cool_model_resource(
                    PLUGIN,
                    antigravity::RESOURCE_TYPE,
                    &record.id,
                    MODEL,
                    quota::quota_cooldown("antigravity", &error.to_string()),
                )
                .await?;
        }
        tracing::warn!(account_id = %record.id, "image generation failed before response; trying another account");
        excluded.push(record.id);
        previous_error = Some(error);
    }
}

fn image_account_failure(error: &Error) -> bool {
    let text = error.to_string();
    quota::is_account_quota_error("antigravity", &text)
        || quota::is_transient_capacity_error("antigravity", &text)
}

async fn read_image_response(
    response: reqwest::Response,
    cancellation: &CancellationToken,
) -> Result<Vec<u8>> {
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = tokio::select! {
        biased;
        _ = cancellation.cancelled() => return Err(Error::Cancelled),
        chunk = stream.next() => chunk,
    } {
        let chunk = chunk?;
        if bytes.len().saturating_add(chunk.len()) > MAX_RESPONSE {
            return Err(Error::Provider("image response exceeds 24 MiB".into()));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn reference_part(data: &[u8]) -> Result<Value> {
    let format = image::guess_format(data).map_err(|e| Error::Protocol(e.to_string()))?;
    let mime = match format {
        image::ImageFormat::Png => "image/png",
        image::ImageFormat::Jpeg => "image/jpeg",
        image::ImageFormat::WebP => "image/webp",
        _ => {
            return Err(Error::Protocol(
                "Reference must be PNG, JPEG or WebP".into(),
            ))
        }
    };
    let reader = image::ImageReader::new(std::io::Cursor::new(data)).with_guessed_format()?;
    let (width, height) = reader
        .into_dimensions()
        .map_err(|e| Error::Protocol(e.to_string()))?;
    if width == 0 || height == 0 || width > 4096 || height > 4096 {
        return Err(Error::Protocol(
            "reference image dimensions exceed 4096px".into(),
        ));
    }
    image::load_from_memory_with_format(data, format)
        .map_err(|e| Error::Protocol(e.to_string()))?;
    Ok(json!({"inlineData":{"mimeType":mime,"data":STANDARD.encode(data)}}))
}

/// Structural diagnostics only: never copy generated text, image bytes or prompts.
fn image_response_summary(bytes: &[u8]) -> String {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return "format=non_utf8".into();
    };
    let mut events = 0usize;
    let mut invalid_json = 0usize;
    let mut candidates = 0usize;
    let mut text_parts = 0usize;
    let mut inline_parts = 0usize;
    let mut thought_parts = 0usize;
    let mut errors = 0usize;
    let mut blocked = false;
    let mut done = false;
    for line in text.lines() {
        let Some(data) = line.strip_prefix("data:") else {
            continue;
        };
        if data.trim() == "[DONE]" {
            done = true;
            continue;
        }
        events += 1;
        let Ok(value) = serde_json::from_str::<Value>(data) else {
            invalid_json += 1;
            continue;
        };
        let response = value.get("response").unwrap_or(&value);
        errors += usize::from(
            response
                .get("error")
                .or_else(|| value.get("error"))
                .is_some(),
        );
        blocked |= response.pointer("/promptFeedback/blockReason").is_some()
            || response.pointer("/prompt_feedback/block_reason").is_some();
        for candidate in response["candidates"].as_array().into_iter().flatten() {
            candidates += 1;
            for part in candidate["content"]["parts"]
                .as_array()
                .into_iter()
                .flatten()
            {
                text_parts += usize::from(part.get("text").is_some());
                inline_parts += usize::from(
                    part.get("inlineData")
                        .or_else(|| part.get("inline_data"))
                        .is_some(),
                );
                thought_parts += usize::from(part["thought"].as_bool() == Some(true));
            }
        }
    }
    let format = if events > 0 || done {
        "sse"
    } else if text.trim().is_empty() {
        "empty"
    } else if serde_json::from_str::<Value>(text).is_ok() {
        "json_without_sse"
    } else {
        "non_sse"
    };
    format!("format={format} events={events} invalid_json={invalid_json} candidates={candidates} text_parts={text_parts} inline_parts={inline_parts} thought_parts={thought_parts} errors={errors} prompt_block_field={blocked} done={done}")
}

fn decode_image(bytes: &[u8]) -> Result<Vec<u8>> {
    let mut generated = None;
    for line in std::str::from_utf8(bytes)
        .map_err(|e| Error::Provider(e.to_string()))?
        .lines()
    {
        let Some(data) = line.strip_prefix("data:") else {
            continue;
        };
        if data.trim() == "[DONE]" {
            continue;
        }
        let value: Value = serde_json::from_str(data)?;
        let response = value.get("response").unwrap_or(&value);
        if let Some(error) = response.get("error").or_else(|| value.get("error")) {
            return Err(Error::Provider(format!("image generation: {error}")));
        }
        for part in response["candidates"]
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|candidate| {
                candidate["content"]["parts"]
                    .as_array()
                    .into_iter()
                    .flatten()
            })
        {
            if part["thought"].as_bool() == Some(true) {
                continue;
            }
            let Some(inline) = part.get("inlineData").or_else(|| part.get("inline_data")) else {
                continue;
            };
            if generated.is_some() {
                continue;
            }
            let encoded = inline["data"]
                .as_str()
                .ok_or_else(|| Error::Provider("image result missing data".into()))?;
            let data = STANDARD
                .decode(encoded)
                .map_err(|e| Error::Provider(e.to_string()))?;
            let reader =
                image::ImageReader::new(std::io::Cursor::new(&data)).with_guessed_format()?;
            let (width, height) = reader
                .into_dimensions()
                .map_err(|e| Error::Provider(e.to_string()))?;
            if width == 0 || height == 0 || width > 4096 || height > 4096 {
                return Err(Error::Provider(
                    "generated image dimensions exceed 4096px".into(),
                ));
            }
            let decoded =
                image::load_from_memory(&data).map_err(|e| Error::Provider(e.to_string()))?;
            let mut png = std::io::Cursor::new(Vec::new());
            decoded
                .write_to(&mut png, image::ImageFormat::Png)
                .map_err(|e| Error::Provider(e.to_string()))?;
            if png.get_ref().len() > 16 * 1024 * 1024 {
                return Err(Error::Provider("generated PNG exceeds 16 MiB".into()));
            }
            generated = Some(png.into_inner());
        }
    }
    generated.ok_or_else(|| {
        Error::Provider(format!(
            "provider returned no generated image ({})",
            image_response_summary(bytes)
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_image_diagnostics_distinguish_shapes_without_exposing_content() {
        let response = format!(
            "data: {}\n\ndata: [DONE]\n\n",
            json!({"response":{
                "promptFeedback":{"blockReason":"PRIVATE_REASON"},
                "candidates":[{"content":{"parts":[{"text":"PRIVATE_TEXT"},
                    {"thought":true,"inlineData":{"data":"PRIVATE_IMAGE"}}]}}]
            }})
        );
        let summary = image_response_summary(response.as_bytes());
        assert!(summary.contains("format=sse events=1"));
        assert!(summary.contains("text_parts=1 inline_parts=1 thought_parts=1"));
        assert!(summary.contains("prompt_block_field=true done=true"));
        assert!(!summary.contains("PRIVATE"));
        let error = decode_image(response.as_bytes()).unwrap_err().to_string();
        assert!(error.contains("provider returned no generated image"));
        assert!(!error.contains("PRIVATE"));
        for (bytes, expected) in [
            (&b""[..], "format=empty"),
            (&b"{\"message\":\"PRIVATE\"}"[..], "format=json_without_sse"),
            (&b"<html>PRIVATE</html>"[..], "format=non_sse"),
            (&b"\xff"[..], "format=non_utf8"),
            (&b"data: not-json\n\n"[..], "invalid_json=1"),
        ] {
            let summary = image_response_summary(bytes);
            assert!(summary.contains(expected), "{summary}");
            assert!(!summary.contains("PRIVATE"));
        }
    }

    #[tokio::test]
    async fn image_failover_preserves_quota_scope_single_policy_and_late_errors() {
        use crate::{
            plugin::{PoolStrategy, ResourceRecord, ResourceState},
            store::Store,
        };
        use axum::{
            extract::State,
            http::{HeaderMap, StatusCode},
            response::IntoResponse,
            routing::post,
            Router,
        };
        use std::sync::Arc;
        use tokio::sync::Mutex;
        #[derive(Clone)]
        struct StateData {
            mode: &'static str,
            image: String,
            seen: Arc<Mutex<Vec<String>>>,
        }
        async fn primary(
            State(s): State<StateData>,
            headers: HeaderMap,
        ) -> axum::response::Response {
            let account = headers["authorization"].to_str().unwrap().to_owned();
            s.seen.lock().await.push(format!("primary:{account}"));
            if account == "Bearer b" || s.mode == "late" {
                let tail = if s.mode == "late" {
                    "data: {\"error\":{\"code\":503,\"message\":\"late image error\"}}\n\n"
                } else {
                    ""
                };
                return (
                    [("content-type", "text/event-stream")],
                    format!("{}{tail}", s.image),
                )
                    .into_response();
            }
            if s.mode == "capacity" {
                return (StatusCode::SERVICE_UNAVAILABLE, "no capacity available").into_response();
            }
            (StatusCode::TOO_MANY_REQUESTS,r#"{"error":{"status":"RESOURCE_EXHAUSTED","details":[{"reason":"QUOTA_EXHAUSTED"},{"@type":"type.googleapis.com/google.rpc.RetryInfo","retryDelay":"300s"}]}}"#).into_response()
        }
        async fn fallback(
            State(s): State<StateData>,
            headers: HeaderMap,
        ) -> axum::response::Response {
            s.seen.lock().await.push(format!(
                "fallback:{}",
                headers["authorization"].to_str().unwrap()
            ));
            if s.mode == "endpoint" {
                ([("content-type", "text/event-stream")], s.image).into_response()
            } else {
                StatusCode::NOT_FOUND.into_response()
            }
        }
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(2, 2)
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        let image = format!(
            "data: {}\n\n",
            json!({"response":{"candidates":[{"content":{"parts":[{"inlineData":{"mimeType":"image/png","data":STANDARD.encode(png.get_ref())}}]}}]}})
        );
        for mode in [
            "quota",
            "capacity",
            "single",
            "late",
            "endpoint",
            "cancelled",
        ] {
            let seen = Arc::new(Mutex::new(Vec::new()));
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let base = format!("http://{}", listener.local_addr().unwrap());
            let app = Router::new()
                .route("/primary", post(primary))
                .route("/fallback", post(fallback))
                .with_state(StateData {
                    mode,
                    image: image.clone(),
                    seen: seen.clone(),
                });
            let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
            let root = tempfile::tempdir().unwrap();
            let registry = PluginRegistry::for_test(
                Store::connect("sqlite::memory:").await.unwrap(),
                root.path(),
            );
            registry
                .set_antigravity_test_urls(vec![
                    format!("{base}/primary"),
                    format!("{base}/fallback"),
                ])
                .await;
            registry
                .set_pool_strategy(
                    PLUGIN,
                    if mode == "single" {
                        PoolStrategy::Single
                    } else {
                        PoolStrategy::Failover
                    },
                )
                .await
                .unwrap();
            let now = crate::store::now_ms();
            registry.restore_resources(PLUGIN,antigravity::RESOURCE_TYPE,["a","b"].into_iter().map(|id|ResourceRecord{
                id:id.into(),key:id.into(),state:ResourceState::Ready,
                private_data:json!({"accessToken":id,"expiresAtMs":now+3_600_000,"projectId":"test"}),
                created_at_ms:now,updated_at_ms:now,
            }).collect()).await.unwrap();
            let cancellation = CancellationToken::new();
            if mode == "cancelled" {
                cancellation.cancel();
            }
            let result = generate_from_pool(
                &registry,
                &[json!({"text":"fixture"})],
                "1:1",
                vec![],
                &cancellation,
            )
            .await;
            let records = registry
                .resources(PLUGIN, antigravity::RESOURCE_TYPE)
                .await
                .unwrap();
            if ["quota", "capacity", "endpoint"].contains(&mode) {
                assert!(result.unwrap().starts_with(b"\x89PNG"));
            } else {
                assert!(result.is_err(), "{mode}");
            }
            let expected = match mode {
                "cancelled" => vec![],
                "late" => vec!["primary:Bearer a"],
                "single" | "endpoint" => vec!["primary:Bearer a", "fallback:Bearer a"],
                _ => vec!["primary:Bearer a", "fallback:Bearer a", "primary:Bearer b"],
            };
            assert_eq!(*seen.lock().await, expected, "{mode}");
            let first = records.iter().find(|r| r.id == "a").unwrap();
            assert!(matches!(first.state, ResourceState::Ready));
            assert!(first.private_data["modelFamilyCooldowns"]["claude"].is_null());
            assert_eq!(
                first.private_data["modelFamilyCooldowns"]["gemini"]
                    .as_i64()
                    .is_some(),
                mode == "quota"
            );
            if mode == "quota" {
                assert!(
                    first.private_data["modelFamilyCooldowns"]["gemini"]
                        .as_i64()
                        .unwrap()
                        >= now + 360_000
                );
            }
            assert!(records.iter().find(|r| r.id == "b").unwrap().private_data
                ["modelFamilyCooldowns"]
                .is_null());
            server.abort();
        }
    }
    #[test]
    fn image_response_requires_real_pixels_and_ignores_thought_images() {
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(2, 2)
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        let response = json!({"response":{"candidates":[{"content":{"parts":[
            {"thought":true,"inlineData":{"data":"bad"}},
            {"inlineData":{"mimeType":"image/png","data":STANDARD.encode(png.get_ref())}}
        ]}}]}});
        assert!(decode_image(format!("data: {response}\n\n").as_bytes())
            .unwrap()
            .starts_with(b"\x89PNG"));
        assert!(decode_image(b"data: {\"candidates\":[]}\n").is_err());
        assert!(decode_image(b"data: {\"error\":{\"code\":429}}\n").is_err());
        let trailing_error = format!("data: {response}\n\ndata: {{\"error\":{{\"code\":503,\"message\":\"failed after image\"}}}}\n\n");
        let error = decode_image(trailing_error.as_bytes())
            .expect_err("late provider failure must not save an image");
        assert!(error.to_string().contains("failed after image"));
        assert!(decode_image(format!("data: {response}\n\ndata: [DONE]\n\n").as_bytes()).is_ok());
    }

    #[test]
    fn reference_validation_preserves_bytes_and_rejects_corrupt_images() {
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(2, 2)
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        let part = reference_part(png.get_ref()).unwrap();
        assert_eq!(part["inlineData"]["mimeType"], "image/png");
        assert_eq!(
            STANDARD
                .decode(part["inlineData"]["data"].as_str().unwrap())
                .unwrap(),
            *png.get_ref()
        );
        assert!(reference_part(b"\x89PNG\r\n\x1a\ncorrupt").is_err());
        let mut oversized = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(4097, 1)
            .write_to(&mut oversized, image::ImageFormat::Png)
            .unwrap();
        assert!(reference_part(oversized.get_ref()).is_err());
    }

    #[test]
    fn output_path_never_overwrites_or_guesses_a_workspace() {
        assert!(destination(Some("../relative.png")).is_err());
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("existing.png");
        std::fs::write(&path, b"user-data").unwrap();
        assert!(destination(path.to_str()).is_err());
        assert_eq!(std::fs::read(path).unwrap(), b"user-data");
        assert!(destination(root.path().join("new.png").to_str()).is_ok());
    }
}
