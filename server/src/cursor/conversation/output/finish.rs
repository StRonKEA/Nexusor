use super::*;

pub(crate) fn cursor_error(failure: RunFailure) -> Error {
    match failure {
        RunFailure::Protocol(message) => Error::Protocol(message),
        RunFailure::Provider(message) => Error::Provider(translate_user_friendly_error(&message)),
        RunFailure::Store(message) => Error::Store(message),
        RunFailure::Client(message) => Error::Protocol(message),
    }
}

pub fn translate_user_friendly_error(message: &str) -> String {
    let lower = message.to_ascii_lowercase();

    if lower.contains("cooling down") {
        if let Some(pos) = message.find("available in ") {
            let rest = &message[pos + "available in ".len()..];
            let secs = rest
                .split(|c: char| !c.is_numeric())
                .next()
                .unwrap_or("birkaç");
            return format!(
                "Nexusor: Bu model için hesap kotaları doldu ve soğuma sürecinde. En yakın hesap yaklaşık {secs} saniye sonra tekrar hazır olacak. Lütfen bekleyin veya başka bir modele geçin."
            );
        }
        return "Nexusor: Model kotası doldu ve soğuma sürecinde. Lütfen birkaç dakika sonra tekrar deneyin veya alternatif bir modele geçin.".into();
    }

    if lower.contains("no available") || lower.contains("all accounts currently unavailable") {
        return "Nexusor: Bu model için hazır hesap bulunmuyor veya bağlı tüm hesaplar şu an soğumada. Lütfen Sağlayıcılar sekmesinden hesap durumunu kontrol edin.".into();
    }

    if lower.contains("combo exhausted") {
        return "Nexusor: Combo içerisindeki tüm modeller denendi ancak kota veya bağlantı nedeniyle yanıt alınamadı. Lütfen sağlayıcı hesaplarınızı kontrol edin.".into();
    }

    message.to_string()
}

pub(crate) fn accept_tool_completion(
    delivery: CommandResult,
    request_id: &str,
    run_id: &str,
    call_id: &str,
) -> Result<bool> {
    match delivery {
        CommandResult::Applied | CommandResult::Duplicate => Ok(true),
        CommandResult::RunClosing | CommandResult::RunEnded => {
            tracing::warn!(
                request_id,
                run_id,
                call_id,
                ?delivery,
                "ignoring ToolCompletion delivered after Run stopped accepting results"
            );
            Ok(false)
        }
        CommandResult::StaleTarget => Err(Error::RunNotFound(request_id.into())),
    }
}

pub(crate) fn finish_success(handle: &TransportHandle) {
    handle.emit_frame(crate::cursor::protocol::connect::encode_end_stream());
    handle.close_output();
}

pub(crate) fn finish_failed(handle: &TransportHandle, error: &Error) -> Result<()> {
    use crate::cursor::protocol::connect::{
        encode_end_stream, encode_error_end_stream, ConnectCode, ConnectErrorDetail,
        ConnectStreamError,
    };
    use crate::cursor::protocol::proto::aiserver::v1 as ai;

    let plain = |code, message| ConnectStreamError {
        code,
        message,
        details: Vec::new(),
    };
    let stream_error = match error {
        Error::Provider(_) | Error::Http(_) => {
            let detail = ai::ErrorDetails {
                error: ai::error_details::Error::ProviderError as i32,
                details: Some(ai::CustomErrorDetails {
                    title: "Provider Error".into(),
                    detail: error.to_string(),
                    allow_command_links_potentially_unsafe_please_only_use_for_handwritten_trusted_markdown: Some(true),
                    is_retryable: Some(true),
                    show_request_id: Some(true),
                    should_show_immediate_error: Some(false),
                }),
                is_expected: Some(true),
            };
            ConnectStreamError {
                code: ConnectCode::Unavailable,
                message: error.to_string(),
                details: vec![ConnectErrorDetail {
                    type_name: "aiserver.v1.ErrorDetails".into(),
                    value: STANDARD_NO_PAD.encode(detail.encode_to_vec()),
                }],
            }
        }
        Error::Protocol(message) => plain(ConnectCode::InvalidArgument, message.clone()),
        Error::Decode(_) | Error::Json(_) => plain(ConnectCode::InvalidArgument, error.to_string()),
        Error::RunNotFound(_) => plain(ConnectCode::NotFound, error.to_string()),
        Error::Cancelled => plain(ConnectCode::Canceled, error.to_string()),
        _ => plain(ConnectCode::Internal, error.to_string()),
    };
    handle
        .emit_frame(encode_error_end_stream(&stream_error).unwrap_or_else(|_| encode_end_stream()));
    handle.close_output();
    Ok(())
}

pub(crate) fn finish_cancelled(handle: &TransportHandle) -> Result<()> {
    use crate::cursor::protocol::connect::{
        encode_end_stream, encode_error_end_stream, ConnectCode, ConnectStreamError,
    };
    let error = if handle.user_stopped() {
        user_stop_error()
    } else {
        ConnectStreamError {
            code: ConnectCode::Canceled,
            message: "run was cancelled".into(),
            details: Vec::new(),
        }
    };
    handle.emit_frame(encode_error_end_stream(&error).unwrap_or_else(|_| encode_end_stream()));
    handle.close_output();
    Ok(())
}

pub(crate) fn user_stop_error() -> crate::cursor::protocol::connect::ConnectStreamError {
    use crate::cursor::protocol::{
        connect::{ConnectCode, ConnectErrorDetail, ConnectStreamError},
        proto::aiserver::v1 as ai,
    };
    // Cursor 3.22.12 treats remote Connect Canceled as a transport retry unless
    // its own abort signal fired. Its typed USER_ABORTED_REQUEST is terminal.
    let detail = ai::ErrorDetails {
        error: 21, // USER_ABORTED_REQUEST in the installed Cursor ErrorDetails enum.
        details: Some(ai::CustomErrorDetails {
            title: "Stopped by user".into(),
            detail: "This run was stopped from Nexusor.".into(),
            is_retryable: Some(false),
            ..Default::default()
        }),
        is_expected: Some(true),
    };
    ConnectStreamError {
        code: ConnectCode::InvalidArgument,
        message: "This run was stopped from Nexusor.".into(),
        details: vec![ConnectErrorDetail {
            type_name: "aiserver.v1.ErrorDetails".into(),
            value: STANDARD_NO_PAD.encode(detail.encode_to_vec()),
        }],
    }
}

pub(crate) fn checkpoint_context_tokens(
    checkpoint: &pb::ConversationStateStructure,
) -> Option<u64> {
    checkpoint
        .token_details
        .as_ref()
        .map(|details| u64::from(details.used_tokens))
}
