# Installs optional, local-only PDF/video decoders in Nexusor's isolated runtime.
param([switch]$Speech, [ValidateSet('base', 'small')][string]$SpeechModel = 'base')
$ErrorActionPreference = 'Stop'
if (-not (Get-Command python -ErrorAction SilentlyContinue)) {
    throw 'Python 3 x64 is required. Install it with PATH support, reopen PowerShell, then run this script again.'
}
python -c "import struct,sys; sys.exit(0 if sys.version_info.major == 3 and struct.calcsize('P') == 8 else 1)"
if ($LASTEXITCODE -ne 0) { throw 'A working Python 3 x64 interpreter is required; check python --version and disable a non-working Windows Store alias if necessary.' }
$runtime = Join-Path $HOME '.nexusor/media-runtime'
python -m venv $runtime
if ($LASTEXITCODE -ne 0) { throw 'Cannot create media runtime; install Python 3 first.' }
& (Join-Path $runtime 'Scripts/python.exe') -m pip install 'pypdfium2==5.13.0' 'imageio-ffmpeg==0.6.0' 'Pillow==12.1.0'
if ($LASTEXITCODE -ne 0) { throw 'Cannot install local media dependencies.' }
if ($Speech) {
    # PyAV 19 removed metadata_errors, still required by faster-whisper 1.2.1.
    & (Join-Path $runtime 'Scripts/python.exe') -m pip install 'faster-whisper==1.2.1' 'av==16.0.1'
    if ($LASTEXITCODE -ne 0) { throw 'Cannot install local speech dependencies.' }
    & (Join-Path $runtime 'Scripts/python.exe') -I -c "import json,sys; from pathlib import Path; from faster_whisper.utils import download_model; name=sys.argv[1]; root=Path.home()/'.nexusor/media-models'; download_model(name, output_dir=str(root/('whisper-'+name))); config=root/'speech-model.json'; pending=config.with_suffix('.tmp'); pending.write_text(json.dumps({'model':name}), encoding='utf-8'); pending.replace(config)" $SpeechModel
    if ($LASTEXITCODE -ne 0) { throw 'Cannot download local speech model.' }
}
