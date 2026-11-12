#!/usr/bin/env python3
"""The minimal original runtime must preserve explicitly admitted real audio."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / 'docker/retro/contact-entrypoint.sh'


class Audio(unittest.TestCase):
    def test_default_dummy_and_explicit_pulse(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / 'game.cue').write_text('fixture')
            for name, body in [('Xvfb', 'printf "%s\\n" "$*" > "$CYRUS_TEST_GAME/xvfb.args"'), ('sleep', 'exit 0'),
                               ('dosbox-x', 'printf "%s" "$SDL_AUDIODRIVER"')]:
                path = root / name
                path.write_text('#!/bin/sh\n' + body + '\n')
                path.chmod(0o755)
            for selected, expected in [(None, 'dummy'), ('pulseaudio', 'pulseaudio')]:
                env = {**os.environ, 'PATH': str(root) + ':' + os.environ['PATH'],
                       'CYRUS_TEST_GAME': str(root), 'CYRUS_CONFIG_DIR': str(root / 'config')}
                env.pop('SDL_AUDIODRIVER', None)
                if selected:
                    env['SDL_AUDIODRIVER'] = selected
                result = subprocess.run(['bash', '-c',
                    'cd() { builtin cd "$CYRUS_TEST_GAME"; }; source "$1"', 'test', str(SCRIPT)],
                    env=env, capture_output=True, text=True)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(result.stdout, expected)
                self.assertTrue((root / 'config').is_dir())
                self.assertIn('-fbdir ' + str(root / 'config'), (root / 'xvfb.args').read_text())


if __name__ == '__main__':
    unittest.main()
