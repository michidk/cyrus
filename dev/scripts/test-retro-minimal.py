#!/usr/bin/env python3
"""Check source-bound headless runtime projection without Docker or game data."""
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import subprocess
import shutil
import tarfile
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('minimal', Path(__file__).with_name('prepare-retro-minimal.py'))
minimal = importlib.util.module_from_spec(spec)
spec.loader.exec_module(minimal)


def fixture(path, entries):
    stream = io.BytesIO()
    with tarfile.open(fileobj=stream, mode='w') as target:
        for key, data, kind in entries:
            member = tarfile.TarInfo(key)
            member.type, member.mode = kind, 0o755
            member.size = len(data) if member.isfile() else 0
            if member.issym() or member.islnk():
                member.linkname = data.decode()
            target.addfile(member, io.BytesIO(data) if member.isfile() else None)
    config = dict(architecture='amd64', os='linux', config=dict(Env=['SOURCE=retained']),
                  rootfs=dict(type='layers', diff_ids=[]))
    with tarfile.open(path, 'w') as target:
        minimal.layers.add_bytes(target, 'layer.tar', stream.getvalue())
        minimal.layers.add_bytes(target, 'config.json', json.dumps(config).encode())
        minimal.layers.add_bytes(target, 'manifest.json', json.dumps([dict(
            Config='config.json', RepoTags=['fixture:1'], Layers=['layer.tar'])]).encode())


class MinimalRuntime(unittest.TestCase):
    def run_prepare(self, root, *paths, success=True):
        command = ['python3', str(Path(minimal.__file__)), str(root/'source.tar'),
                   str(root/'minimal.tar'), 'fixture:1']
        for path in paths:
            command += ['--root', path]
        result = subprocess.run(command, text=True, capture_output=True)
        self.assertEqual(result.returncode == 0, success, result.stderr)
        return result

    def test_projection_keeps_links_modes_config_and_excludes_unrequested_payload(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            fixture(root/'source.tar', [('usr/bin', b'', tarfile.DIRTYPE),
                ('usr/bin/python3', b'python3.14', tarfile.SYMTYPE),
                ('usr/bin/python3.14', b'python', tarfile.REGTYPE),
                ('usr/lib/large-unrelated', b'unrelated', tarfile.REGTYPE)])
            first = self.run_prepare(root, 'usr/bin/python3')
            digest = hashlib.sha256((root/'minimal.tar').read_bytes()).hexdigest()
            self.assertEqual(self.run_prepare(root, 'usr/bin/python3').stdout, first.stdout)
            self.assertEqual(hashlib.sha256((root/'minimal.tar').read_bytes()).hexdigest(), digest)
            with tarfile.open(root/'minimal.tar') as archive:
                manifest = json.load(archive.extractfile('manifest.json'))[0]
                config = json.load(archive.extractfile(manifest['Config']))
                self.assertEqual(config['config']['Env'], ['SOURCE=retained'])
                self.assertEqual(config['config']['Entrypoint'], ['/usr/local/bin/headless-retro'])
                with tarfile.open(fileobj=archive.extractfile(manifest['Layers'][0])) as layer:
                    self.assertNotIn('usr/lib/large-unrelated', layer.getnames())
                    self.assertEqual(layer.getmember('usr/bin/python3').linkname, 'python3.14')
                    self.assertEqual(layer.getmember('usr/bin/python3.14').mode, 0o755)
            # Corrupt output cannot reuse its source-bound receipt.
            (root/'minimal.tar').write_bytes(b'corrupt')
            self.run_prepare(root, 'usr/bin/python3')
            self.assertEqual(hashlib.sha256((root/'minimal.tar').read_bytes()).hexdigest(), digest)

    def test_hardlink_executable_retains_its_own_filename_and_bytes(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            fixture(root/'source.tar', [('usr/bin/tool', b'usr/lib/tool', tarfile.LNKTYPE),
                                       ('usr/lib/tool', b'payload', tarfile.REGTYPE)])
            self.run_prepare(root, 'usr/bin/tool')
            with tarfile.open(root/'minimal.tar') as archive:
                manifest = json.load(archive.extractfile('manifest.json'))[0]
                with tarfile.open(fileobj=archive.extractfile(manifest['Layers'][0])) as layer:
                    alias = layer.getmember('usr/bin/tool')
                    self.assertTrue(alias.isfile())
                    self.assertEqual(layer.extractfile(alias).read(), b'payload')
                    self.assertEqual(layer.extractfile('usr/lib/tool').read(), b'payload')
                    self.assertEqual(layer.getmember('tmp').mode, 0o1777)

    def test_requested_data_subtree_follows_directory_alias(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            fixture(root/'source.tar', [('usr/share/X11/xkb', b'../keyboard', tarfile.SYMTYPE),
                ('usr/share/keyboard', b'', tarfile.DIRTYPE),
                ('usr/share/keyboard/keycodes/evdev', b'keycodes', tarfile.REGTYPE)])
            self.run_prepare(root, 'usr/share/X11/xkb')
            receipt = json.loads((root/'minimal.tar.json').read_text())
            self.assertIn('usr/share/keyboard/keycodes/evdev', receipt['selected'])

    def test_changed_source_cannot_reuse_decompressed_layer_or_final_receipt(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for value in (b'first', b'changed'):
                fixture(root/'source.tar', [('usr/bin/program', value, tarfile.REGTYPE)])
                self.run_prepare(root, 'usr/bin/program')
                with tarfile.open(root/'minimal.tar') as archive:
                    manifest = json.load(archive.extractfile('manifest.json'))[0]
                    with tarfile.open(fileobj=archive.extractfile(manifest['Layers'][0])) as layer:
                        self.assertEqual(layer.extractfile('usr/bin/program').read(), value)
                receipt = json.loads((root/'minimal.tar.json').read_text())
                self.assertEqual(receipt['binding']['source_sha256'],
                                 hashlib.sha256((root/'source.tar').read_bytes()).hexdigest())

    def test_missing_escaping_cyclic_and_whiteout_entries_reject(self):
        for entries, path in [([], 'missing'),
            ([('link', b'../../escape', tarfile.SYMTYPE)], 'link'),
            ([('a', b'b', tarfile.SYMTYPE), ('b', b'a', tarfile.SYMTYPE)], 'a'),
            ([('.wh.deleted', b'', tarfile.REGTYPE)], 'missing')]:
            with self.subTest(path=path), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                fixture(root/'source.tar', entries)
                self.run_prepare(root, path, success=False)
                self.assertFalse((root/'minimal.tar.json').exists())

    @unittest.skipUnless(shutil.which('readelf') and Path('/lib/x86_64-linux-gnu/libc.so.6').is_file(),
                         'requires Linux x86-64 ELF fixtures and readelf')
    def test_public_projection_resolves_elf_closure_and_rejects_missing_library(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            loader = Path('/lib64/ld-linux-x86-64.so.2').read_bytes()
            entries = [('usr/bin/program', Path('/bin/true').read_bytes(), tarfile.REGTYPE),
                       ('usr/lib/x86_64-linux-gnu/libc.so.6', Path('/lib/x86_64-linux-gnu/libc.so.6').read_bytes(), tarfile.REGTYPE),
                       ('usr/lib/x86_64-linux-gnu/ld-linux-x86-64.so.2', loader, tarfile.REGTYPE),
                       ('lib64/ld-linux-x86-64.so.2', loader, tarfile.REGTYPE)]
            fixture(root/'source.tar', entries)
            self.run_prepare(root, 'usr/bin/program')
            receipt = json.loads((root/'minimal.tar.json').read_text())
            self.assertIn('usr/lib/x86_64-linux-gnu/libc.so.6', receipt['selected'])
            self.assertIn('lib64/ld-linux-x86-64.so.2', receipt['selected'])
            fixture(root/'source.tar', entries[:1])
            result = self.run_prepare(root, 'usr/bin/program', success=False)
            self.assertIn('missing retained ELF dependency libc.so.6', result.stderr)

    @unittest.skipUnless(shutil.which('readelf') and Path('/bin/true').is_file(),
                         'requires readelf and a Linux ELF fixture')
    def test_real_elf_metadata_includes_loader_and_required_libraries(self):
        dependencies = minimal.elf_dependencies(Path('/bin/true').read_bytes())
        self.assertIn('libc.so.6', dependencies)
        self.assertTrue(any(value.startswith('/') and 'ld-linux' in value for value in dependencies))
        self.assertEqual(minimal.elf_dependencies(b'ordinary data'), [])


if __name__ == '__main__':
    unittest.main()
