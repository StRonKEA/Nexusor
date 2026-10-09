"""Bounded, local-only PDF/video rasterization for Nexusor's image bridge."""
import json
import math
import re
import subprocess
import sys
from pathlib import Path

MAX_EDGE = 1600


def save_image(image, directory, labels, label):
    image.thumbnail((MAX_EDGE, MAX_EDGE))
    image.convert("RGB").save(directory / f"{len(labels)}.png")
    labels.append(label)


def render_pdf(source, directory):
    import pypdfium2 as pdfium

    labels = []
    with pdfium.PdfDocument(source) as document:
        count = len(document)
        if not 0 < count <= 200:
            raise ValueError("PDF must have 1–200 pages")
        for index in range(min(count, 8)):
            page = document[index]
            try:
                width, height = page.get_size()
                if not all(math.isfinite(v) and v > 0 for v in (width, height)):
                    raise ValueError("invalid PDF page dimensions")
                bitmap = page.render(scale=min(2, MAX_EDGE / max(width, height)))
                try:
                    save_image(bitmap.to_pil(), directory, labels, f"PDF page {index + 1}")
                finally:
                    bitmap.close()
            finally:
                page.close()
    return labels, f"Rendered first {len(labels)} of {count} PDF pages. {count - len(labels)} pages omitted from images. Scanned text and diagrams are visible in these images; OCR accuracy depends on the vision model."


def run_ffmpeg(executable, arguments, timeout):
    return subprocess.run(
        [executable, "-nostdin", "-hide_banner", *arguments],
        stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, timeout=timeout,
        creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0),
    )


def render_video(source, directory):
    import imageio_ffmpeg
    from PIL import Image

    executable = imageio_ffmpeg.get_ffmpeg_exe()
    # Only a supplied local file is allowed; embedded network URLs are disabled.
    inputs = ["-protocol_whitelist", "file,pipe", "-i", str(source)]
    probe = run_ffmpeg(executable, inputs + ["-f", "null", "-t", "0", "-"], 10)
    metadata = probe.stderr.decode("utf-8", errors="replace")
    match = re.search(r"Duration: (\d+):(\d+):(\d+\.\d+)", metadata)
    if probe.returncode or not match:
        raise ValueError("cannot decode video or determine its duration")
    hours, minutes, seconds = map(float, match.groups())
    duration = hours * 3600 + minutes * 60 + seconds
    if not 0 < duration <= 3600:
        raise ValueError("video duration must be at most one hour")
    count = min(12, max(1, math.ceil(duration)))
    labels = []
    missing_tail = ''
    for index in range(count):
        timestamp = index * duration / count
        target = directory / f"{index}.png"
        result = run_ffmpeg(executable, [
            "-loglevel", "error", "-threads", "1", "-ss", f"{timestamp:.6f}",
            *inputs, "-map", "0:v:0", "-an", "-sn", "-dn",
            "-frames:v", "1", "-vf",
            "scale=1600:1600:force_original_aspect_ratio=decrease",
            "-threads", "1", "-y", str(target),
        ], 4)
        if result.returncode:
            raise ValueError(f"video frame decoding failed at {timestamp:.3f}s")
        if not target.is_file():
            if not labels:
                raise ValueError('video has no decodable frames')
            missing_tail = f' No frame returned at {timestamp:.3f}s; remaining visual samples omitted (audio may outlast video).'
            break
        with Image.open(target) as image:
            image.verify()
        labels.append(f"Video frame sampled at {timestamp:.3f}s")
    return labels, f"Video duration {duration:.3f}s; {len(labels)} sampled frames attached in timestamp order. Motion/events between samples are omitted.{missing_tail} Raw audio is not attached; a separate speech transcript or audio status follows."


def main():
    kind, source, destination = sys.argv[1:]
    directory = Path(destination)
    if kind == "pdf":
        labels, summary = render_pdf(source, directory)
    elif kind == "video":
        labels, summary = render_video(source, directory)
    else:
        raise ValueError("unsupported media kind")
    (directory / "manifest.json").write_text(json.dumps({"labels": labels, "summary": summary}), encoding="utf-8")


if __name__ == "__main__":
    main()
