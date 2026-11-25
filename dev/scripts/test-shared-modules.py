#!/usr/bin/env python3
"""Shared driver modules import normally and keep documented CLI compatibility."""
import ast
import os
import runpy
import subprocess
import sys
import unittest
from pathlib import Path

import original_fight
import original_scenario
import replay_chain


class SharedModules(unittest.TestCase):
    def test_documented_commands_forward_to_importable_modules(self):
        root = Path(__file__).parent
        for module in ('native_session_capture', 'native_trace', 'book_audio_continuity', 'replay_chain',
                       'original_scenario', 'original_fight'):
            with self.subTest(module=module):
                command = root / (module.replace('_', '-') + '.py')
                canonical = root / (module + '.py')
                self.assertTrue(canonical.is_file())
                legacy = subprocess.run([sys.executable, str(command), '--help'],
                                        capture_output=True, text=True, timeout=10, check=False)
                current = subprocess.run([sys.executable, str(canonical), '--help'],
                                         capture_output=True, text=True, timeout=10, check=False)
                self.assertEqual(legacy.returncode, 0, legacy.stderr)
                self.assertEqual(current.returncode, 0, current.stderr)
                self.assertEqual(legacy.stdout.replace(command.name, canonical.name), current.stdout)

    def test_replay_consumers_share_one_imported_driver(self):
        for name, binding in [('run-native-session.py', 'replay'),
                              ('native-source-clock.py', 'replay'),
                              ('verify-original-slot.py', 'driver')]:
            with self.subTest(command=name):
                namespace = runpy.run_path(str(Path(__file__).with_name(name)))
                self.assertIs(namespace[binding], replay_chain)

    def test_original_scenario_reuses_the_imported_sampler(self):
        self.assertIs(original_scenario.fight, original_fight)

    def test_trace_commands_remain_directly_executable(self):
        for name in ('native-trace.py', 'native_trace.py'):
            self.assertTrue(os.access(Path(__file__).with_name(name), os.X_OK))

    def test_compatibility_commands_contain_only_import_and_main(self):
        for name in ('native-session-capture', 'native-trace', 'book-audio-continuity', 'replay-chain',
                     'original-scenario', 'original-fight'):
            tree = ast.parse(Path(__file__).with_name(name + '.py').read_text())
            self.assertEqual([type(node).__name__ for node in tree.body],
                             ['Expr', 'ImportFrom', 'If'])


if __name__ == '__main__':
    unittest.main()
