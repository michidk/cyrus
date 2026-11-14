#!/usr/bin/env python3
"""Check interrupted application compilation recovery without invoking Cargo."""
import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('units', Path(__file__).with_name('web-compile-checkpoint.py'))
units = importlib.util.module_from_spec(spec)
spec.loader.exec_module(units)


class Recovery(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.target = self.root / 'target'
        self.cache = self.root / 'retained'
        self.artifact = self.target / 'wasm32-unknown-unknown/wasm-dev/deps/libexample-abc.rlib'
        self.artifact.parent.mkdir(parents=True)
        self.artifact.write_bytes(b'completed dependency')
        self.dep = self.artifact.with_name('example-abc.d')
        self.dep.write_text('dependency paths')
        self.fingerprint = self.artifact.parent.parent / '.fingerprint/example-abc/lib-example'
        self.fingerprint.parent.mkdir(parents=True, exist_ok=True)
        self.fingerprint.write_text('fingerprint')
        self.event = {'reason': 'compiler-artifact', 'target': {'name': 'example'}, 'filenames': [str(self.artifact)]}
        self.identity = {'source': 'same', 'configuration': 'same'}

    def store(self):
        units.retain(self.cache, self.target, self.event, self.identity)

    def test_completed_dependency_survives_loss_before_application_completion(self):
        self.store()
        units.shutil.rmtree(self.target)
        self.assertEqual(units.restore(self.cache, self.target, self.identity), 3)
        self.assertEqual(self.artifact.read_bytes(), b'completed dependency')
        self.assertEqual(self.fingerprint.read_text(), 'fingerprint')
        self.assertEqual(units.restore(self.cache, self.target, self.identity), 0)

    def test_restore_streams_compressed_members_without_random_archive_seeks(self):
        self.store()
        units.shutil.rmtree(self.target)
        with patch.object(units.tarfile.TarFile, 'getmember', side_effect=AssertionError('random compressed archive seek')):
            self.assertEqual(units.restore(self.cache, self.target, self.identity), 3)

    def test_staging_uses_writable_target_not_its_managed_parent(self):
        self.store()
        temporary = units.tempfile.TemporaryDirectory
        def inside_target(*args, **kwargs):
            self.assertEqual(kwargs['dir'], self.target)
            return temporary(*args, **kwargs)
        with patch.object(units.tempfile, 'TemporaryDirectory', side_effect=inside_target):
            units.restore(self.cache, self.target, self.identity)

    def test_changed_inputs_rejected_and_live_shared_files_preserved(self):
        self.store()
        self.artifact.write_bytes(b'new live compiler output')
        self.assertEqual(units.restore(self.cache, self.target, {'source': 'changed'}), 0)
        units.restore(self.cache, self.target, self.identity)
        self.assertEqual(self.artifact.read_bytes(), b'new live compiler output')

    def test_corrupt_unit_rejected_before_any_publication(self):
        self.store()
        record = next((self.cache / 'units').glob('*/record.json'))
        data = units.json.loads(record.read_text())
        data['files'][str(self.artifact.relative_to(self.target))] = 'corrupt'
        record.write_text(units.json.dumps(data))
        units.shutil.rmtree(self.target)
        with self.assertRaisesRegex(ValueError, 'corrupt compilation checkpoint'):
            units.restore(self.cache, self.target, self.identity)
        self.assertEqual(list(self.target.rglob("*")), [])

    def test_orchestration_changes_do_not_invalidate_identical_compilation(self):
        self.identity = {'source': {'sha256': 'before', 'files': {'crates/app/lib.rs': 'same', 'dev/scripts/run.sh': 'before', 'dev/vendor/lib.rs': 'same'}}, 'configuration': 'flags'}
        self.store()
        units.shutil.rmtree(self.target)
        changed = {'source': {'sha256': 'after', 'files': {'crates/app/lib.rs': 'same', 'dev/scripts/run.sh': 'after', 'dev/vendor/lib.rs': 'same'}}, 'configuration': 'flags'}
        self.assertEqual(units.restore(self.cache, self.target, changed), 3)
        changed['source']['files']['crates/app/lib.rs'] = 'changed'
        units.shutil.rmtree(self.target)
        self.assertEqual(units.restore(self.cache, self.target, changed), 0)

    def test_build_script_compiler_fingerprint_and_executable_survive(self):
        executable = self.target / 'wasm-dev/build/example-xyz/build-script-build'
        executable.parent.mkdir(parents=True)
        executable.write_bytes(b'executable')
        fingerprint = self.target / 'wasm-dev/.fingerprint/example-xyz/build-script-build-script-build'
        fingerprint.parent.mkdir(parents=True, exist_ok=True)
        fingerprint.write_text('compiler fingerprint')
        event = {'reason': 'compiler-artifact', 'target': {'name': 'build-script-build'}, 'filenames': [str(executable)]}
        units.retain(self.cache, self.target, event, self.identity)
        units.shutil.rmtree(self.target)
        self.assertEqual(units.restore(self.cache, self.target, self.identity), 2)
        self.assertEqual(fingerprint.read_text(), 'compiler fingerprint')

    def test_reset_during_staging_does_not_admit_partial_units(self):
        pending = self.cache / 'units/tmp-interrupted'
        pending.mkdir(parents=True)
        (pending / 'record.json').write_text('partial manifest')
        self.assertEqual(units.restore(self.cache, self.target, self.identity), 0)

    def test_identical_bytes_with_different_configuration_have_separate_records(self):
        self.store()
        changed = {**self.identity, 'configuration': 'other flags'}
        units.retain(self.cache, self.target, self.event, changed)
        self.assertEqual(len(list((self.cache / 'units').iterdir())), 2)
        units.shutil.rmtree(self.target)
        self.assertEqual(units.restore(self.cache, self.target, changed), 3)

    def test_final_application_retains_its_cargo_fingerprint(self):
        app = self.target / 'wasm32-unknown-unknown/wasm-dev/cyrus-web.wasm'
        app.write_bytes(b'application')
        fingerprint = app.parent / '.fingerprint/cyrus-web-123/bin-cyrus-web'
        fingerprint.parent.mkdir(parents=True)
        fingerprint.write_text('application fingerprint')
        event = {'reason': 'compiler-artifact', 'package_id': 'path+file:///repo#cyrus-web@1.0', 'target': {'name': 'cyrus-web'}, 'filenames': [str(app)]}
        units.retain(self.cache, self.target, event, self.identity)
        units.shutil.rmtree(self.target)
        self.assertEqual(units.restore(self.cache, self.target, self.identity), 2)
        self.assertEqual(fingerprint.read_text(), 'application fingerprint')

    def test_concurrent_publication_cannot_be_overwritten(self):
        self.store()
        units.shutil.rmtree(self.target)
        link = units.os.link

        def publish_first(source, destination):
            if destination == self.artifact:
                destination.write_bytes(b'peer compiler result')
            return link(source, destination)

        with patch.object(units.os, 'link', side_effect=publish_first):
            units.restore(self.cache, self.target, self.identity)
        self.assertEqual(self.artifact.read_bytes(), b'peer compiler result')

    def test_unfinished_unit_is_not_retained(self):
        units.retain(self.cache, self.target, {'reason': 'compiler-message'}, self.identity)
        self.assertFalse(self.cache.exists())

    def test_completed_build_script_outputs_retained(self):
        output = self.target / 'wasm32-unknown-unknown/wasm-dev/build/example-abc/out'
        output.mkdir(parents=True)
        (output / 'generated.rs').write_text('generated source')
        fingerprint = output.parent.parent.parent / '.fingerprint' / output.parent.name / 'run-build-script-build-script-build'
        fingerprint.parent.mkdir(parents=True, exist_ok=True)
        fingerprint.write_text('completed run')
        units.retain(self.cache, self.target, {'reason': 'build-script-executed', 'out_dir': str(output)}, self.identity)
        units.shutil.rmtree(self.target)
        self.assertEqual(units.restore(self.cache, self.target, self.identity), 3)
        self.assertEqual((output / 'generated.rs').read_text(), 'generated source')


if __name__ == '__main__':
    unittest.main()
