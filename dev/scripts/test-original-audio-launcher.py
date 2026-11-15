#!/usr/bin/env python3
"""Exercise original audio launch payloads through the public shell command."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


class AudioLauncher(unittest.TestCase):
    def test_state_reader_can_import_its_region_owner(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            install = root / 'gog/extracted/app/Redguard'
            (install / 'soup386').mkdir(parents=True)
            (install / 'soup386/SOUP386.DEF').write_text('synthetic definition')
            (install / 'system').mkdir()
            (root / 'run/Redguard').mkdir(parents=True)
            (root / 'container').mkdir()
            (root / 'events.json').write_text('[]')
            reader = root / 'reader.py'
            reader.write_text('''import importlib.util, io, pathlib
spec = importlib.util.spec_from_file_location('region', pathlib.Path(__file__).with_name('original-memory-region.py'))
region = importlib.util.module_from_spec(spec)
spec.loader.exec_module(region)
assert region.locate(io.BytesIO(b'___anchor' + bytes(20)), [(0, 29)], b'anchor', 3, 20) == 0
print('{"reader": "bound"}')
''')
            docker = root / 'docker'
            docker.write_text('''#!/usr/bin/env python3
import json, os, pathlib, shutil, subprocess, sys
root = pathlib.Path(os.environ['CAPTURE_TEST'])
args = sys.argv[1:]
if args[0] == 'cp':
    shutil.copy2(args[1], root / 'container' / args[2].split(':', 1)[1].rsplit('/', 1)[1])
elif args[0] == 'run' and '-d' in args:
    (root / 'launch.json').write_text(json.dumps(args))
elif args[0] == 'exec':
    result = subprocess.run([sys.executable, str(root / 'container/original-state.py')])
    if result.returncode == 0:
        (root / 'run/capture/fixture.wav').write_bytes(b'RIFFfixture')
    sys.exit(result.returncode)
''')
            docker.chmod(0o755)
            # Queue/lock coordination is outside this isolated payload contract.
            flock = root / 'flock'
            flock.write_text('#!/bin/sh\nexit 0\n')
            flock.chmod(0o755)
            result = subprocess.run(
                ['bash', str(Path(__file__).with_name('original-audio-capture.sh')),
                 str(root / 'events.json'), str(root / 'output'), '1'],
                capture_output=True, text=True, timeout=10,
                env={**os.environ, 'PATH': f"{root}:{os.environ['PATH']}",
                     'CAPTURE_TEST': str(root), 'CYRUS_RETRO_RUN': str(root / 'run'),
                     'CYRUS_RETRO_GOG': str(root / 'gog'), 'CYRUS_STATE_READER': str(reader)},
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual((root / 'output/audio.wav').read_bytes(), b'RIFFfixture')
            self.assertIn('bound', (root / 'output/timeline.json').read_text())
            launch = json.loads((root / 'launch.json').read_text())
            self.assertIn('CYRUS_CONFIG_DIR=/capture-config', launch)
            self.assertIn(f"{root / 'output/config'}:/capture-config", launch)


if __name__ == '__main__':
    unittest.main()
