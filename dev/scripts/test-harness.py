#!/usr/bin/env python3
"""Public process-lock behavior without a game, display or Docker."""
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

from harness import exclusive_lock


class ExclusiveLock(unittest.TestCase):
    def contender(self, path):
        code = ('from harness import exclusive_lock\nimport sys\n'
                'with exclusive_lock(sys.argv[1]): print("acquired")\n')
        return subprocess.run([sys.executable, '-c', code, str(path)],
                              cwd=Path(__file__).parent, capture_output=True,
                              text=True, timeout=5, check=False)

    def test_contention_is_nonblocking_and_release_allows_another_process(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'run.lock'
            with exclusive_lock(path) as owned:
                self.assertFalse(owned.closed)
                rejected = self.contender(path)
                self.assertNotEqual(rejected.returncode, 0)
                self.assertIn('BlockingIOError', rejected.stderr)
            self.assertTrue(owned.closed)
            self.assertEqual(self.contender(path).returncode, 0)

    def test_exception_releases_the_lock_and_preserves_existing_file(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'run.lock'
            path.write_text('retained lock metadata')
            with self.assertRaisesRegex(ValueError, 'driver failed'), exclusive_lock(path):
                raise ValueError('driver failed')
            self.assertEqual(path.read_text(), 'retained lock metadata')
            self.assertEqual(self.contender(path).returncode, 0)

    def test_distinct_resources_do_not_block_each_other(self):
        with tempfile.TemporaryDirectory() as directory, exclusive_lock(Path(directory) / 'original.lock'):
            self.assertEqual(self.contender(Path(directory) / 'native.lock').returncode, 0)

    def test_aliases_of_the_same_resource_contend(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'run.lock'
            alias = Path(directory) / 'alias.lock'
            alias.symlink_to(path)
            with exclusive_lock(path):
                self.assertNotEqual(self.contender(alias).returncode, 0)

    def test_missing_parent_is_reported_without_creating_it(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'missing/run.lock'
            with self.assertRaises(FileNotFoundError), exclusive_lock(path):
                self.fail('missing parent admitted')


if __name__ == '__main__':
    unittest.main()
