//! Local Nexusor panel endpoints; never create a transport or an agent run.
use crate::{
    cursor::{
        conversation::{
            local_control::{ActiveLocalRun, ControlRequest, ControlResponse},
            TransportCommand,
        },
        transport::TransportRegistry,
    },
    Error, Result,
};
use axum::{
    extract::{Path, State},
    http::HeaderMap,
    Json,
};

fn check_origin(headers: &HeaderMap) -> Result<()> {
    if headers
        .get("sec-fetch-site")
        .is_some_and(|v| v == "cross-site")
    {
        return Err(Error::Protocol(
            "Cross-site control request rejected".into(),
        ));
    }
    if let Some(origin) = headers.get("origin") {
        let url = url::Url::parse(
            origin
                .to_str()
                .map_err(|_| Error::Protocol("Invalid control origin".into()))?,
        )
        .map_err(|_| Error::Protocol("Invalid control origin".into()))?;
        if !matches!(
            url.host_str(),
            Some("localhost" | "127.0.0.1" | "[::1]" | "tauri.localhost")
        ) {
            return Err(Error::Protocol(
                "Only local control origins are accepted".into(),
            ));
        }
    }
    Ok(())
}

pub async fn list(
    State(registry): State<TransportRegistry>,
    headers: HeaderMap,
) -> Result<Json<Vec<ActiveLocalRun>>> {
    check_origin(&headers)?;
    let mut result = Vec::new();
    let queries = registry
        .local_handles()
        .await
        .into_iter()
        .map(|handle| async move {
            let (reply, receive) = tokio::sync::oneshot::channel();
            let query = async {
                handle
                    .command(TransportCommand::InspectLocal { reply })
                    .await?;
                receive
                    .await
                    .map_err(|_| Error::RunNotFound("Transport closed".into()))
            };
            match tokio::time::timeout(std::time::Duration::from_millis(500), query).await {
                Ok(Ok(run)) => run,
                _ => None,
            }
        });
    use futures_util::StreamExt;
    let mut responses = futures_util::stream::iter(queries).buffer_unordered(16);
    while let Some(run) = responses.next().await {
        if let Some(run) = run {
            result.push(run);
        }
    }
    result.sort_by(|a, b| a.run_id.cmp(&b.run_id));
    Ok(Json(result))
}

pub async fn control(
    State(registry): State<TransportRegistry>,
    Path(request_id): Path<String>,
    headers: HeaderMap,
    Json(request): Json<ControlRequest>,
) -> Result<Json<ControlResponse>> {
    check_origin(&headers)?;
    let handle = registry
        .local(&request_id)
        .await
        .ok_or_else(|| Error::RunNotFound(request_id))?;
    let (reply, receive) = tokio::sync::oneshot::channel();
    let operation = async {
        handle
            .command(TransportCommand::LocalControl { request, reply })
            .await?;
        receive
            .await
            .map_err(|_| Error::RunNotFound("Transport closed".into()))?
    };
    let response = tokio::time::timeout(std::time::Duration::from_secs(5), operation)
        .await
        .map_err(|_| {
            Error::Provider(
                "Control acknowledgement timed out; refresh status before retrying".into(),
            )
        })??;
    Ok(Json(response))
}
