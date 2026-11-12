#!/usr/bin/env python3
"""Exercise oracle decoding with generated PCM, independent of an install."""
import importlib.util
from pathlib import Path
import tempfile
import unittest
import wave

spec = importlib.util.spec_from_file_location('source', Path(__file__).with_name('ambient-source-fixture.py'))
source = importlib.util.module_from_spec(spec)
spec.loader.exec_module(source)


class SourceTests(unittest.TestCase):
    def fixture(self, width, channels, data):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        path = Path(temporary.name) / 'source.wav'
        with wave.open(str(path), 'wb') as audio:
            audio.setnchannels(channels)
            audio.setsampwidth(width)
            audio.setframerate(22050)
            audio.writeframes(data)
        return path

    def test_signed_and_unsigned_pcm(self):
        for width, data in [(1, bytes([64, 192])), (2, bytes([0, 192, 0, 64]))]:
            result = source.source_levels(self.fixture(width, 1, data))
            self.assertEqual(result['bits'], width * 8)
            self.assertEqual(result['source_rms'], .5)

    def test_rejects_stereo_and_empty(self):
        for channels, data in [(2, bytes([128, 128])), (1, b'')]:
            with self.assertRaises(ValueError):
                source.source_levels(self.fixture(1, channels, data))


if __name__ == '__main__':
    unittest.main()
