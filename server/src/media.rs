//! Isolates optional local media decoding and returns bounded image payloads.
use std::{path::PathBuf, time::Duration};

use serde::Deserialize;
use tokio::process::Command;

use crate::{Error, Result};

#[cfg(test)]
mod tests;

pub(crate) struct RenderedMedia {
    pub summary: String,
    pub images: Vec<(String, Vec<u8>)>,
}

#[derive(Deserialize)]
struct Manifest {
    summary: String,
    labels: Vec<String>,
}

pub(crate) fn kind(data: &[u8], mime: &str, path: &str) -> Option<&'static str> {
    if data.starts_with(b"%PDF-") {
        return Some("pdf");
    }
    let extension = std::path::Path::new(path)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    (mime.starts_with("video/")
        || matches!(
            extension.as_str(),
            "mp4" | "webm" | "mov" | "mkv" | "avi" | "m4v"
        ))
    .then_some("video")
}

fn python() -> Result<PathBuf> {
    let root = dirs::home_dir()
        .ok_or_else(|| Error::Protocol("cannot locate media runtime home".into()))?
        .join(".nexusor/media-runtime");
    let path = if cfg!(windows) {
        root.join("Scripts/python.exe")
    } else {
        root.join("bin/python")
    };
    if !path.is_file() {
        return Err(Error::Protocol("local media runtime missing; see FIRST_RUN.md and scripts/setup-media.ps1 in the Nexusor installation directory (Python 3 required)".into()));
    }
    Ok(path)
}

pub(crate) async fn render(data: &[u8], kind: &str) -> Result<RenderedMedia> {
    let limit = if kind == "pdf" { 32 } else { 128 } * 1024 * 1024;
    if data.is_empty() || data.len() > limit || !matches!(kind, "pdf" | "video") {
        return Err(Error::Protocol("media is empty, unsupported, or exceeds its input size limit (PDF 32 MiB / video 128 MiB)".into()));
    }
    // Serialize decoders to bound their combined memory and CPU footprint.
    static DECODER: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(1);
    let _permit = tokio::time::timeout(Duration::from_secs(90), DECODER.acquire())
        .await
        .map_err(|_| Error::Protocol("local media decoder is busy".into()))?
        .map_err(|_| Error::Protocol("local media decoder is closed".into()))?;
    let directory = tempfile::tempdir()?;
    let source = directory.path().join("input");
    tokio::fs::write(&source, data).await?;
    let mut command = Command::new(python()?);
    command
        .args(["-I", "-c", include_str!("media/render.py"), kind])
        .arg(&source)
        .arg(directory.path())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    let output = tokio::time::timeout(Duration::from_secs(75), command.output())
        .await
        .map_err(|_| Error::Protocol("local media rendering timed out after 75s".into()))??;
    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr);
        return Err(Error::Protocol(format!(
            "local media rendering failed: {}",
            error.chars().take(1500).collect::<String>()
        )));
    }
    let manifest: Manifest =
        serde_json::from_slice(&tokio::fs::read(directory.path().join("manifest.json")).await?)?;
    let max_images = if kind == "pdf" { 8 } else { 12 };
    if manifest.labels.is_empty() || manifest.labels.len() > max_images {
        return Err(Error::Protocol("invalid media image count".into()));
    }
    let mut images = Vec::new();
    let mut total = 0;
    for (index, label) in manifest.labels.into_iter().enumerate() {
        let file = directory.path().join(format!("{index}.png"));
        let length = tokio::fs::metadata(&file).await?.len();
        total += length;
        if total > 24 * 1024 * 1024 {
            return Err(Error::Protocol(
                "rendered media exceeds 24 MiB image budget".into(),
            ));
        }
        let bytes = tokio::fs::read(file).await?;
        let reader = image::ImageReader::new(std::io::Cursor::new(&bytes)).with_guessed_format()?;
        let dimensions = reader
            .into_dimensions()
            .map_err(|e| Error::Protocol(format!("invalid rendered image: {e}")))?;
        if dimensions.0 == 0 || dimensions.1 == 0 || dimensions.0 > 1600 || dimensions.1 > 1600 {
            return Err(Error::Protocol(
                "rendered image dimensions exceed limit".into(),
            ));
        }
        images.push((label, bytes));
    }
    let mut summary = manifest.summary;
    if kind == "video" {
        // Audio has its own deadline: optional ASR failures must not discard frames.
        let audio = transcribe(&source, directory.path()).await;
        summary.push('\n');
        match audio {
            Ok(text) => summary.push_str(&text),
            Err(error) => summary.push_str(&format!(
                "Audio not transcribed: {error}. Video frames are still available."
            )),
        }
    }
    Ok(RenderedMedia { summary, images })
}

async fn transcribe(source: &std::path::Path, directory: &std::path::Path) -> Result<String> {
    let model = dirs::home_dir()
        .ok_or_else(|| Error::Protocol("cannot locate speech model home".into()))?
        .join(".nexusor/media-models/whisper-base");
    let mut command = Command::new(python()?);
    command
        .args(["-I", "-c", include_str!("media/transcribe.py")])
        .arg(source)
        .arg(directory)
        .arg(model)
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    let output = tokio::time::timeout(Duration::from_secs(30), command.output())
        .await
        .map_err(|_| Error::Protocol("local speech recognition timed out after 30s".into()))??;
    if !output.status.success() {
        return Err(Error::Protocol(format!(
            "local speech recognition unavailable: {}",
            String::from_utf8_lossy(&output.stderr)
                .chars()
                .take(500)
                .collect::<String>()
        )));
    }
    let file = directory.join("audio.json");
    if tokio::fs::metadata(&file).await?.len() > 16 * 1024 {
        return Err(Error::Protocol(
            "speech transcript exceeds 16 KiB manifest budget".into(),
        ));
    }
    Ok(serde_json::from_slice(&tokio::fs::read(file).await?)?)
}
