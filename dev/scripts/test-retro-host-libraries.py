#!/usr/bin/env python3
"""Retained host dependency preparation without Docker or installed game data."""
import io
import json
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest

SCRIPT = Path(__file__).with_name('prepare-retro-host.py')


def archive(path, entries):
    payload = io.BytesIO()
    with tarfile.open(fileobj=payload, mode='w') as layer:
        for name, data, link in entries:
            member = tarfile.TarInfo(name)
            if link:
                member.type, member.linkname = tarfile.SYMTYPE, link
            else:
                member.size, member.mode = len(data), 0o755
            layer.addfile(member, None if link else io.BytesIO(data))
    with tarfile.open(path, 'w') as outer:
        for name, data in [('layer.tar', payload.getvalue()), ('manifest.json',
                json.dumps([dict(Layers=['layer.tar'])]).encode())]:
            member = tarfile.TarInfo(name)
            member.size = len(data)
            outer.addfile(member, io.BytesIO(data))


class HostLibraries(unittest.TestCase):
    def run_prepare(self, source, root, *names):
        return subprocess.run(['python3', str(SCRIPT), str(source), str(root), *names],
                              capture_output=True, text=True)

    def test_recovers_nested_soname_target_and_reuses_source_bound_result(self):
        with tempfile.TemporaryDirectory() as d:
            source, root = Path(d)/'image.tar', Path(d)/'host'
            archive(source, [('usr/lib/libfixture.so.1', b'', 'compat/libfixture.so.1'),
                             ('usr/lib/compat/libfixture.so.1', b'library', ''),
                             ('game/asset', b'excluded', '')])
            result = self.run_prepare(source, root, 'libfixture.so.1')
            self.assertEqual(result.returncode, 0, result.stderr)
            target = root/'usr/lib/compat/libfixture.so.1'
            self.assertEqual((root/'usr/lib/libfixture.so.1').read_bytes(), b'library')
            self.assertFalse((root/'game').exists())
            before = target.stat().st_mtime_ns
            self.assertEqual(self.run_prepare(source, root, 'libfixture.so.1').returncode, 0)
            self.assertEqual(target.stat().st_mtime_ns, before)
            target.write_bytes(b'broken')
            self.assertEqual(self.run_prepare(source, root, 'libfixture.so.1').returncode, 0)
            self.assertEqual(target.read_bytes(), b'library')
            target.chmod(0o600)
            self.assertEqual(self.run_prepare(source, root, 'libfixture.so.1').returncode, 0)
            self.assertEqual(target.stat().st_mode & 0o777, 0o755)

    def test_missing_dependency_and_escaping_link_reject(self):
        for entries in [[], [('usr/lib/libfixture.so.1', b'', '../../../outside')]]:
            with tempfile.TemporaryDirectory() as d:
                source, root = Path(d)/'image.tar', Path(d)/'host'
                archive(source, entries)
                self.assertNotEqual(self.run_prepare(source, root, 'libfixture.so.1').returncode, 0)
                self.assertFalse((root/'host-preparation.json').exists())

    def test_symlink_cycle_and_preexisting_parent_escape_reject(self):
        with tempfile.TemporaryDirectory() as d:
            source, root = Path(d)/'image.tar', Path(d)/'host'
            archive(source, [('usr/lib/libfixture.so.1', b'', 'libfixture.so.2'),
                             ('usr/lib/libfixture.so.2', b'', 'libfixture.so.1')])
            self.assertNotEqual(self.run_prepare(source, root, 'libfixture.so.1').returncode, 0)
            self.assertFalse((root/'host-preparation.json').exists())
        with tempfile.TemporaryDirectory() as d:
            source, root, outside = Path(d)/'image.tar', Path(d)/'host', Path(d)/'outside'
            root.mkdir(); outside.mkdir(); (root/'usr').symlink_to(outside)
            archive(source, [('usr/lib/libfixture.so.1', b'library', '')])
            self.assertNotEqual(self.run_prepare(source, root, 'libfixture.so.1').returncode, 0)
            self.assertFalse((outside/'lib').exists())

    def test_absolute_library_link_is_relocated_inside_owned_root(self):
        with tempfile.TemporaryDirectory() as d:
            source, root = Path(d)/'image.tar', Path(d)/'host'
            archive(source, [('usr/lib/libfixture.so.1', b'', '/usr/lib/libfixture.so.2'),
                             ('usr/lib/libfixture.so.2', b'library', '')])
            result = self.run_prepare(source, root, 'libfixture.so.1')
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual((root/'usr/lib/libfixture.so.1').resolve(), root/'usr/lib/libfixture.so.2')


if __name__ == '__main__':
    unittest.main()
