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
        with self.assertRaisesRegex(ValueError, 'corrupt build checkpoint'):
            self.restore()
        retained.unlink()
        with self.assertRaisesRegex(ValueError, 'corrupt build checkpoint'):
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

    def test_worker_recovery_ignores_unrelated_application_source(self):
        worker = self.root / 'crates' / 'cyrus-assets-worker'
        dependency = self.root / 'crates' / 'cyrus-assets'
        startup = self.root / 'crates' / 'cyrus-startup'
        for directory in (worker, dependency, startup):
            directory.mkdir(parents=True)
            (directory / 'Cargo.toml').write_text('[package]\nname = "' + directory.name + '"\n')
        (worker / 'Cargo.toml').write_text('[package]\nname = "cyrus-assets-worker"\n[dependencies]\ncyrus-assets = { path = "../cyrus-assets" }\n')
        self.artifact = self.artifact.with_name('cyrus_assets_worker.wasm')
        self.artifact.write_bytes(b'worker')
        files = {'Cargo.lock': 'lock', 'crates/cyrus-assets-worker/src/lib.rs': 'worker',
                 'crates/cyrus-assets/src/lib.rs': 'asset', 'crates/cyrus-startup/src/movie.rs': 'old'}
        self.expected = {'source': {'sha256': 'old-global', 'files': files}, 'configuration': 'flags'}
        self.store()
        changed = {'source': {'sha256': 'new-global', 'files': {**files, 'crates/cyrus-startup/src/movie.rs': 'new'}}, 'configuration': 'flags'}
        with patch.object(checkpoint.Path, 'cwd', return_value=self.root):
            self.assertTrue(self.restore(changed))
            changed['source']['files']['crates/cyrus-assets/src/lib.rs'] = 'changed dependency'
            self.assertFalse(self.restore(changed))
            changed['source']['files'] = {**files, 'Cargo.lock': 'changed lock'}
            self.assertFalse(self.restore(changed))

    def test_application_orchestration_changes_preserve_compiled_wasm(self):
        app = self.root / 'crates/cyrus-web'
        app.mkdir(parents=True)
        (app / 'Cargo.toml').write_text('[package]\nname = "cyrus-web"\n')
        self.artifact = self.artifact.with_name('cyrus-web.wasm')
        self.artifact.write_bytes(b'application')
        files = {'Cargo.lock': 'lock', 'crates/cyrus-web/src/main.rs': 'app', 'dev/scripts/build-web.sh': 'before'}
        self.expected = {'source': {'sha256': 'before', 'files': files}, 'configuration': 'flags'}
        self.store()
        changed = {'source': {'sha256': 'after', 'files': {**files, 'dev/scripts/build-web.sh': 'after'}}, 'configuration': 'flags'}
        with patch.object(checkpoint.Path, 'cwd', return_value=self.root):
            self.assertTrue(self.restore(changed))
            changed['source']['files']['crates/cyrus-web/src/main.rs'] = 'changed application'
            self.assertFalse(self.restore(changed))

    def test_manifest_records_exact_artifact_hash(self):
        self.store()
        record = json.loads((self.cache / (self.artifact.name + '.json')).read_text())
        self.assertEqual(record['sha256'], checkpoint.digest(self.artifact))
        self.assertEqual(record['source'], self.expected['source'])


if __name__ == '__main__':
    unittest.main()
