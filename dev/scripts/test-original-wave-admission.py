#!/usr/bin/env python3
"""Exercise completed WAV admission through the original recorder command."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
import wave


class WaveAdmission(unittest.TestCase):
    def capture(self, mode):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / 'events.json').write_text('[]')
            reader = root / 'reader.py'
            reader.write_text('print(\'{"counters":{"mainloop_frame":401}}\')')
            if mode == 'stale':
                with wave.open(str(root / 'recording.wav'), 'wb') as output:
                    output.setparams((2, 2, 48000, 0, 'NONE', 'not compressed'))
                    output.writeframes(bytes(1920))
            tool = root / 'xdotool'
            tool.write_text('''#!/usr/bin/env python3
import os, pathlib, wave
root = pathlib.Path(os.environ['CYRUS_CAPTURE_DIR'])
marker = root / 'armed'
if not marker.exists():
    marker.touch()
else:
    mode = os.environ['WAVE_TEST_MODE']
    if mode not in ('missing', 'stale'):
        with wave.open(str(root / 'recording.wav'), 'wb') as output:
            output.setparams((2, 2, 48000, 0, 'NONE', 'not compressed'))
            output.writeframes(bytes(480 * 4) if mode != 'empty' else b'')
        if mode == 'ambiguous':
            (root / 'other.wav').write_bytes((root / 'recording.wav').read_bytes())
        if mode == 'truncated':
            path = root / 'recording.wav'
            path.write_bytes(path.read_bytes()[:-4])
''')
            tool.chmod(0o755)
            result = subprocess.run(
                [sys.executable, str(Path(__file__).with_name('original-capture-inner.py')),
                 str(reader), str(root / 'events.json'), '0.01'],
                env={**os.environ, 'PATH': f"{root}:{os.environ['PATH']}",
                     'CYRUS_CAPTURE_DIR': str(root), 'WAVE_TEST_MODE': mode},
                text=True, capture_output=True, timeout=5)
            return result

    def test_missing_empty_and_truncated_audio_cannot_complete(self):
        for mode in ('missing', 'empty', 'truncated', 'stale', 'ambiguous'):
            with self.subTest(mode=mode):
                result = self.capture(mode)
                self.assertNotEqual(result.returncode, 0, result.stdout)
                self.assertIn('WAV', result.stderr)

    def test_silent_complete_pcm_retains_its_sample_clock(self):
        result = self.capture('complete')
        self.assertEqual(result.returncode, 0, result.stderr)
        audio = json.loads(result.stdout)['audio']
        self.assertEqual(audio, {'file': 'recording.wav', 'frames': 480,
                                'sample_rate': 48000, 'channels': 2,
                                'sample_width': 2})


if __name__ == '__main__':
    unittest.main()
