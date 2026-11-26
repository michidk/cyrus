#!/usr/bin/env python3
"""Check source-bound layer preparation without Docker or game files."""
import hashlib
import io
import json
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest

SCRIPT = Path(__file__).with_name('layer-runtime-archive.py')


def fixture(path, entries):
    layer = io.BytesIO()
    with tarfile.open(fileobj=layer, mode='w') as tar:
        for entry in entries:
            name, data, kind = entry[:3]
            member = tarfile.TarInfo(name)
            member.type = kind
            member.mode = 0o755
            member.pax_headers = entry[3] if len(entry) > 3 else {}
            member.size = len(data) if kind == tarfile.REGTYPE else 0
            member.linkname = data.decode() if kind != tarfile.REGTYPE else ''
            tar.addfile(member, io.BytesIO(data) if member.isfile() else None)
    payload = layer.getvalue()
    config = json.dumps({'architecture': 'amd64', 'os': 'linux',
                         'config': {'Env': ['RETAIN=yes'], 'Entrypoint': ['/entry']},
                         'rootfs': {'type': 'layers', 'diff_ids': ['sha256:' + hashlib.sha256(payload).hexdigest()]}}).encode()
    with tarfile.open(path, 'w') as tar:
        for name, data in [('config.json', config), ('layer.tar', payload),
                           ('manifest.json', json.dumps([{'Config': 'config.json', 'RepoTags': ['fixture:1'],
                                                          'Layers': ['layer.tar']}]).encode())]:
            member = tarfile.TarInfo(name)
            member.size = len(data)
            tar.addfile(member, io.BytesIO(data))


class LayerPreparation(unittest.TestCase):
    def prepare(self, source, output, expected=0):
        result = subprocess.run(['python3', str(SCRIPT), str(source), str(output), 'fixture:1',
                                 '--layer-bytes', '1024'], text=True, capture_output=True)
        self.assertEqual(result.returncode, expected, result.stderr)
        return result

    def test_smaller_layers_preserve_bytes_modes_links_and_runtime_config(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            fixture(root/'source.tar', [('entry', b'first' * 300, tarfile.REGTYPE),
                                       ('second', b'last', tarfile.REGTYPE),
                                       ('link', b'entry', tarfile.SYMTYPE)])
            self.prepare(root/'source.tar', root/'derived.tar')
            with tarfile.open(root/'derived.tar') as archive:
                manifest = json.load(archive.extractfile('manifest.json'))[0]
                config = json.load(archive.extractfile(manifest['Config']))
                self.assertEqual(config['config'], {'Env': ['RETAIN=yes'], 'Entrypoint': ['/entry']})
                self.assertGreater(len(manifest['Layers']), 1)
                members = {}
                for name, diff in zip(manifest['Layers'], config['rootfs']['diff_ids']):
                    import gzip
                    data = gzip.decompress(archive.extractfile(name).read())
                    self.assertEqual('sha256:' + hashlib.sha256(data).hexdigest(), diff)
                    with tarfile.open(fileobj=io.BytesIO(data)) as layer:
                        for member in layer:
                            members[member.name] = (member.mode, member.linkname,
                                                    layer.extractfile(member).read() if member.isfile() else None)
                self.assertEqual(members, {'entry': (0o755, '', b'first' * 300),
                                           'second': (0o755, '', b'last'), 'link': (0o755, 'entry', None)})

    def test_completed_preparation_is_reused_and_corrupted_output_is_rebuilt(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            fixture(root/'source.tar', [('entry', b'a' * 2048, tarfile.REGTYPE)])
            self.prepare(root/'source.tar', root/'derived.tar')
            output = root/'derived.tar'
            chunks = list((root/'derived.tar.layers').glob('*.tar.gz'))
            chunk_times = [chunk.stat().st_mtime_ns for chunk in chunks]
            before = output.stat().st_mtime_ns
            self.prepare(root/'source.tar', output)
            self.assertEqual(output.stat().st_mtime_ns, before)
            output.write_bytes(b'interrupted')
            self.prepare(root/'source.tar', output)
            self.assertTrue(tarfile.is_tarfile(output))
            self.assertEqual([chunk.stat().st_mtime_ns for chunk in chunks], chunk_times)

    def test_binary_capability_metadata_survives_layer_conversion(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            value = b'\x01\x80\x00\x02'.decode('utf-8', 'surrogateescape')
            fixture(root/'source.tar', [('entry', b'a' * 2048, tarfile.REGTYPE,
                                        {'SCHILY.xattr.security.capability': value})])
            self.prepare(root/'source.tar', root/'derived.tar')
            with tarfile.open(root/'derived.tar') as archive:
                manifest = json.load(archive.extractfile('manifest.json'))[0]
                with tarfile.open(fileobj=archive.extractfile(manifest['Layers'][0]), mode='r|*') as layer:
                    entry = next(iter(layer))
                    self.assertEqual(entry.pax_headers['SCHILY.xattr.security.capability'], value)

    def test_changed_source_invalidates_prepared_layers(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            fixture(root/'source.tar', [('entry', b'a' * 2048, tarfile.REGTYPE)])
            self.prepare(root/'source.tar', root/'derived.tar')
            original = hashlib.sha256((root/'derived.tar').read_bytes()).hexdigest()
            fixture(root/'source.tar', [('entry', b'b' * 2048, tarfile.REGTYPE)])
            self.prepare(root/'source.tar', root/'derived.tar')
            self.assertNotEqual(hashlib.sha256((root/'derived.tar').read_bytes()).hexdigest(), original)

    def test_unsupported_whiteout_or_hardlink_never_publishes_an_archive(self):
        for name, data, kind in [('dir/.wh.file', b'', tarfile.REGTYPE),
                                  ('alias', b'entry', tarfile.LNKTYPE)]:
            with self.subTest(name=name), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                fixture(root/'source.tar', [('entry', b'a' * 2048, tarfile.REGTYPE), (name, data, kind)])
                result = self.prepare(root/'source.tar', root/'derived.tar', expected=1)
                self.assertIn('unsupported', result.stderr)
                self.assertFalse((root/'derived.tar').exists())


if __name__ == '__main__':
    unittest.main()
