#!/usr/bin/env python3
"""Original CLI run ownership without Docker, a game install or a display."""
import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

from harness import exclusive_lock

SCRIPTS = Path(__file__).parent


class OriginalRunLocks(unittest.TestCase):
    def fixture(self, root):
        run = root / 'retro-run'
        (run / 'Redguard/maps').mkdir(parents=True)
        ini = run / 'Redguard/WORLD.INI'
        ini.write_text('start_world=0\n')
        (root / 'tools').mkdir()
        (root / 'tools/original-state.py').write_text('# synthetic reader\n')
        commands = root / 'commands.jsonl'
        docker = root / 'docker'
        docker.write_text('''#!/usr/bin/env python3
import fcntl, json, os, sys
from pathlib import Path
root = Path(os.environ['CYRUS_MAGIC'])
with (root / 'retro-run.lock').open('a') as lock:
    try:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        locked = False
    except BlockingIOError:
        locked = True
with (root / 'commands.jsonl').open('a') as output:
    output.write(json.dumps({'args': sys.argv[1:], 'locked': locked}) + '\\n')
sys.exit(0 if sys.argv[1] == 'rm' else 3)
''')
        docker.chmod(0o755)
        return run, ini, commands

    def invoke(self, root, script, *extra):
        args = ['--world', '7', '--seconds', '0'] if script == 'original-idle.py' else ['--limit', '0']
        return subprocess.run([sys.executable, str(SCRIPTS / script), *args, *extra],
            env=dict(os.environ, CYRUS_MAGIC=str(root), PATH=f'{root}:{os.environ["PATH"]}'),
            capture_output=True, text=True, timeout=10, check=False)

    def test_contended_run_rejects_before_mutating_ini_or_calling_docker(self):
        for script in ('original-idle.py', 'original-duel.py'):
            with self.subTest(script=script), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                run, ini, commands = self.fixture(root)
                before = ini.read_bytes()
                with exclusive_lock(run.with_suffix('.lock')):
                    result = self.invoke(root, script)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn('BlockingIOError', result.stderr)
                self.assertEqual(ini.read_bytes(), before)
                self.assertFalse(commands.exists())

    def test_failed_startup_cleans_owned_container_before_releasing_run(self):
        for script in ('original-idle.py', 'original-duel.py'):
            with self.subTest(script=script), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                run, ini, commands = self.fixture(root)
                result = self.invoke(root, script)
                self.assertNotEqual(result.returncode, 0)
                calls = [json.loads(line) for line in commands.read_text().splitlines()]
                self.assertTrue(all(call['locked'] for call in calls), calls)
                self.assertEqual(calls[-1]['args'], ['rm', '-f', script[:-3].replace('original-', 'retro-')])
                self.assertEqual(ini.read_text(), 'start_world=0\n')
                with exclusive_lock(run.with_suffix('.lock')):
                    pass

    def test_attach_does_not_claim_default_run_or_remove_caller_container(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            run, _, commands = self.fixture(root)
            with exclusive_lock(run.with_suffix('.lock')):
                result = self.invoke(root, 'original-duel.py', '--attach', 'caller-owned')
            self.assertNotEqual(result.returncode, 0)
            self.assertNotIn('BlockingIOError', result.stderr)
            calls = [json.loads(line)['args'] for line in commands.read_text().splitlines()]
            self.assertEqual(len(calls), 1)
            self.assertEqual(calls[0][0], 'cp')
            self.assertEqual(calls[0][-1], 'caller-owned:/tmp/original-state.py')


if __name__ == '__main__':
    unittest.main()
