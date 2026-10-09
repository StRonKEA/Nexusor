# Local PDF and video images

Run `scripts/setup-media.ps1` once on the Windows PC (Python 3 and internet access
are needed for installation). The isolated runtime lives at
`~/.nexusor/media-runtime`. Its PDFium, FFmpeg and Pillow packages decode media
locally. The renderer script is embedded in the Nexusor executable; the Python
runtime is a separate prerequisite, not included in the NSIS installer.
The setup script is distributed at `scripts/setup-media.ps1` in the installation
directory, alongside `FIRST_RUN.md`; the source checkout is not required.

Supported inputs:

- Cursor selected PDF/video bytes, cached blobs, or blob-with-data attachments.
- Successful Cursor `Read` binary results containing a PDF, or a video with a
  supported file extension (`mp4`, `webm`, `mov`, `mkv`, `avi`, `m4v`).
- Selected paths without bytes, upload references and signed video URLs are not
  downloaded or read by this decoder. Cursor's file tools must supply the bytes.

PDF text extraction remains available without the optional runtime. Rendering
adds the first 8 pages as PNG images (up to 200 pages / 32 MiB input). A vision
model can read scanned text and diagrams; there is no separate OCR text engine.
Videos (up to 128 MiB / one hour) provide at most 12 evenly spaced samples, each
labeled with its requested timestamp. These are seek positions, not measured
source-frame presentation timestamps. Events between samples are omitted.
If audio outlasts the video track, available frames are retained and the missing
visual tail is reported. Read's binary gate uses the media input limits above;
other binary results keep their existing 32 KiB limit.
Models without vision require the existing vision-sidecar
configuration.

Optional speech: run `scripts/setup-media.ps1 -Speech` to install faster-whisper
1.2.1 / PyAV 16.0.1 and download the multilingual base model (about 148 MB) into
`~/.nexusor/media-models/whisper-base`. Recognition then runs locally with
`local_files_only=True`, CPU/int8 and four threads. No model download occurs
during a media request. This separate runtime/model is not bundled in NSIS.

To select the larger local model, run `scripts/setup-media.ps1 -Speech -SpeechModel small`.
Use `-Speech -SpeechModel base` to switch back. Setup saves the selection in
`~/.nexusor/media-models/speech-model.json` only after a successful download.
Without this file, base remains the default. Both choices keep the same 30-second
deadline; small can time out on slower PCs. In the measured synthetic Turkish
samples, small improved AAC price recognition but introduced some word errors;
it is a quality/speed choice, not a guarantee of better transcription.

Only the first audio stream, up to 60 seconds, is decoded to mono 16 kHz PCM.
Speech gets a separate 30-second process deadline and 12,000-byte UTF-8 text
budget. Its timestamped transcript is added to the same attachment/Read context
as the frames. Timestamps are relative to decoded audio, not verified video PTS.
Later audio, truncated speech, recognition errors and missing runtime/model are
reported explicitly; ASR failure does not discard successfully rendered frames.
Raw audio, music descriptions, speaker identification and sound effects are not
sent to the model. No recognized speech does not establish silence. Short
synthetic English speech is verified; multilingual model capability alone does
not establish Turkish, noisy-audio or long-clip accuracy.

Images are capped at 1600 pixels per edge and 24 MiB total per attachment.
One decoder runs at a time, with a 75-second visual execution timeout followed
by at most 30 seconds of optional speech recognition and temporary
files removed after the call. Conversion errors are included in the model
context; an unsuccessful conversion does not count as visual access.

Verification:

```powershell
& "$HOME/.nexusor/media-runtime/Scripts/python.exe" server/src/media/test_render.py
& "$HOME/.nexusor/media-runtime/Scripts/python.exe" server/src/media/test_transcribe.py
cargo test -p cursor-server --lib media::tests -- --include-ignored
cargo test -p cursor-server --test local_rules_context
```

The ignored speech/Read test additionally requires `NEXUSOR_MEDIA_TEST_VIDEO`
pointing to the documented `audio-smoke.mp4` fixture.
