"""Optional bounded speech recognition; only model setup may use the network."""
import json
import math
import re
import subprocess
import sys
from pathlib import Path

MAX_SECONDS = 60
MAX_TEXT_BYTES = 12000


def transcript_text(segments, duration, language):
    if not math.isfinite(duration) or not 0 < duration <= 3600:
        raise ValueError('invalid transcript duration')
    coverage = min(duration, MAX_SECONDS)
    lines = []
    size = 0
    truncated = False
    for segment in segments:
        start, end = float(segment.start), float(segment.end)
        if not all(math.isfinite(v) for v in (start, end)) or not 0 <= start <= end:
            raise ValueError('invalid speech timestamps')
        if start >= coverage:
            break
        line = f'[{start:.2f}–{min(end, coverage):.2f}s] {segment.text.strip()}\n'
        length = len(line.encode('utf-8'))
        if size + length > MAX_TEXT_BYTES:
            truncated = True
            break
        lines.append(line)
        size += length
    notice = (
        f'Local automatic speech transcript ({language}), first audio stream, '
        f'Analyzed audio coverage: 0–{coverage:.2f}s of {duration:.2f}s. '
        'Segment timestamps locate recognized speech, not the analysis boundary; '
        'gaps or an earlier last segment do not mean that audio was unprocessed. '
        'Timestamps are relative to the decoded audio stream, not verified video synchronization. '
        'Recognition may be inaccurate; music, non-speech sounds and speaker identity are not described. '
    )
    if duration > MAX_SECONDS:
        notice += f'Audio after {MAX_SECONDS}s was not analyzed. '
    if truncated:
        notice += 'Transcript text budget reached; remaining speech is omitted. '
    if not lines:
        notice += 'No speech was recognized; this does not prove silence. '
    return notice + '\n' + ''.join(lines)


def transcribe(source, directory, model_path):
    import imageio_ffmpeg

    executable = imageio_ffmpeg.get_ffmpeg_exe()
    inputs = ['-protocol_whitelist', 'file,pipe', '-i', str(source)]
    options = dict(stdout=subprocess.DEVNULL, stderr=subprocess.PIPE,
                   creationflags=getattr(subprocess, 'CREATE_NO_WINDOW', 0))
    probe = subprocess.run([executable, '-nostdin', '-hide_banner', *inputs,
                            '-t', '0', '-f', 'null', '-'], timeout=8, **options)
    metadata = probe.stderr.decode('utf-8', errors='replace')
    if probe.returncode:
        raise ValueError('cannot inspect video audio')
    if not re.search(r'Stream #.*Audio:', metadata):
        return 'No audio stream found in the supplied video.'
    match = re.search(r'Duration: (\d+):(\d+):(\d+\.\d+)', metadata)
    if not match:
        raise ValueError('cannot determine audio coverage')
    hours, minutes, seconds = map(float, match.groups())
    duration = hours * 3600 + minutes * 60 + seconds
    if not 0 < duration <= 3600:
        raise ValueError('invalid video duration')
    if not (model_path / 'model.bin').is_file():
        return 'Audio not transcribed: local speech model missing; run scripts/setup-media.ps1 -Speech.'
    from faster_whisper import WhisperModel

    target = directory / 'audio.wav'
    result = subprocess.run([
        executable, '-nostdin', '-hide_banner', '-loglevel', 'error', '-threads', '1',
        *inputs, '-map', '0:a:0', '-t', str(MAX_SECONDS), '-vn', '-sn', '-dn',
        '-ac', '1', '-ar', '16000', '-c:a', 'pcm_s16le', '-y', str(target),
    ], timeout=8, **options)
    if result.returncode or not target.is_file() or target.stat().st_size > 2_000_000:
        raise ValueError('bounded audio extraction failed')
    model = WhisperModel(str(model_path), device='cpu', compute_type='int8',
                         cpu_threads=4, local_files_only=True)
    segments, info = model.transcribe(str(target), beam_size=5, temperature=0, vad_filter=True,
                                      condition_on_previous_text=False)
    return transcript_text(segments, duration, info.language)


def selected_model(default_path):
    config = default_path.parent / 'speech-model.json'
    if not config.is_file():
        return default_path
    if config.stat().st_size > 1024:
        raise ValueError('local speech model configuration exceeds limit')
    value = json.loads(config.read_text(encoding='utf-8'))
    name = value.get('model') if isinstance(value, dict) else None
    if name not in ('base', 'small'):
        raise ValueError('local speech model must be base or small')
    return default_path.parent / f'whisper-{name}'


def main():
    source, destination, model = map(Path, sys.argv[1:])
    model = selected_model(model)
    text = f'Local speech model: {model.name}.\n' + transcribe(source, destination, model)
    (destination / 'audio.json').write_text(json.dumps(text, ensure_ascii=False), encoding='utf-8')


if __name__ == '__main__':
    main()
