use super::ToolCompletion;
use crate::{
    cursor::{protocol::proto::agent::v1 as pb, tools::runtime::PendingInteraction},
    model::ToolResult,
    provider::image_generation::GeneratedImage,
    Result,
};

pub(crate) fn complete_generated_image(
    pending: PendingInteraction,
    args: pb::GenerateImageArgs,
    outcome: Result<GeneratedImage>,
) -> ToolCompletion {
    let (content, result, is_error) = match outcome {
        Ok(image) => (
            format!("Generated image saved to {}. PNG image, {} bytes. Use Read on this path to inspect the generated content.", image.path, image.data.len()),
            pb::generate_image_result::Result::Success(pb::GenerateImageSuccess {
                file_path: image.path,
                // Cursor resolves the persisted local file. Keep binary data out
                // of replay history rather than passing a fake base64 placeholder.
                image_data: String::new(),
            }), false,
        ),
        Err(error) => {
            let message = error.to_string();
            (message.clone(), pb::generate_image_result::Result::Error(pb::GenerateImageError { error: message }), true)
        }
    };
    ToolCompletion::new(
        &pending.call,
        pending.started_at_ms,
        ToolResult {
            call_id: pending.call.call_id.clone(),
            content,
            is_error,
            image: None,
            images: Vec::new(),
        },
        pb::tool_call::Tool::GenerateImageToolCall(pb::GenerateImageToolCall {
            args: Some(args),
            result: Some(pb::GenerateImageResult {
                result: Some(result),
            }),
        }),
    )
}
