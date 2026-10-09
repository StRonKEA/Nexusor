//! Sends one Provider HTTP attempt without applying retry policy.

use tokio_util::sync::CancellationToken;

use crate::{Error, Result};

use super::CallRecorder;

#[derive(Debug)]
pub(crate) enum Attempt {
    Response(reqwest::Response),
    Cancelled,
}

pub(crate) async fn send_once<F>(
    label: &str,
    build: F,
    cancellation: &CancellationToken,
    recorder: Option<&CallRecorder>,
) -> Result<Attempt>
where
    F: FnOnce() -> reqwest::RequestBuilder,
{
    send_attempt(label, build, cancellation, recorder, None).await
}

/// Bounds only the send/response-header phase; streaming bodies keep their own
/// idle budget and cancellation. A slow stream must not restart generation.
pub(crate) async fn send_once_with_header_timeout<F>(
    label: &str,
    build: F,
    cancellation: &CancellationToken,
    recorder: Option<&CallRecorder>,
    timeout: std::time::Duration,
) -> Result<Attempt>
where
    F: FnOnce() -> reqwest::RequestBuilder,
{
    send_attempt(label, build, cancellation, recorder, Some(timeout)).await
}

async fn send_attempt<F>(
    label: &str,
    build: F,
    cancellation: &CancellationToken,
    recorder: Option<&CallRecorder>,
    header_timeout: Option<std::time::Duration>,
) -> Result<Attempt>
where
    F: FnOnce() -> reqwest::RequestBuilder,
{
    let send = async {
        match header_timeout {
            Some(timeout) => tokio::time::timeout(timeout, build().send())
                .await
                .map_err(|_| {
                    Error::Provider(format!(
                        "{label} response headers timed out after {} ms (before streaming)",
                        timeout.as_millis()
                    ))
                })?
                .map_err(Error::from),
            None => build().send().await.map_err(Error::from),
        }
    };
    let response = tokio::select! {
        _ = cancellation.cancelled() => return Ok(Attempt::Cancelled),
        response = send => response,
    }?;
    if let Some(recorder) = recorder {
        recorder
            .response_headers(response.status().as_u16())
            .await?;
    }
    if response.status().is_success() {
        return Ok(Attempt::Response(response));
    }
    let status = response.status();
    let bytes = tokio::select! {
        _ = cancellation.cancelled() => return Ok(Attempt::Cancelled),
        bytes = response.bytes() => bytes,
    }?;
    Err(Error::Provider(format!(
        "{label} {status}: {}",
        String::from_utf8_lossy(&bytes)
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    async fn server(response: &'static [u8]) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0_u8; 1024];
            let _ = socket.read(&mut request).await;
            socket.write_all(response).await.unwrap();
        });
        format!("http://{address}")
    }

    #[tokio::test]
    async fn non_success_status_is_one_failed_attempt() {
        let url =
            server(b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 4\r\n\r\ndown").await;
        let client = reqwest::Client::new();
        let error = send_once("test", || client.get(&url), &CancellationToken::new(), None)
            .await
            .unwrap_err();
        assert!(
            matches!(error, Error::Provider(message) if message.contains("503") && message.contains("down"))
        );
    }

    #[tokio::test]
    async fn response_body_transport_failure_is_one_failed_attempt() {
        let url =
            server(b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 100\r\n\r\nshort").await;
        let client = reqwest::Client::new();
        let error = send_once("test", || client.get(&url), &CancellationToken::new(), None)
            .await
            .unwrap_err();
        assert!(matches!(error, Error::Http(_)));
    }

    #[tokio::test]
    async fn request_transport_failure_is_one_failed_attempt() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let url = format!("http://{address}");
        let client = reqwest::Client::new();
        let error = send_once("test", || client.get(&url), &CancellationToken::new(), None)
            .await
            .unwrap_err();
        assert!(matches!(error, Error::Http(_)));
    }

    #[tokio::test]
    async fn missing_headers_time_out_and_allow_a_later_endpoint() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let stalled = tokio::spawn(async move {
            let (_socket, _) = listener.accept().await.unwrap();
            std::future::pending::<()>().await;
        });
        let client = reqwest::Client::new();
        let error = send_once_with_header_timeout(
            "test",
            || client.get(&url),
            &CancellationToken::new(),
            None,
            std::time::Duration::from_millis(50),
        )
        .await
        .unwrap_err();
        stalled.abort();
        assert!(
            matches!(error, Error::Provider(message) if message.contains("response headers timed out"))
        );
        let fallback = server(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok").await;
        let result = send_once_with_header_timeout(
            "test",
            || client.get(&fallback),
            &CancellationToken::new(),
            None,
            std::time::Duration::from_secs(2),
        )
        .await
        .unwrap();
        assert!(matches!(result, Attempt::Response(_)));
    }

    #[tokio::test]
    async fn header_timeout_does_not_cut_off_a_streaming_body() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let (release, wait) = tokio::sync::oneshot::channel::<()>();
        let sender = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0; 1024];
            assert!(socket.read(&mut request).await.unwrap() > 0);
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n")
                .await
                .unwrap();
            wait.await.unwrap();
            socket.write_all(b"ok").await.unwrap();
        });
        let client = reqwest::Client::new();
        let Attempt::Response(response) = send_once_with_header_timeout(
            "test",
            || client.get(&url),
            &CancellationToken::new(),
            None,
            std::time::Duration::from_secs(1),
        )
        .await
        .unwrap() else {
            panic!("unexpected cancellation")
        };
        tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
        release.send(()).unwrap();
        assert_eq!(response.text().await.unwrap(), "ok");
        sender.await.unwrap();
    }

    #[tokio::test]
    async fn cancellation_interrupts_header_wait() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let cancellation = CancellationToken::new();
        let cancel = cancellation.clone();
        let server = tokio::spawn(async move {
            let (_socket, _) = listener.accept().await.unwrap();
            cancel.cancel();
            std::future::pending::<()>().await;
        });
        let client = reqwest::Client::new();
        let result = send_once_with_header_timeout(
            "test",
            || client.get(&url),
            &cancellation,
            None,
            std::time::Duration::from_secs(60),
        )
        .await;
        server.abort();
        assert!(matches!(result, Ok(Attempt::Cancelled)));
    }
}
