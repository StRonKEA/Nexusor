//! Trims a generated image reference; the image itself never travels as text.

use crate::cursor::protocol::proto::agent::v1 as pb;

pub(super) fn gate_generate_image(tool: &mut pb::GenerateImageToolCall) {
    let Some(pb::generate_image_result::Result::Success(success)) = tool
        .result
        .as_mut()
        .and_then(|result| result.result.as_mut())
    else {
        return;
    };
    if !success.image_data.trim().is_empty()
        && !success
            .image_data
            .starts_with("[base64 image data omitted from replay; bytes=")
    {
        let original = success.image_data.trim().len();
        success.image_data = format!("[base64 image data omitted from replay; bytes={original}]");
    }
}
