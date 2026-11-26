#!/usr/bin/env python3
"""Recover original runtime definitions without overwriting per-run state."""
import importlib.util
from pathlib import Path
import tempfile
import unittest

MODULE = Path(__file__).with_name('prepare-original-definitions.py')


class Definitions(unittest.TestCase):
    def test_recovers_missing_nested_definitions_and_preserves_run_edits(self):
        spec = importlib.util.spec_from_file_location('definitions', MODULE)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source, run = root / 'install', root / 'run'
            (source / 'soup386').mkdir(parents=True)
            (source / 'soup386/SOUP386.DEF').write_text('installed definition')
            (source / 'system').mkdir()
            (source / 'fonts').mkdir()
            (source / 'fonts/REDSEL.FNT').write_bytes(b'installed font')
            (source / '3DfxSpl2.dll').write_bytes(b'installed splash')
            (source / 'system/pointers.bmp').write_bytes(b'installed presentation')
            run.mkdir()
            (run / 'SYSTEM.INI').write_text('per-run settings')
            (run / 'RGFX.EXE').write_bytes(b'per-run executable')
            module.prepare(source, run)
            self.assertEqual((run / 'soup386/SOUP386.DEF').read_text(), 'installed definition')
            self.assertEqual((run / 'system/pointers.bmp').read_bytes(), b'installed presentation')
            self.assertEqual((run / 'fonts/REDSEL.FNT').read_bytes(), b'installed font')
            self.assertEqual((run / '3DfxSpl2.dll').read_bytes(), b'installed splash')
            self.assertFalse((run / '3DfxSpl2.dll').is_symlink())
            (run / '3DfxSpl2.dll').write_bytes(b'private splash')
            (run / 'system/pointers.bmp').write_bytes(b'preserved run edit')
            module.prepare(source, run)
            self.assertEqual((run / 'system/pointers.bmp').read_bytes(), b'preserved run edit')
            self.assertEqual((run / 'SYSTEM.INI').read_text(), 'per-run settings')
            self.assertEqual((run / '3DfxSpl2.dll').read_bytes(), b'private splash')
            self.assertEqual((source / '3DfxSpl2.dll').read_bytes(), b'installed splash')
            self.assertEqual((run / 'RGFX.EXE').read_bytes(), b'per-run executable')
            self.assertEqual((source / 'system/pointers.bmp').read_bytes(), b'installed presentation')

    def test_map_state_writes_stay_in_run_and_asset_links_survive_repeated_preparation(self):
        spec = importlib.util.spec_from_file_location('definitions', MODULE)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source, run = root / 'install/maps', root / 'run'
            source.mkdir(parents=True)
            run.mkdir()
            (source / 'START.RGM').write_bytes(b'installed map')
            (source / 'START.TSG').write_bytes(b'unrelated saved state')
            (run / 'maps').symlink_to(source, target_is_directory=True)
            module.prepare_map_state(run)
            self.assertFalse((run / 'maps').is_symlink())
            self.assertTrue((run / 'maps/START.RGM').is_symlink())
            self.assertEqual((run / 'maps/START.RGM').read_bytes(), b'installed map')
            self.assertFalse((run / 'maps/START.TSG').exists())
            (run / 'maps/START.TSG').write_bytes(b'new world state')
            module.prepare_map_state(run)
            self.assertEqual((run / 'maps/START.TSG').read_bytes(), b'new world state')
            self.assertEqual((source / 'START.TSG').read_bytes(), b'unrelated saved state')

    def test_missing_install_definition_fails_without_mutating_the_run(self):
        spec = importlib.util.spec_from_file_location('definitions', MODULE)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source, run = root / 'install', root / 'run'
            source.mkdir()
            run.mkdir()
            with self.assertRaisesRegex(ValueError, 'SOUP386.DEF'):
                module.prepare(source, run)
            self.assertEqual(list(run.iterdir()), [])

    def test_rejects_a_linked_writable_definition_directory(self):
        spec = importlib.util.spec_from_file_location('definitions', MODULE)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source, run = root / 'install', root / 'run'
            (source / 'soup386').mkdir(parents=True)
            (source / 'soup386/SOUP386.DEF').write_text('definition')
            (source / 'system').mkdir()
            (source / 'fonts').mkdir()
            (source / 'fonts/REDSEL.FNT').write_bytes(b'installed font')
            run.mkdir()
            (run / 'soup386').symlink_to(source / 'soup386', target_is_directory=True)
            with self.assertRaisesRegex(ValueError, 'linked'):
                module.prepare(source, run)


if __name__ == '__main__':
    unittest.main()
