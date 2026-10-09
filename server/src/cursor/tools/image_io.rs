//! GenerateImage persists its own output and reads references through Cursor's
//! permission-aware Read channel.
//!
//! No Cursor program file is modified. File creation is atomic and never
//! clobbers an existing file: `create_new(true)` makes the existence check and
//! the creation a single filesystem operation, so there is no check-then-write
//! race. Cursor still owns permission decisions — the destination is read through
//! Cursor's Read executor *before* any byte is generated, so a denial still
//! prevents the file from being written.
use std::io::Write as _;
use std::path::{Path, PathBuf};

use super::{runtime::CursorToolRuntime, tool_call_result::ToolResultSender};
use crate::{
    cursor::protocol::proto::agent::v1 as pb, model::ToolCall, plugin::PluginRegistry, Error,
    Result,
};
use tokio_util::sync::CancellationToken;

/// Maximum wall time for the whole generate-and-persist operation.
const OPERATION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);
const MAX_REFERENCE_IMAGES: usize = 4;

async fn cursor_read(
    runtime: &CursorToolRuntime,
    results: &ToolResultSender,
    call: &ToolCall,
    path: &str,
    cancellation: &CancellationToken,
) -> Result<pb::ReadResult> {
    let result = runtime
        .image_io(
            results,
            call,
            pb::exec_server_message::Message::ReadArgs(pb::ReadArgs {
                path: path.to_string(),
                tool_call_id: call.call_id.clone(),
                ..Default::default()
            }),
            cancellation,
        )
        .await?;
    match result {
        pb::exec_client_message::Message::ReadResult(result) => Ok(result),
        _ => Err(Error::Protocol(format!("Expected ReadResult for {path}"))),
    }
}

/// Routes the destination through Cursor's permission layer before generation
/// starts. An absent file is fine; a denial is not.
async fn gate_destination(
    runtime: &CursorToolRuntime,
    results: &ToolResultSender,
    call: &ToolCall,
    path: &Path,
    cancellation: &CancellationToken,
) -> Result<()> {
    let target = path.to_string_lossy().into_owned();
    match cursor_read(runtime, results, call, &target, cancellation)
        .await?
        .result
    {
        // Cursor answered, so the request reached the permission layer.
        Some(pb::read_result::Result::Success(_)) => Ok(()),
        Some(pb::read_result::Result::Rejected(rejected)) => Err(Error::Provider(format!(
            "Cursor read of {target} was declined; image not written: {}",
            rejected.reason
        ))),
        Some(pb::read_result::Result::PermissionDenied(denied)) => Err(Error::Provider(format!(
            "Cursor denied access to {target}; image not written: {denied:?}"
        ))),
        // Absent, or present in a form we never overwrite: still gated. The
        // atomic create below reports the real failure for the awkward cases.
        Some(pb::read_result::Result::FileNotFound(_))
        | Some(pb::read_result::Result::InvalidFile(_))
        | Some(pb::read_result::Result::Error(_))
        | None => Ok(()),
    }
}

/// Atomic create-new write. Fails rather than overwriting an existing file.
///
/// `create_new` makes the existence check and the creation one filesystem
/// operation, so there is no check-then-write window. Runs on the blocking pool
/// because the `sync_all` flush can wait on the disk.
async fn persist_noclobber(path: PathBuf, data: Vec<u8>) -> Result<String> {
    let target = path.to_string_lossy().into_owned();
    let written = target.clone();
    tokio::task::spawn_blocking(move || -> std::io::Result<()> {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?;
        file.write_all(&data)?;
        file.sync_all()
    })
    .await
    .map_err(|error| Error::Provider(format!("image write task failed: {error}")))?
    .map_err(|error| match error.kind() {
        std::io::ErrorKind::AlreadyExists => Error::Provider(format!(
            "Refusing to overwrite existing image {written}; generate a new filename instead"
        )),
        _ => Error::Provider(format!("cannot create {written}: {error}")),
    })?;
    Ok(target)
}

pub(crate) async fn generate(
    runtime: &CursorToolRuntime,
    results: &ToolResultSender,
    call: &ToolCall,
    plugins: &PluginRegistry,
    args: &pb::GenerateImageArgs,
    cancellation: &CancellationToken,
) -> Result<crate::provider::image_generation::GeneratedImage> {
    let path = crate::provider::image_generation::destination(args.file_path.as_deref())?;
    if args.reference_image_paths.len() > MAX_REFERENCE_IMAGES {
        return Err(Error::Protocol(format!(
            "GenerateImage accepts at most {MAX_REFERENCE_IMAGES} reference images"
        )));
    }
    if cancellation.is_cancelled() {
        return Err(Error::Cancelled);
    }
    let operation_token = runtime.begin_image_operation(&call.call_id).await;
    let _guard = operation_token.clone().drop_guard();
    let cancellation = &operation_token;
    let operation = async {
        let mut references = Vec::new();
        for path in &args.reference_image_paths {
            if !std::path::Path::new(path).is_absolute()
                || path.starts_with("\\\\")
                || path.starts_with("//")
            {
                return Err(Error::Protocol(
                    "Reference image must be an absolute local path".into(),
                ));
            }
            let result = cursor_read(runtime, results, call, path, cancellation).await?;
            let data = match result.result {
                Some(pb::read_result::Result::Success(success)) => match success.output {
                    Some(pb::read_success::Output::Data(data)) => data,
                    _ => {
                        return Err(Error::Protocol(
                            "Cursor did not return binary reference image".into(),
                        ))
                    }
                },
                Some(pb::read_result::Result::PermissionDenied(_)) => {
                    return Err(Error::Provider(format!(
                        "Cursor reference Read denied access to {path}. Ensure the file exists and is accessible."
                    )))
                }
                Some(pb::read_result::Result::FileNotFound(_)) => {
                    return Err(Error::Provider(format!(
                        "Cursor reference image not found at {path}. Provide an existing absolute path."
                    )))
                }
                other => {
                    return Err(Error::Provider(format!(
                        "Cursor reference Read failed for {path}: {other:?}"
                    )))
                }
            };
            let mime =
                match image::guess_format(&data).map_err(|e| Error::Protocol(e.to_string()))? {
                    image::ImageFormat::Png => "image/png",
                    image::ImageFormat::Jpeg => "image/jpeg",
                    image::ImageFormat::WebP => "image/webp",
                    _ => return Err(Error::Protocol("Unsupported image reference format".into())),
                };
            references.push((data, mime.into()));
        }

        // Permission gate before any provider spend or mutation.
        gate_destination(runtime, results, call, &path, cancellation).await?;

        let data = crate::provider::image_generation::generate_bytes(
            plugins,
            &args.description,
            &references,
            args.aspect_ratio.as_deref(),
            cancellation,
        )
        .await?;
        if cancellation.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let target = persist_noclobber(path.clone(), data.clone()).await?;
        Ok(crate::provider::image_generation::GeneratedImage { path: target, data })
    };
    let result = tokio::select! {
        biased;
        _ = cancellation.cancelled() => Err(Error::Cancelled),
        result = tokio::time::timeout(OPERATION_TIMEOUT, operation) =>
            result.unwrap_or_else(|_| Err(Error::Provider("Cursor image operation exceeded 120s".into()))),
    };
    for id in runtime.abort_image_io(&call.call_id).await {
        let _ = results.send_exec(call.call_id.clone(), super::codec::abort(id));
    }
    runtime.finish_image_operation(&call.call_id).await;
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cursor::tools::{
        codec::{client_event, ClientExecEvent},
        tool_call_result::{tool_result_channel, ToolWorkEvent},
    };

    fn call() -> ToolCall {
        ToolCall {
            call_id: "image-1".into(),
            model_call_id: "model-1".into(),
            index: 0,
            name: "GenerateImage".into(),
            arguments: serde_json::json!({}),
            arguments_text: String::new(),
            argument_error: None,
        }
    }

    #[tokio::test]
    async fn cursor_read_uses_original_tool_identity_and_retains_denials() {
        let runtime = CursorToolRuntime::default();
        let (sender, mut receiver) = tool_result_channel();
        let worker = runtime.clone();
        let task = tokio::spawn(async move {
            worker
                .image_io(
                    &sender,
                    &call(),
                    pb::exec_server_message::Message::ReadArgs(pb::ReadArgs {
                        path: "reference.png".into(),
                        tool_call_id: "image-1".into(),
                        ..Default::default()
                    }),
                    &CancellationToken::new(),
                )
                .await
        });
        let ToolWorkEvent::Exec { call_id, message } = receiver.recv().await.unwrap().unwrap()
        else {
            panic!("expected exec")
        };
        assert_eq!(call_id, "image-1");
        let Some(pb::agent_server_message::Message::ExecServerMessage(exec)) = message.message
        else {
            panic!("expected exec")
        };
        assert!(exec.exec_id.starts_with("image-1:image-io:"));
        let denied = pb::exec_client_message::Message::ReadResult(pb::ReadResult {
            result: Some(pb::read_result::Result::Rejected(pb::ReadRejected {
                path: "reference.png".into(),
                reason: "user declined".into(),
            })),
        });
        let response = pb::ExecClientMessage {
            id: exec.id,
            message: Some(denied.clone()),
            ..Default::default()
        };
        assert!(matches!(
            client_event(&response, &runtime).await.unwrap(),
            ClientExecEvent::Pending
        ));
        assert_eq!(task.await.unwrap().unwrap(), denied);
        assert!(runtime.running_exec_ids().await.is_empty());
    }

    #[tokio::test]
    async fn denied_destination_read_prevents_generation() {
        let runtime = CursorToolRuntime::default();
        let (sender, mut receiver) = tool_result_channel();
        let worker = runtime.clone();
        let handle = tokio::spawn(async move {
            gate_destination(
                &worker,
                &sender,
                &call(),
                Path::new("/tmp/nexusor-does-not-exist.png"),
                &CancellationToken::new(),
            )
            .await
        });
        let ToolWorkEvent::Exec { message, .. } = receiver.recv().await.unwrap().unwrap() else {
            panic!("expected exec")
        };
        let Some(pb::agent_server_message::Message::ExecServerMessage(exec)) = message.message
        else {
            panic!("expected exec")
        };
        let denied = pb::ExecClientMessage {
            id: exec.id,
            message: Some(pb::exec_client_message::Message::ReadResult(
                pb::ReadResult {
                    result: Some(pb::read_result::Result::Rejected(pb::ReadRejected {
                        path: "/tmp/nexusor-does-not-exist.png".into(),
                        reason: "user declined".into(),
                    })),
                },
            )),
            ..Default::default()
        };
        assert!(matches!(
            client_event(&denied, &runtime).await.unwrap(),
            ClientExecEvent::Pending
        ));
        assert!(handle.await.unwrap().is_err());
    }

    #[tokio::test]
    async fn persist_noclobber_creates_once_and_refuses_to_overwrite() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("generated.png");
        assert_eq!(
            persist_noclobber(path.clone(), b"first".to_vec())
                .await
                .unwrap(),
            path.to_string_lossy()
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"first");
        let error = persist_noclobber(path.clone(), b"second".to_vec())
            .await
            .unwrap_err();
        assert!(
            matches!(&error, Error::Provider(message) if message.contains("Refusing to overwrite"))
        );
        // The existing file is untouched.
        assert_eq!(std::fs::read(&path).unwrap(), b"first");
    }

    #[tokio::test]
    async fn cancellation_aborts_waiting_exec_without_completion() {
        let runtime = CursorToolRuntime::default();
        let (sender, mut receiver) = tool_result_channel();
        let worker = runtime.clone();
        let cancellation = CancellationToken::new();
        let child = cancellation.clone();
        let task = tokio::spawn(async move {
            worker
                .image_io(
                    &sender,
                    &call(),
                    pb::exec_server_message::Message::ReadArgs(pb::ReadArgs {
                        path: "output.png".into(),
                        tool_call_id: "image-1".into(),
                        ..Default::default()
                    }),
                    &child,
                )
                .await
        });
        let ToolWorkEvent::Exec { message, .. } = receiver.recv().await.unwrap().unwrap() else {
            panic!("expected exec")
        };
        let Some(pb::agent_server_message::Message::ExecServerMessage(exec)) = message.message
        else {
            panic!("expected exec")
        };
        assert!(runtime.image_exec_is_active(exec.id, "image-1").await);
        cancellation.cancel();
        assert!(matches!(task.await.unwrap(), Err(Error::Cancelled)));
        let ToolWorkEvent::Exec { message, .. } = receiver.recv().await.unwrap().unwrap() else {
            panic!("expected abort")
        };
        assert_eq!(*message, crate::cursor::tools::codec::abort(exec.id));
        assert!(runtime.running_exec_ids().await.is_empty());
        assert!(!runtime.image_exec_is_active(exec.id, "image-1").await);
    }

    #[tokio::test]
    async fn message_interrupt_cancels_inference_gap() {
        let runtime = CursorToolRuntime::default();
        let token = runtime.begin_image_operation("image-1").await;
        assert!(runtime.running_exec_ids().await.is_empty());
        runtime.interrupt_for_message().await;
        assert!(token.is_cancelled());
        assert!(!runtime.local_cancellation.is_cancelled());
    }
}
