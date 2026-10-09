//! Cursor local-tool image RPC; inference is local BYOK, persistence stays in Cursor.
use crate::{
    api::cursor::{handlers::is_byok_model, proxy, CURSOR_MAX_BODY_BYTES},
    cursor::{protocol::connect, transport::TransportRegistry},
    Error, Result,
};
use axum::{
    body::{to_bytes, Body},
    extract::{Extension, State},
    http::{header, Request, Response},
};
use base64::{engine::general_purpose::STANDARD, Engine};
use prost::Message;
use tokio_util::sync::CancellationToken;

// Wire tags verified against the installed Cursor 3.22.12 host schema.
#[derive(Clone, PartialEq, Message)]
pub(crate) struct ImageRequest {
    #[prost(string, tag = "1")]
    pub description: String,
    #[prost(message, repeated, tag = "2")]
    pub reference_images: Vec<ReferenceImage>,
    #[prost(string, tag = "3")]
    pub model_id: String,
    #[prost(bool, tag = "4")]
    pub max_mode: bool,
    #[prost(string, optional, tag = "5")]
    pub aspect_ratio: Option<String>,
}
#[derive(Clone, PartialEq, Message)]
pub(crate) struct ReferenceImage {
    #[prost(string, tag = "1")]
    pub data: String,
    #[prost(string, tag = "2")]
    pub mime_type: String,
}
#[derive(Clone, PartialEq, Message)]
pub(crate) struct ImageResponse {
    #[prost(oneof = "image_response::Outcome", tags = "1,2")]
    pub result: Option<image_response::Outcome>,
}
pub(crate) mod image_response {
    #[derive(Clone, PartialEq, prost::Oneof)]
    pub enum Outcome {
        #[prost(message, tag = "1")]
        Success(super::ImageSuccess),
        #[prost(message, tag = "2")]
        Error(super::ImageError),
    }
}
#[derive(Clone, PartialEq, Message)]
pub(crate) struct ImageSuccess {
    #[prost(string, tag = "1")]
    pub image_data: String,
    #[prost(string, tag = "2")]
    pub mime_type: String,
}
#[derive(Clone, PartialEq, Message)]
pub(crate) struct ImageError {
    #[prost(string, tag = "1")]
    pub error: String,
    #[prost(bool, tag = "2")]
    pub model_restricted: bool,
    #[prost(int32, optional, tag = "3")]
    pub provider_status_code: Option<i32>,
    #[prost(bool, tag = "4")]
    pub content_safety_blocked: bool,
}

pub async fn generate(
    State(registry): State<TransportRegistry>,
    Extension(upstream): Extension<proxy::CursorProxy>,
    request: Request<Body>,
) -> Result<Response<Body>> {
    let (parts, body) = request.into_parts();
    let bytes = to_bytes(body, CURSOR_MAX_BODY_BYTES)
        .await
        .map_err(|e| Error::Protocol(format!("cannot read image RPC body: {e}")))?;
    // A disabled integration must pass through even future/unknown payloads.
    if !registry.store().cursor_takeover_enabled().await? {
        return proxy::forward(
            Extension(upstream),
            Request::from_parts(parts, Body::from(bytes)),
        )
        .await;
    }
    let input: ImageRequest = connect::decode_unary(&bytes)?;
    if input.model_id.is_empty() || !is_byok_model(&registry, &input.model_id).await? {
        return proxy::forward(
            Extension(upstream),
            Request::from_parts(parts, Body::from(bytes)),
        )
        .await;
    }
    let cancellation = CancellationToken::new();
    let _guard = cancellation.clone().drop_guard();
    let timeout = parts
        .headers
        .get("connect-timeout-ms")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(120_000)
        .min(120_000);
    let result = tokio::time::timeout(std::time::Duration::from_millis(timeout), async {
        let references = decode_references(&input.reference_images)?;
        let plugins = registry
            .plugins()
            .ok_or_else(|| Error::Provider("image generator is unavailable".into()))?;
        crate::provider::image_generation::generate_bytes(
            plugins,
            &input.description,
            &references,
            input.aspect_ratio.as_deref(),
            &cancellation,
        )
        .await
    })
    .await
    .unwrap_or_else(|_| Err(Error::Provider("image RPC deadline exceeded".into())));
    let outcome = match result {
        Ok(data) => image_response::Outcome::Success(ImageSuccess {
            image_data: STANDARD.encode(data),
            mime_type: "image/png".into(),
        }),
        Err(error) => image_response::Outcome::Error(ImageError {
            error: error.to_string(),
            model_restricted: false,
            provider_status_code: None,
            content_safety_blocked: false,
        }),
    };
    Response::builder()
        .header(header::CONTENT_TYPE, "application/proto")
        .body(Body::from(
            ImageResponse {
                result: Some(outcome),
            }
            .encode_to_vec(),
        ))
        .map_err(|e| Error::Protocol(e.to_string()))
}

fn decode_references(references: &[ReferenceImage]) -> Result<Vec<(Vec<u8>, String)>> {
    if references.len() > 4 {
        return Err(Error::Protocol(
            "GenerateImage accepts at most four reference images".into(),
        ));
    }
    references
        .iter()
        .map(|reference| {
            if reference.data.len() > 11_184_812 {
                return Err(Error::Protocol("Reference image exceeds 8 MiB".into()));
            }
            let data = STANDARD
                .decode(&reference.data)
                .map_err(|_| Error::Protocol("Reference image contains invalid base64".into()))?;
            if data.len() > 8 * 1024 * 1024 {
                return Err(Error::Protocol("Reference image exceeds 8 MiB".into()));
            }
            Ok((data, reference.mime_type.clone()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_rpc_references_reject_invalid_base64_and_excess_count() {
        let invalid = ReferenceImage {
            data: "not base64".into(),
            mime_type: "image/png".into(),
        };
        assert!(decode_references(&[invalid]).is_err());
        let valid = ReferenceImage {
            data: STANDARD.encode(b"fixture"),
            mime_type: "image/png".into(),
        };
        assert_eq!(
            decode_references(std::slice::from_ref(&valid)).unwrap()[0].0,
            b"fixture"
        );
        assert!(decode_references(&vec![valid; 5]).is_err());
    }
}
