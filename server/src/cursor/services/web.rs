//! Cursor web RPCs backed by Nexusor's provider-independent web services.
use axum::{
    body::{to_bytes, Body},
    extract::{Extension, State},
    http::{Request, Response},
};
use prost::Message;

use crate::{
    api::cursor::{handlers::is_byok_model, proxy, CURSOR_MAX_BODY_BYTES},
    cursor::{protocol::connect, transport::TransportRegistry},
    local_app,
    search::{WebFetch, WebSearch},
    Error, Result,
};

#[derive(Clone, PartialEq, Message)]
pub struct RunWebSearchRequest {
    #[prost(string, tag = "1")]
    pub search_term: String,
    #[prost(string, optional, tag = "2")]
    pub explanation: Option<String>,
    #[prost(string, tag = "3")]
    pub model_id: String,
    #[prost(string, optional, tag = "4")]
    pub snapshot_as_of: Option<String>,
}

#[derive(Clone, PartialEq, Message)]
pub struct WebSearchDocument {
    #[prost(string, tag = "1")]
    pub url: String,
    #[prost(string, tag = "2")]
    pub title: String,
    #[prost(string, tag = "3")]
    pub text: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct RunWebSearchResponse {
    #[prost(string, optional, tag = "1")]
    pub answer: Option<String>,
    #[prost(message, repeated, tag = "2")]
    pub documents: Vec<WebSearchDocument>,
}

#[derive(Clone, PartialEq, Message)]
pub struct RunWebFetchRequest {
    #[prost(string, tag = "1")]
    pub url: String,
    #[prost(string, optional, tag = "2")]
    pub snapshot_as_of: Option<String>,
}

#[derive(Clone, PartialEq, Message)]
pub struct RunWebFetchSuccess {
    #[prost(string, tag = "1")]
    pub content: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct RunWebFetchError {
    #[prost(string, tag = "1")]
    pub error: String,
    #[prost(bool, tag = "2")]
    pub is_timeout: bool,
}

#[derive(Clone, PartialEq, Message)]
pub struct RunWebFetchResponse {
    #[prost(message, optional, tag = "1")]
    pub success: Option<RunWebFetchSuccess>,
    #[prost(message, optional, tag = "2")]
    pub error: Option<RunWebFetchError>,
}

pub async fn search(
    State(registry): State<TransportRegistry>,
    Extension(upstream): Extension<proxy::CursorProxy>,
    request: Request<Body>,
) -> Result<Response<Body>> {
    let (parts, body) = request.into_parts();
    let bytes = to_bytes(body, CURSOR_MAX_BODY_BYTES)
        .await
        .map_err(|error| Error::Protocol(format!("cannot read web search request: {error}")))?;
    if !registry.store().cursor_takeover_enabled().await? {
        return proxy::forward(
            Extension(upstream),
            Request::from_parts(parts, Body::from(bytes)),
        )
        .await;
    }
    let input: RunWebSearchRequest = connect::decode_unary(&bytes)?;
    // Same rule as RunGenerateImage: an empty or non-BYOK model cannot be
    // served locally, so it keeps the original wire payload upstream.
    if input.model_id.is_empty() || !is_byok_model(&registry, &input.model_id).await? {
        return proxy::forward(
            Extension(upstream),
            Request::from_parts(parts, Body::from(bytes)),
        )
        .await;
    }
    let search = WebSearch::managed(registry.store().clone());
    let documents = search
        .search(&input.search_term)
        .await
        .map_err(|error| Error::Protocol(error.to_string()))?
        .into_iter()
        .map(|hit| WebSearchDocument {
            url: hit.url,
            title: hit.title,
            text: hit.chunk,
        })
        .collect();
    proto(RunWebSearchResponse {
        answer: None,
        documents,
    })
}

pub async fn fetch(
    State(registry): State<TransportRegistry>,
    Extension(upstream): Extension<proxy::CursorProxy>,
    request: Request<Body>,
) -> Result<Response<Body>> {
    let (parts, body) = request.into_parts();
    // RunWebFetch carries no model id, so the auth token is the only routing
    // signal: local sessions are served by Nexusor's fetcher, everything else
    // (official account or disabled integration) keeps the upstream path.
    if !registry.store().cursor_takeover_enabled().await?
        || !local_app::request_uses_local_cursor_token(&parts.headers)
    {
        return proxy::forward(Extension(upstream), Request::from_parts(parts, body)).await;
    }
    let input: RunWebFetchRequest = connect::decode_unary(
        &to_bytes(body, CURSOR_MAX_BODY_BYTES)
            .await
            .map_err(|error| Error::Protocol(format!("cannot read web fetch request: {error}")))?,
    )?;
    match WebFetch::managed(registry.store().clone(), registry.web_cache().clone())
        .fetch(&input.url)
        .await
    {
        Ok(page) => proto(RunWebFetchResponse {
            success: Some(RunWebFetchSuccess {
                content: page.markdown,
            }),
            error: None,
        }),
        Err(error) => proto(RunWebFetchResponse {
            success: None,
            error: Some(RunWebFetchError {
                error: error.to_string(),
                is_timeout: error.to_string().contains("timed out"),
            }),
        }),
    }
}

fn proto(message: impl Message) -> Result<Response<Body>> {
    let body = message.encode_to_vec();
    Response::builder()
        .header("content-type", "application/proto")
        .body(Body::from(body))
        .map_err(|error| Error::Protocol(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn web_rpc_wire_tags_match_the_installed_cursor_schema() {
        // Tags verified against workbench.glass.main.js (Cursor 3.24.12):
        // RunWebSearchRequest{search_term:1, explanation:2, model_id:3},
        // RunWebSearchResponse{answer:1, documents:2},
        // WebSearchDocument{url:1, title:2, text:3},
        // RunWebFetchRequest{url:1}, RunWebFetchResponse{success:1, error:2},
        // RunWebFetchSuccess{content:1}, RunWebFetchError{error:1, is_timeout:2}.
        let search = RunWebSearchRequest {
            search_term: "fixture".into(),
            explanation: None,
            model_id: String::new(),
            snapshot_as_of: None,
        }
        .encode_to_vec();
        assert_eq!(search, b"\x0a\x07fixture");
        let fetch = RunWebFetchRequest {
            url: "https://example.com".into(),
            snapshot_as_of: None,
        }
        .encode_to_vec();
        assert_eq!(fetch, b"\x0a\x13https://example.com");
        let response = RunWebFetchResponse {
            success: Some(RunWebFetchSuccess {
                content: "hello".into(),
            }),
            error: None,
        };
        let decoded = RunWebFetchResponse::decode(response.encode_to_vec().as_slice()).unwrap();
        assert_eq!(decoded.success.unwrap().content, "hello");
        assert!(decoded.error.is_none());
        let failed = RunWebFetchResponse {
            success: None,
            error: Some(RunWebFetchError {
                error: "request timed out".into(),
                is_timeout: true,
            }),
        };
        let decoded = RunWebFetchResponse::decode(failed.encode_to_vec().as_slice()).unwrap();
        assert!(decoded.success.is_none());
        assert!(decoded.error.unwrap().is_timeout);
    }
}
