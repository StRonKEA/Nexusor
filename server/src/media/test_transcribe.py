"""Bounded audio coverage and optional runtime behavior."""
import subprocess
import tempfile
import unittest
import wave
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

import imageio_ffmpeg
import transcribe


class SpeechTests(unittest.TestCase):
    def test_model_selection_defaults_and_rejects_invalid_paths(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            default = root / 'whisper-base'
            self.assertEqual(transcribe.selected_model(default), default)
            config = root / 'speech-model.json'
            config.write_text('{"model":"small"}', encoding='utf-8')
            self.assertEqual(transcribe.selected_model(default), root / 'whisper-small')
            for value in ['{"model":"../remote"}', '[]', '{"model":"unknown"}', 'x' * 1025]:
                config.write_text(value, encoding='utf-8')
                with self.assertRaises(ValueError):
                    transcribe.selected_model(default)

    def test_utf8_budget_and_partial_coverage_are_explicit(self):
        segments = [SimpleNamespace(start=i, end=i + 1, text='ğ' * 2000) for i in range(8)]
        text = transcribe.transcript_text(segments, 90, 'tr')
        self.assertIn('Audio after 60s was not analyzed', text)
        self.assertIn('remaining speech is omitted', text)
        self.assertLess(len(text.encode('utf-8')), 13000)
        self.assertIn('ğ', text)
        with self.assertRaises(ValueError):
            transcribe.transcript_text([SimpleNamespace(start=float('nan'), end=1, text='x')], 2, 'en')

    def test_no_recognized_speech_does_not_claim_silence(self):
        self.assertIn('does not prove silence', transcribe.transcript_text([], 3, 'en'))

    def test_real_audio_extraction_is_limited_and_runtime_is_optional(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / 'clip.mp4'
            subprocess.run([
                imageio_ffmpeg.get_ffmpeg_exe(), '-nostdin', '-loglevel', 'error',
                '-f', 'lavfi', '-i', 'color=c=black:s=32x32:r=1',
                '-f', 'lavfi', '-i', 'sine=frequency=440:sample_rate=16000',
                '-t', '65', '-c:v', 'libx264', '-c:a', 'aac', str(source),
            ], check=True, timeout=15)
            missing = transcribe.transcribe(source, root, root / 'missing')
            self.assertIn('local speech model missing', missing)
            model = root / 'model'
            model.mkdir()
            (model / 'model.bin').touch()
            with patch('faster_whisper.WhisperModel') as constructor:
                constructor.return_value.transcribe.return_value = ([], SimpleNamespace(language='en'))
                text = transcribe.transcribe(source, root, model)
                self.assertTrue(constructor.call_args.kwargs['local_files_only'])
            with wave.open(str(root / 'audio.wav')) as audio:
                self.assertEqual(audio.getframerate(), 16000)
                self.assertEqual(audio.getnchannels(), 1)
                self.assertLessEqual(audio.getnframes(), 60 * 16000)
            self.assertIn('Audio after 60s was not analyzed', text)

    def test_silent_video_does_not_require_speech_model(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / 'silent.mp4'
            subprocess.run([
                imageio_ffmpeg.get_ffmpeg_exe(), '-nostdin', '-loglevel', 'error',
                '-f', 'lavfi', '-i', 'color=c=black:s=32x32:r=1', '-t', '1',
                '-c:v', 'libx264', str(source),
            ], check=True, timeout=10)
            self.assertIn('No audio stream', transcribe.transcribe(source, root, root / 'missing'))


if __name__ == '__main__':
    unittest.main()
