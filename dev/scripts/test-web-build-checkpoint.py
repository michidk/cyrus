#!/usr/bin/env python3
"""Exercise recovery after compiler/package loss without Cargo or installed data."""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('checkpoint', Path(__file__).with_name('web-build-checkpoint.py'))
checkpoint = importlib.util.module_from_spec(spec)
spec.loader.exec_module(checkpoint)


class Checkpoint(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.cache = self.root / 'persistent'
        self.artifact = self.root / 'ephemeral' / 'game.wasm'
        self.artifact.parent.mkdir()
        self.artifact.write_bytes(b'compiled named WASM')
        self.expected = {'source': {'sha256': 'source'}, 'configuration': 'compiler/profile/remaps'}

    def store(self):
        checkpoint.store(self.cache, self.artifact.name, self.artifact, self.expected)

    def restore(self, expected=None):
        return checkpoint.restore(self.cache, self.artifact.name, self.artifact, expected or self.expected)

    def test_recover_after_target_loss_and_repeat_packaging(self):
        self.store()
        self.artifact.unlink()
        self.artifact.parent.rmdir()
        self.assertTrue(self.restore())
        self.assertEqual(self.artifact.read_bytes(), b'compiled named WASM')
        self.assertTrue(self.restore())

    def test_changed_source_or_configuration_requires_build(self):
        self.store()
        for key in self.expected:
            changed = {**self.expected, key: 'changed'}
            self.assertFalse(self.restore(changed))

    def test_corrupt_or_missing_retained_artifact_fails_closed(self):
        self.store()
        retained = self.cache / self.artifact.name
        retained.write_bytes(b'truncated')
        with self.assertRaisesRegex(ValueError, 'corrupt WASM checkpoint'):
            self.restore()
        retained.unlink()
        with self.assertRaisesRegex(ValueError, 'corrupt WASM checkpoint'):
            self.restore()

    def test_interrupted_copy_does_not_publish_completion(self):
        with patch.object(checkpoint.shutil, 'copyfile', side_effect=OSError('reset')):
            with self.assertRaises(OSError):
                self.store()
        self.assertFalse(self.restore())
        self.assertEqual(list(self.cache.iterdir()), [])

    def test_uncommitted_artifact_is_not_completion(self):
        self.cache.mkdir()
        (self.cache / self.artifact.name).write_bytes(b'partial')
        self.assertFalse(self.restore())

    def test_commit_identity_does_not_invalidate_unchanged_compiler_inputs(self):
        snapshot = self.root / 'source.json'
        snapshot.write_text(json.dumps({'revision': 'old', 'sha256': 'source'}))
        before = checkpoint.identity(snapshot, 'flags')
        snapshot.write_text(json.dumps({'revision': 'new', 'sha256': 'source'}))
        self.assertEqual(before, checkpoint.identity(snapshot, 'flags'))

    def test_manifest_records_exact_artifact_hash(self):
        self.store()
        record = json.loads((self.cache / (self.artifact.name + '.json')).read_text())
        self.assertEqual(record['sha256'], checkpoint.digest(self.artifact))
        self.assertEqual(record['source'], self.expected['source'])


if __name__ == '__main__':
    unittest.main()
