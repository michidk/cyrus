#!/usr/bin/env python3
"""Check capture runtime recovery and source-owned image invalidation."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]


class CaptureRuntime(unittest.TestCase):
    def test_build_restore_and_source_changes(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            inputs = ['dev/docker/retro/Dockerfile.capture',
                      'dev/docker/retro/Dockerfile.capture.dockerignore',
                      'dev/docker/retro/contact-entrypoint.sh', 'dev/docker/retro/dosbox-x.conf',
                      'dev/scripts/sdl-frame-capture.c', 'dev/scripts/sdl-mixer-capture.c',
                      'dev/scripts/ensure-retro-capture-runtime.sh']
            for name in inputs:
                target = root / name
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(ROOT / name, target)
            tools = root / 'bin'
            tools.mkdir()
            docker = tools / 'docker'
            docker.write_text('''#!/usr/bin/env python3
import json,sys
from pathlib import Path
args=sys.argv[1:]
with Path('calls').open('a') as f:f.write(json.dumps(args)+'\\n')
if args[:2]==['image','inspect']:sys.exit(0 if Path('available').exists() else 1)
if args[0]=='build':Path('available').touch()
''')
            docker.chmod(0o755)
            ensure = root / 'dev/scripts/ensure-runtime-image.sh'
            ensure.write_text('''#!/bin/bash
set -eu
if [ "$1" = --retain ]; then
 mkdir -p "$(dirname "$3")"; echo complete > "$3"
else
 test -s "$2"; touch available
fi
''')
            ensure.chmod(0o755)
            env = {**os.environ, 'PATH': str(tools) + ':' + os.environ['PATH']}

            def run():
                result = subprocess.run(['bash', 'dev/scripts/ensure-retro-capture-runtime.sh'],
                                        cwd=root, env=env, capture_output=True, text=True)
                self.assertEqual(result.returncode, 0, result.stderr)
                return result.stdout.strip()

            first = run()
            self.assertRegex(first, r'^cyrus-retro-capture:[0-9a-f]{64}$')
            self.assertEqual(run(), first)
            (root / 'available').unlink()
            self.assertEqual(run(), first)
            calls = [json.loads(line) for line in (root / 'calls').read_text().splitlines()]
            self.assertEqual(sum(row[0] == 'build' for row in calls), 1)
            (root / 'available').unlink()
            with (root / 'dev/scripts/sdl-frame-capture.c').open('a') as output:
                output.write('\n/* changed producer */\n')
            self.assertNotEqual(run(), first)
            calls = [json.loads(line) for line in (root / 'calls').read_text().splitlines()]
            self.assertEqual(sum(row[0] == 'build' for row in calls), 2)


if __name__ == '__main__':
    unittest.main()
