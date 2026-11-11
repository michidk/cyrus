#!/usr/bin/env python3
"""Public recovery contract without Docker builds or shared runtime changes."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

SCRIPT = Path(__file__).with_name('ensure-runtime-image.sh')

class Recovery(unittest.TestCase):
    def run_restore(self, present=False, archive=True, accepted=True, correct_tag=True):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            image = root / 'present'
            if present:
                image.touch()
            saved = root / 'runtime.tar'
            if archive:
                saved.write_bytes(b'fixture')
            docker = root / 'docker'
            docker.write_text('#!/bin/sh\necho "$*" >> "$CALLS"\n'
                'if [ "$1" = image ]; then test -f "$PRESENT"; '
                'elif [ "$1" = load ]; then test "$ACCEPTED" = yes || exit 1; [ "$CORRECT_TAG" = no ] || touch "$PRESENT"; fi\n')
            docker.chmod(0o755)
            result = subprocess.run(['bash', str(SCRIPT), 'fixture:1', str(saved)],
                env=dict(os.environ, PATH=f'{root}:{os.environ["PATH"]}', PRESENT=str(image),
                         CALLS=str(root/'calls'), ACCEPTED='yes' if accepted else 'no', CORRECT_TAG='yes' if correct_tag else 'no'),
                text=True, capture_output=True)
            return result, (root/'calls').read_text()

    def test_existing_image_needs_no_archive_or_load(self):
        result, calls = self.run_restore(present=True, archive=False)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertNotIn('load', calls)

    def test_erased_image_restores_and_verifies_requested_identity(self):
        result, calls = self.run_restore()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(calls.count('load --input'), 1)
        self.assertEqual(calls.count('image inspect fixture:1'), 3)

    def test_successful_load_with_wrong_tag_is_rejected(self):
        result, calls = self.run_restore(correct_tag=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('did not restore requested image fixture:1', result.stderr)
        self.assertIn('load --input', calls)

    def test_missing_archive_or_failed_load_does_not_start_runtime(self):
        for options in [dict(archive=False), dict(accepted=False)]:
            result, _ = self.run_restore(**options)
            self.assertNotEqual(result.returncode, 0)

if __name__ == '__main__':
    unittest.main()
