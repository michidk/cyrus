#!/usr/bin/env python3
"""Exercise workspace test export and admission without compiling Rust."""
import gzip
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('bundle', Path(__file__).with_name('test_bundle.py'))
bundle = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bundle)


class BundleTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.target = self.root / 'target'
        self.target.mkdir()
        self.metadata = {'workspace_root': str(self.root / 'source'),
                         'target_directory': str(self.target), 'workspace_members': ['pkg'],
                         'packages': [{'id': 'pkg', 'name': 'cyrus', 'targets': [
                             {'name': 'cyrus', 'kind': ['bin'], 'test': True}]}]}
        self.identity = {'commit': 'a' * 40, 'tree': 'b' * 40, 'repository': 'michidk/cyrus',
                         'run': '1', 'attempt': '1', 'system': 'Linux', 'machine': 'x86_64'}
        self.messages = []
        for name, test in [('deps/cyrus-hash', True), ('cyrus', False)]:
            path = self.target / name
            path.parent.mkdir(exist_ok=True)
            path.write_bytes(b'ELF test fixture ' + name.encode())
            path.chmod(0o755)
            self.messages.append({'reason': 'compiler-artifact', 'package_id': 'pkg',
                                  'target': {'name': 'cyrus', 'kind': ['bin']},
                                  'profile': {'test': test}, 'features': ['original-save-import'],
                                  'executable': str(path), 'filenames': [str(path)]})
        self.messages.append({'reason': 'build-finished', 'success': True})
        self.output = self.root / 'bundle'

    def export(self):
        return bundle.collect(self.messages, self.metadata, self.output, self.identity)

    def test_roundtrip_includes_companion_and_exact_identity(self):
        manifest = self.export()
        admitted = bundle.admit(self.output, 'a' * 40, 'b' * 40, 'Linux', 'x86_64')
        self.assertEqual(manifest, admitted)
        self.assertEqual({entry['role'] for entry in admitted['files']}, {'test', 'companion'})
        self.assertEqual(admitted['files'][0]['features'], ['original-save-import'])
        destination = self.root / 'extract'
        bundle.extract(self.output, admitted, destination)
        self.assertEqual((destination / 'cyrus').read_bytes(), (self.target / 'cyrus').read_bytes())
        self.assertTrue((destination / 'deps/cyrus-hash').stat().st_mode & 0o111)

    def test_rejects_incomplete_failed_or_escaping_build_without_publication(self):
        for mutation in ('missing', 'failed', 'escape', 'symlink'):
            with self.subTest(mutation=mutation):
                original = json.loads(json.dumps(self.messages))
                if mutation == 'missing':
                    self.messages.pop(0)
                elif mutation == 'failed':
                    self.messages[-1]['success'] = False
                elif mutation == 'escape':
                    self.messages[0]['executable'] = str(self.root / 'outside')
                else:
                    path = self.target / 'deps/cyrus-hash'
                    path.unlink()
                    path.symlink_to(self.target / 'cyrus')
                with self.assertRaises((ValueError, OSError)):
                    self.export()
                self.assertFalse(self.output.exists())
                self.messages = original

    def test_same_named_lib_bin_and_dependency_runtime_are_all_retained(self):
        self.metadata['packages'][0]['targets'].append(
            {'name': 'cyrus', 'kind': ['lib'], 'test': True})
        library_test = self.target / 'deps/cyrus-lib'
        library_test.write_bytes(b'library test')
        library_test.chmod(0o755)
        self.messages.insert(0, dict(self.messages[0], executable=str(library_test),
                                    target={'name': 'cyrus', 'kind': ['lib']}))
        shared = self.target / 'deps/libcompanion.so'
        shared.write_bytes(b'runtime library')
        self.messages.insert(0, {'reason': 'compiler-artifact', 'package_id': 'dependency',
                                'target': {'name': 'companion', 'kind': ['dylib']},
                                'profile': {'test': False}, 'executable': None,
                                'filenames': [str(shared)]})
        manifest = self.export()
        self.assertEqual(len(manifest['files']), 4)
        self.assertEqual(sum(f['role'] == 'test' for f in manifest['files']), 2)
        self.assertEqual(sum(f['role'] == 'runtime' for f in manifest['files']), 1)
        destination = self.root / 'one-test'
        bundle.extract(self.output, manifest, destination, 'cyrus', 'deps/cyrus-lib')
        self.assertTrue((destination / 'deps/cyrus-lib').is_file())
        self.assertFalse((destination / 'deps/cyrus-hash').exists())
        self.assertTrue((destination / 'cyrus').is_file())
        with self.assertRaises(ValueError):
            bundle.extract(self.output, manifest, self.root / 'absent', test='missing')
        with self.assertRaises(ValueError):
            bundle.extract(self.output, manifest, self.root / 'absent', package='missing')

    def test_integration_tests_require_reported_binary_companion(self):
        self.metadata['packages'][0]['targets'].append(
            {'name': 'integration', 'kind': ['test'], 'test': True})
        self.messages.pop(1)
        with self.assertRaisesRegex(ValueError, 'companions'):
            self.export()
        self.assertFalse(self.output.exists())

    def test_failed_stream_does_not_publish_receipt_or_replace_existing_bundle(self):
        for error in (OSError(28, 'full'), KeyboardInterrupt()):
            with patch.object(bundle, 'copy_payload', side_effect=error):
                with self.assertRaises(type(error)):
                    self.export()
            self.assertFalse(self.output.exists())
            self.assertEqual(list(self.root.glob('.bundle-*')), [])
        self.export()
        before = (self.output / 'manifest.json').read_bytes()
        with self.assertRaises(FileExistsError):
            self.export()
        self.assertEqual(before, (self.output / 'manifest.json').read_bytes())

    def test_admission_rejects_identity_platform_missing_and_tampered_payloads(self):
        manifest = self.export()
        for arguments in [('c' * 40, 'b' * 40, 'Linux', 'x86_64'),
                          ('a' * 40, 'c' * 40, 'Linux', 'x86_64'),
                          ('a' * 40, 'b' * 40, 'Linux', 'aarch64')]:
            with self.assertRaises(ValueError):
                bundle.admit(self.output, *arguments)
        payload = self.output / manifest['files'][0]['payload']
        with gzip.open(payload, 'wb') as stream:
            stream.write(b'tampered')
        with self.assertRaises((ValueError, OSError)):
            bundle.admit(self.output, 'a' * 40, 'b' * 40, 'Linux', 'x86_64')
        payload.unlink()
        with self.assertRaises((ValueError, OSError)):
            bundle.admit(self.output, 'a' * 40, 'b' * 40, 'Linux', 'x86_64')

    def test_admission_rejects_traversal_and_duplicates(self):
        self.export()
        path = self.output / 'manifest.json'
        original = json.loads(path.read_text())
        for mutation in ('path', 'payload', 'duplicate'):
            manifest = json.loads(json.dumps(original))
            if mutation == 'duplicate':
                manifest['files'].append(manifest['files'][0])
            else:
                manifest['files'][0][mutation] = '../outside'
            path.write_text(json.dumps(manifest))
            with self.assertRaises(ValueError):
                bundle.admit(self.output, 'a' * 40, 'b' * 40, 'Linux', 'x86_64')


if __name__ == '__main__':
    unittest.main()
