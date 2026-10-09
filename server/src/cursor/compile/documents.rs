//! Preserves document attachment paths and resolves supplied textual payloads.
use crate::{
    cursor::{protocol::proto::agent::v1 as pb, services::blob_sync::BlobSynchronizer},
    store::BlobId,
    Error, Result,
};

use super::context::xml;

const MAX_TEXT_BYTES: usize = 2 * 1024 * 1024;

pub(super) async fn context(
    document: &pb::SelectedDocument,
    blobs: &BlobSynchronizer,
) -> Result<String> {
    let mime = document
        .mime_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    let textual = mime.starts_with("text/")
        || matches!(
            mime.as_str(),
            "application/json" | "application/xml" | "application/yaml" | "application/x-yaml"
        )
        || mime.ends_with("+json")
        || mime.ends_with("+xml");
    let pdf = mime == "application/pdf";
    let payload = if textual || pdf {
        data(document, blobs).await?
    } else {
        None
    };
    let content = match payload {
        Some(data) if pdf => tokio::task::spawn_blocking(move || crate::documents::pdf_text(&data))
            .await.map_err(|error| Error::Protocol(format!("PDF extraction failed: {error}")))??,
        Some(data) => {
            if data.len() > MAX_TEXT_BYTES {
                return Err(Error::Protocol("selected text document exceeds 2 MiB limit".into()));
            }
            String::from_utf8(data)
                .map_err(|_| Error::Protocol("selected text document is not UTF-8".into()))?
                .trim_start_matches('\u{feff}').to_owned()
        }
        None if !document.path.is_empty() => "Attachment content is not in this message. Read the attached path with available file tools before answering about its contents.".into(),
        None => "Attachment content is unavailable through this model bridge (binary or uploaded document without a local path). Do not infer its contents from the filename.".into(),
    };
    Ok(format!(
        "<document filename=\"{}\" path=\"{}\" mime_type=\"{}\">\n{}\n</document>",
        xml(&document.filename),
        xml(&document.path),
        xml(&document.mime_type),
        content
    ))
}

pub(super) async fn data(
    document: &pb::SelectedDocument,
    blobs: &BlobSynchronizer,
) -> Result<Option<Vec<u8>>> {
    use pb::selected_document::DataOrBlobId;
    Ok(match document.data_or_blob_id.as_ref() {
        Some(DataOrBlobId::Data(data)) => Some(data.clone()),
        Some(DataOrBlobId::BlobId(raw_id)) => Some(load(raw_id, blobs).await?),
        Some(DataOrBlobId::BlobIdWithData(value)) => {
            if value.data.is_empty() {
                Some(load(&value.blob_id, blobs).await?)
            } else {
                let id = BlobId::from_bytes(&value.blob_id)?;
                blobs.cache_received(&id, &value.data).await?;
                Some(value.data.clone())
            }
        }
        Some(DataOrBlobId::PromptUploadRef(_)) | None => None,
    })
}

pub(super) async fn load(raw_id: &[u8], blobs: &BlobSynchronizer) -> Result<Vec<u8>> {
    let id = BlobId::from_bytes(raw_id)?;
    blobs.get(&id).await?.ok_or_else(|| {
        Error::Protocol(format!(
            "selected document Blob is missing: {}",
            id.to_base64()
        ))
    })
}
