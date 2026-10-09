"""Exercise the real local decoders with image-only PDF and generated video."""
import subprocess
import tempfile
import unittest
from pathlib import Path

from PIL import Image
import imageio_ffmpeg

import render


class RenderTests(unittest.TestCase):
    def test_audio_longer_than_video_preserves_available_frames(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / 'tail.mp4'
            subprocess.run([
                imageio_ffmpeg.get_ffmpeg_exe(), '-nostdin', '-loglevel', 'error',
                '-f', 'lavfi', '-t', '1', '-i', 'color=c=red:s=32x32:r=1',
                '-f', 'lavfi', '-t', '3', '-i', 'sine=frequency=440',
                '-c:v', 'libx264', '-c:a', 'aac', str(source),
            ], check=True, timeout=10)
            labels, summary = render.render_video(source, root)
            self.assertEqual(len(labels), 1)
            self.assertIn('remaining visual samples omitted', summary)
            self.assertTrue((root / '0.png').is_file())

    def test_scanned_pdf_page_limit_and_color(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            pages = [Image.new("RGB", (640, 480), (i * 25, 0, 0)) for i in range(10)]
            source = root / "scan.pdf"
            pages[0].save(source, save_all=True, append_images=pages[1:])
            labels, summary = render.render_pdf(source, root)
            self.assertEqual(labels, [f"PDF page {i}" for i in range(1, 9)])
            self.assertIn("2 pages omitted", summary)
            with Image.open(root / "7.png") as image:
                self.assertLess(abs(image.getpixel((50, 50))[0] - 175), 5)

    def test_video_timestamps_order_and_visual_content(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for index, color in enumerate(["red", "green", "blue"]):
                Image.new("RGB", (640, 480), color).save(root / f"frame{index}.png")
            source = root / "clip.mp4"
            subprocess.run([imageio_ffmpeg.get_ffmpeg_exe(), "-nostdin", "-loglevel", "error", "-framerate", "1", "-i", str(root / "frame%d.png"), "-c:v", "libx264", "-pix_fmt", "yuv420p", str(source)], check=True, timeout=10)
            labels, summary = render.render_video(source, root)
            self.assertEqual(labels, [f"Video frame sampled at {i:.3f}s" for i in range(3)])
            self.assertIn("Raw audio is not attached", summary)
            for index in range(3):
                with Image.open(root / f"{index}.png") as image:
                    pixel = image.getpixel((50, 50))
                    self.assertEqual(pixel.index(max(pixel)), index)


if __name__ == "__main__":
    unittest.main()
