#!/usr/bin/env python3
"""Exercise recovery, run ownership and failure cleanup without Docker or game data."""
from pathlib import Path
import os
import subprocess
import tempfile
import unittest

SCRIPT = Path(__file__).with_name('original-book-audio.sh').resolve()

class Recovery(unittest.TestCase):
    def test_capture_failure_cleans_only_its_run_and_retains_host_paths(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            for name in ['run', 'gog', 'bin']:
                (root/name).mkdir()
            for name in ['image.tar', 'ready.py', 'driver.py']:
                (root/name).write_text('local test fixture')
            docker = root/'bin/docker'
            docker.write_text('''#!/bin/bash
printf '%s\\n' "$*" >> "$DOCKER_LOG"
case "$1" in
 image) exit 0;;
 run) echo owned-container;;
 exec) exit 23;;
esac
''')
            docker.chmod(0o755)
            out = root/'evidence'
            result = subprocess.run(['bash', str(SCRIPT), str(root/'run'), str(root/'gog'),
                str(out), str(root/'image.tar'), str(root/'ready.py'), str(root/'driver.py')],
                env={**os.environ, 'PATH': str(root/'bin')+':'+os.environ['PATH'],
                     'DOCKER_LOG': str(root/'docker.log')}, capture_output=True, text=True)
            self.assertEqual(result.returncode, 23, result.stderr)
            commands = (root/'docker.log').read_text().splitlines()
            run = next(c for c in commands if c.startswith('run '))
            name = run.split('--name ')[1].split()[0]
            self.assertIn(f'-v {out}:/evidence', run)
            self.assertIn('CYRUS_CONFIG_DIR=/evidence/config', run)
            self.assertIn('bash /evidence/runtime/entrypoint.sh', run)
            self.assertIn(f'rm -f {name}', commands)
            self.assertTrue((out/'runtime/ready.py').exists())
            self.assertTrue((out/'container.log').exists())
            (out/'capture').mkdir(); (out/'capture/result.json').write_text('{}')
            repeated = subprocess.run(['bash', str(SCRIPT), str(root/'run'), str(root/'gog'),
                str(out), str(root/'image.tar'), str(root/'ready.py'), str(root/'driver.py')],
                capture_output=True, text=True)
            self.assertNotEqual(repeated.returncode, 0)
            self.assertIn('preserve evidence', repeated.stderr)

if __name__ == '__main__':
    unittest.main()
