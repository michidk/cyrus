#!/usr/bin/env python3
"""Reject incomplete or contradictory production torch observations."""
import importlib.util
from pathlib import Path
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('torch_acceptance', Path(__file__).with_name('torch-acceptance.py'))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class TorchAcceptanceTest(unittest.TestCase):
    def test_retention_requires_refresh_and_continuation(self):
        module.verify([100, 100, 100], retained=True)
        for counters in ([], [100], [73, 73, 73], [100, 99, 100], [0, 0, 0]):
            with self.subTest(counters=counters), self.assertRaises(AssertionError):
                module.verify(counters, retained=True)

    def test_disabled_control_requires_countdown(self):
        module.verify([73, 69, 60], retained=False)
        for counters in ([], [73], [73, 73, 73], [73, 100, 99]):
            with self.subTest(counters=counters), self.assertRaises(AssertionError):
                module.verify(counters, retained=False)

    def test_script_failure_restores_prior_state(self):
        commands = []

        def call(addr, command):
            commands.append(command)
            if command['type'] == 'state':
                return {'scene': 'START', 'cheats': {'entries': [
                    {'name': 'goldfinger', 'value': 2}]},
                    'scripts': {'failed': 1}}
            if command['type'] == 'scripts':
                return {'actors': [{'control': {'torch_burning': 12}}]}
            return {}

        with patch.object(module.trace, 'wait_live'), \
                patch.object(module.trace, 'player_index', return_value=0), \
                patch.object(module.trace, 'call', side_effect=call), \
                patch('sys.argv', ['torch-acceptance.py']):
            with self.assertRaisesRegex(AssertionError, 'failed'):
                module.main()
        self.assertEqual(commands[-2:], [
            {'type': 'set_torch', 'burning': 12},
            {'type': 'cheat', 'name': 'goldfinger', 'value': 2}])


if __name__ == '__main__':
    unittest.main()
