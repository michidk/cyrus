#!/usr/bin/env python3
"""Check interrupted application compilation recovery without invoking Cargo."""
import importlib.util
import errno
from pathlib import Path
import tempfile
import subprocess
import sys
import unittest
import threading
from unittest.mock import patch
from contextlib import contextmanager

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

    def test_checkpoint_needs_no_uncompressed_staging_copy(self):
        with patch.object(units.shutil, 'copy2', side_effect=OSError(errno.ENOSPC, 'no room for a second copy')):
            self.store()
        self.assertEqual(self.artifact.read_bytes(), b'completed dependency')
        units.shutil.rmtree(self.target)
        self.assertEqual(units.restore(self.cache, self.target, self.identity), 3)
        self.assertEqual(self.artifact.read_bytes(), b'completed dependency')

    def test_failed_archive_preserves_old_receipt_and_live_outputs(self):
        self.store()
        records = {p: p.read_bytes() for p in (self.cache / 'units').glob('*/record.json')}
        self.artifact.write_bytes(b'new live output')
        original = units.tarfile.open

        @contextmanager
        def fail_at_close(*args, **kwargs):
            with original(*args, **kwargs) as archive:
                yield archive
            # Model a compressor/close failure after all members were written.
            raise OSError(errno.ENOSPC, 'archive close failed')

        with patch.object(units.tarfile, 'open', side_effect=fail_at_close):
            with self.assertRaises(OSError) as failed:
                self.store()
        self.assertEqual(failed.exception.errno, errno.ENOSPC)
        self.assertEqual({p: p.read_bytes() for p in (self.cache / 'units').glob('*/record.json')}, records)
        self.assertEqual(len(list((self.cache / 'units').iterdir())), 1)
        self.assertEqual(self.artifact.read_bytes(), b'new live output')
        units.restore(self.cache, self.target, self.identity)
        self.assertEqual(self.artifact.read_bytes(), b'new live output')

    def test_interrupted_archive_has_no_receipt_and_can_be_retried(self):
        code = '''
import importlib.util,json,os,sys
from pathlib import Path
spec=importlib.util.spec_from_file_location('units',sys.argv[1])
m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
original=m.tarfile.TarFile.addfile
def interrupt(archive,info,fileobj=None):
    original(archive,info,fileobj)
    os._exit(23)
m.tarfile.TarFile.addfile=interrupt
m.retain(Path(sys.argv[2]),Path(sys.argv[3]),json.loads(sys.argv[4]),json.loads(sys.argv[5]))
'''
        result = subprocess.run([sys.executable, '-c', code, str(Path(units.__file__)),
                                 str(self.cache), str(self.target), units.json.dumps(self.event),
                                 units.json.dumps(self.identity)], capture_output=True, text=True)
        self.assertEqual(result.returncode, 23, result.stderr)
        self.assertEqual(list((self.cache / 'units').rglob('record.json')), [])
        self.assertEqual(units.restore(self.cache, self.target, self.identity), 0)
        self.assertEqual(self.artifact.read_bytes(), b'completed dependency')
        self.store()
        units.shutil.rmtree(self.target)
        self.assertEqual(units.restore(self.cache, self.target, self.identity), 3)

    def test_archive_write_failure_leaves_no_receipt_or_partial_unit(self):
        original = units.tarfile.TarFile.addfile

        def fail_after_member(archive, info, fileobj=None):
            original(archive, info, fileobj)
            raise OSError(errno.ENOSPC, 'archive write failed')

        with patch.object(units.tarfile.TarFile, 'addfile', side_effect=fail_after_member, autospec=True):
            with self.assertRaises(OSError) as failed:
                self.store()
        self.assertEqual(failed.exception.errno, errno.ENOSPC)
        self.assertEqual(list((self.cache / 'units').iterdir()), [])
        self.assertEqual(self.artifact.read_bytes(), b'completed dependency')
        self.store()
        units.shutil.rmtree(self.target)
        self.assertEqual(units.restore(self.cache, self.target, self.identity), 3)

    def test_archive_or_receipt_flush_failure_preserves_previous_unit(self):
        self.store()
        records = {p: p.read_bytes() for p in (self.cache / 'units').glob('*/record.json')}
        self.artifact.write_bytes(b'new live output')
        original = units.os.fsync
        for failing_call in (1, 2):
            calls = 0

            def fail_flush(fd):
                nonlocal calls
                calls += 1
                if calls == failing_call:
                    raise OSError(errno.ENOSPC, 'flush failed')
                return original(fd)

            with self.subTest(failing_call=failing_call), patch.object(units.os, 'fsync', side_effect=fail_flush):
                with self.assertRaises(OSError) as failed:
                    self.store()
                self.assertEqual(failed.exception.errno, errno.ENOSPC)
                self.assertEqual({p: p.read_bytes() for p in (self.cache / 'units').glob('*/record.json')}, records)
                self.assertEqual(len(list((self.cache / 'units').iterdir())), 1)
                self.assertEqual(self.artifact.read_bytes(), b'new live output')

    def test_changed_source_is_rejected_without_reverting_live_outputs(self):
        original = units.tarfile.TarFile.addfile
        changed = b'x' * self.artifact.stat().st_size

        def change_during_archive(archive, info, fileobj=None):
            if info.name == str(self.artifact.relative_to(self.target)):
                self.artifact.write_bytes(changed)
            return original(archive, info, fileobj)

        with patch.object(units.tarfile.TarFile, 'addfile', side_effect=change_during_archive, autospec=True):
            with self.assertRaisesRegex(ValueError, 'compilation output changed during checkpoint'):
                self.store()
        self.assertEqual(list((self.cache / 'units').iterdir()), [])
        self.assertEqual(self.artifact.read_bytes(), changed)

    def test_failed_legacy_compression_preserves_unpacked_unit(self):
        self.store()
        unit = next((self.cache / 'units').iterdir())
        with units.tarfile.open(unit / 'files.tar.gz', 'r:gz') as archive:
            for member in archive:
                path = unit / 'files' / member.name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(archive.extractfile(member).read())
        (unit / 'files.tar.gz').unlink()
        receipt = (unit / 'record.json').read_bytes()
        with patch.object(units.os, 'fsync', side_effect=OSError(errno.ENOSPC, 'archive flush failed')):
            with self.assertRaises(OSError):
                units.pack(unit)
        self.assertEqual((unit / 'record.json').read_bytes(), receipt)
        self.assertEqual((unit / 'files' / self.artifact.relative_to(self.target)).read_bytes(), b'completed dependency')
        self.assertFalse((unit / 'files.tar.gz').exists())
        units.pack(unit)
        units.shutil.rmtree(self.target)
        self.assertEqual(units.restore(self.cache, self.target, self.identity), 3)

    def test_repeated_completed_unit_does_not_recompress_existing_bytes(self):
        self.store()
        with patch.object(units, 'write_archive', side_effect=AssertionError('recompressed completed unit')):
            self.store()
        units.shutil.rmtree(self.target)
        self.assertEqual(units.restore(self.cache, self.target, self.identity), 3)
        self.assertEqual(self.artifact.read_bytes(), b'completed dependency')

    def test_independent_units_restore_concurrently_inside_one_request(self):
        self.store()
        other = self.artifact.with_name('libother-def.rlib')
        other.write_bytes(b'another completed unit')
        units.retain(self.cache, self.target,
                     {'reason': 'compiler-artifact', 'target': {'name': 'other'},
                      'filenames': [str(other)]}, self.identity)
        barrier = threading.Barrier(2, timeout=3)
        original = units.pack

        def overlap(unit):
            barrier.wait()
            return original(unit)

        units.shutil.rmtree(self.target)
        with patch.object(units, 'pack', side_effect=overlap):
            self.assertEqual(units.restore(self.cache, self.target, self.identity), 4)
        self.assertEqual(self.artifact.read_bytes(), b'completed dependency')
        self.assertEqual(other.read_bytes(), b'another completed unit')

    def test_changed_completed_unit_still_records_new_bytes(self):
        self.store()
        self.artifact.write_bytes(b'changed completed dependency')
        self.store()
        self.assertEqual(len(list((self.cache / 'units').glob('*/record.json'))), 2)

    def test_native_executable_recovers_after_unit_completion_before_final_seal(self):
        import json
        executable = self.target / 'debug/cyrus'
        executable.parent.mkdir(parents=True)
        executable.write_bytes(b'completed native application')
        executable.chmod(0o755)
        snapshot = self.root / 'snapshot.json'
        snapshot.write_text(json.dumps({'files': {'crates/app/src/main.rs': 'source'}, 'sha256': 'source'}))
        identity = units.checkpoint.identity(snapshot, 'compiler and flags')
        event = {'reason': 'compiler-artifact', 'target': {'name': 'cyrus'},
                 'filenames': [str(executable)]}
        units.retain(self.cache, self.target, event, identity)
        units.shutil.rmtree(self.target)
        output = self.root / 'capture/cyrus'
        command = [sys.executable, str(Path(units.__file__)), 'restore-executable',
                   str(self.cache), str(snapshot), 'compiler and flags', str(output)]
        # A changed source or toolchain cannot publish the retained executable.
        for index, value in [(4, 'changed source'), (5, 'changed compiler')]:
            rejected = command.copy()
            if index == 4:
                changed = self.root / 'changed.json'
                changed.write_text(json.dumps({'files': {'crates/app/src/main.rs': value}}))
                rejected[index] = str(changed)
            else:
                rejected[index] = value
            result = subprocess.run(rejected, capture_output=True, text=True)
            self.assertEqual(result.returncode, 3, result.stderr)
            self.assertFalse(output.exists())
        result = subprocess.run(command, capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(output.read_bytes(), b'completed native application')
        self.assertTrue(output.stat().st_mode & 0o111)
        self.assertFalse(self.target.exists(), 'capture recovery must not restore the Cargo dependency tree')
        self.assertTrue(units.checkpoint.restore(self.cache, 'cyrus', output, identity))
        # Corrupt completion must not replace a previously validated output.
        record = next((self.cache / 'units').glob('*/record.json'))
        data = json.loads(record.read_text())
        data['files']['debug/cyrus'] = '0' * 64
        record.write_text(json.dumps(data))
        result = subprocess.run(command, capture_output=True, text=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(output.read_bytes(), b'completed native application')

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

    def test_retained_unpack_survives_target_loss_without_decoding_again(self):
        self.store()
        units.shutil.rmtree(self.target)
        with patch.dict(units.os.environ, {'CYRUS_COMPILE_RETAIN_UNPACKED': '1'}):
            self.assertEqual(units.restore(self.cache, self.target, self.identity), 3)
            units.shutil.rmtree(self.target)
            with patch.object(units.tarfile, 'open', side_effect=AssertionError('decoded again')):
                self.assertEqual(units.restore(self.cache, self.target, self.identity), 3)
        self.assertEqual(self.artifact.read_bytes(), b'completed dependency')

    def test_retained_unpack_is_revalidated_before_target_publication(self):
        self.store()
        units.shutil.rmtree(self.target)
        with patch.dict(units.os.environ, {'CYRUS_COMPILE_RETAIN_UNPACKED': '1'}):
            units.restore(self.cache, self.target, self.identity)
            units.shutil.rmtree(self.target)
            retained = next((self.cache / 'units').glob('*/unpacked'))
            (retained / self.artifact.relative_to(self.target)).write_bytes(b'corrupt')
            with self.assertRaisesRegex(ValueError, 'corrupt compilation checkpoint'):
                units.restore(self.cache, self.target, self.identity)
        self.assertEqual(list(self.target.rglob('*')), [])

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
        self.identity = {'source': {'revision': 'before-commit', 'sha256': 'before', 'files': {'crates/app/lib.rs': 'same', 'dev/scripts/run.sh': 'before', 'dev/vendor/lib.rs': 'same'}}, 'configuration': 'flags'}
        self.store()
        units.shutil.rmtree(self.target)
        changed = {'source': {'revision': 'after-commit', 'sha256': 'after', 'files': {'crates/app/lib.rs': 'same', 'dev/scripts/run.sh': 'after', 'dev/vendor/lib.rs': 'same'}}, 'configuration': 'flags'}
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
