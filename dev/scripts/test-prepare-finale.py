#!/usr/bin/env python3
"""Finale entry uses the console prerequisite without bypassing the confrontation."""
import copy
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('finale', Path(__file__).with_name('prepare-finale.py'))
finale = importlib.util.module_from_spec(spec)
spec.loader.exec_module(finale)


class FinaleEntry(unittest.TestCase):
    def run_entry(self, world=30, won=0, grant=True, owned=0, marker=True):
        calls = []
        state = {'state': 'InGame', 'scripts': {'world': world, 'failed': 0},
                 'script_hud': {'inventory': {'counts': {'79': owned}}}}

        def call(command):
            calls.append(command)
            if command['type'] == 'state':
                return copy.deepcopy(state)
            if command['type'] == 'scripts':
                return {'world': world, 'flags': {'flags': [
                    {'definition': {'name': 'won_game'}, 'value': won}]}}
            if command.get('line') == 'item add,79' and grant:
                state['script_hud']['inventory']['counts']['79'] += 1
            if command['type'] == 'console' and 'line' in command:
                text = ('item add: 79' if command['line'] == 'item add,79' else
                        'getmapmarker: 32' if marker else 'marker unavailable')
                return {'console': {'lines': [{'report': True, 'text': text}]}}
            return {}

        return calls, lambda: finale.prepare(call)

    def test_setup_grants_item_then_uses_installed_marker_without_winning(self):
        calls, run = self.run_entry()
        run()
        self.assertEqual([c for c in calls if c['type'] == 'console'], [
            {'type': 'console', 'line': 'item add,79'},
            {'type': 'console', 'line': 'getmapmarker 32'},
            {'type': 'console', 'open': False}])
        self.assertFalse(any(c['type'] in ['set_attribute', 'place_player'] for c in calls))

    def test_console_acknowledgement_without_inventory_change_fails(self):
        _, run = self.run_entry(grant=False)
        with self.assertRaisesRegex(ValueError, 'did not grant'):
            run()

    def test_console_marker_error_fails_even_when_transport_acknowledges(self):
        _, run = self.run_entry(marker=False)
        with self.assertRaisesRegex(ValueError, 'console rejected getmapmarker'):
            run()

    def test_wrong_world_and_already_won_refuse_before_mutation(self):
        for args in [{'world': 1}, {'won': 1}]:
            calls, run = self.run_entry(**args)
            with self.assertRaises(ValueError):
                run()
            self.assertFalse(any(c['type'] == 'console' for c in calls))

    def test_reusing_prerequisite_does_not_duplicate_it(self):
        calls, run = self.run_entry(owned=1)
        run()
        self.assertFalse(any(c.get('line') == 'item add,79' for c in calls))


if __name__ == '__main__':
    unittest.main()
