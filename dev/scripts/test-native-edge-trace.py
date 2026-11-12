#!/usr/bin/env python3
"""Public scheduled edge capture admits only complete entry/endpoint sequences."""
import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import sys

spec = importlib.util.spec_from_file_location('edge', Path(__file__).with_name('native-edge-trace.py'))
edge = importlib.util.module_from_spec(spec)
spec.loader.exec_module(edge)


class EdgeCapture(unittest.TestCase):
    def fixture(self):
        events = edge.schedule('ArrowUp', 1, 3, 5)
        boundaries = []
        for i, event in enumerate(events):
            boundaries.append(dict(event, update=20+i, generation=2, game_seconds=i/6,
                player=dict(actors=[dict(name='CYRUS', source='player',
                    attributes=[0, 0, 0, 0 if i >= 4 else 100],
                    control=dict(falling_state=1 if i in (2, 3) else 0,
                                 queued_motion=0, sliding_state=0, living_state=0),
                    animation=dict(current_group=74 if i >= 4 else 56),
                    spatial=dict(effective=dict(position=[0, i*256, 0])))])))
        return events, dict(boundaries=boundaries)

    def test_public_schedule_arms_before_trigger_and_retains_endpoint_continuation(self):
        events, response = self.fixture()
        with tempfile.TemporaryDirectory() as directory:
            out = Path(directory)/'edge.jsonl'
            out.write_text('stale acceptance')
            with patch.object(sys, 'argv', ['edge', '--out', str(out), '--release-update', '3', '--updates', '5']), \
                 patch.object(edge.trace, 'wait_live'), patch.object(edge.trace, 'call', return_value=response) as call:
                edge.main()
            self.assertEqual(call.call_count, 1)
            self.assertEqual(call.call_args.args[1], dict(type='schedule_input', events=events))
            self.assertEqual(len(out.read_text().splitlines()), 6)
            self.assertTrue(Path(str(out)+'.raw.json').exists())
        rows = edge.validate(response, events)
        self.assertEqual([r['falling'] for r in rows], [0, 0, 1, 1, 0, 0])
        self.assertEqual([r['health'] for r in rows], [100, 100, 100, 100, 0, 0])

    def test_exact_acceptance_rejects_gaps_bad_ack_and_missing_entry_or_endpoint(self):
        for change in ('gap', 'duplicate', 'generation', 'clock', 'missing', 'offset', 'ack',
                       'already_falling', 'attached', 'queued', 'unavailable', 'before_press', 'no_entry', 'no_endpoint'):
            events, response = self.fixture()
            rows = response['boundaries']
            if change == 'gap':
                for row in rows[2:]: row['update'] += 1
            elif change == 'duplicate': rows[2]['update'] = rows[1]['update']
            elif change == 'generation': rows[2]['generation'] += 1
            elif change == 'clock': rows[2]['game_seconds'] = rows[1]['game_seconds']
            elif change == 'missing': rows.pop()
            elif change == 'offset': rows[2]['after_updates'] += 1
            elif change == 'ack': rows[1]['action'] = 'release'
            elif change == 'already_falling': rows[0]['player']['actors'][0]['control']['falling_state'] = 1
            elif change == 'attached': rows[0]['player']['actors'][0]['control']['living_state'] = 15
            elif change == 'queued': rows[0]['player']['actors'][0]['control']['queued_motion'] = 1
            elif change == 'unavailable': rows[3]['player']['actors'][0]['control'].pop('falling_state')
            elif change == 'before_press': rows[1]['player']['actors'][0]['control']['falling_state'] = 1
            elif change == 'no_entry':
                for row in rows: row['player']['actors'][0]['control']['falling_state'] = 0
            else:
                for row in rows: row['player']['actors'][0]['attributes'][3] = 100
            with self.subTest(change=change), self.assertRaises(ValueError):
                edge.validate(response, events)

    def test_rejected_acquisition_is_retained_without_accepted_output(self):
        _, response = self.fixture()
        response['boundaries'].pop()
        with tempfile.TemporaryDirectory() as directory:
            out = Path(directory)/'edge.jsonl'
            out.write_text('stale acceptance')
            with patch.object(sys, 'argv', ['edge', '--out', str(out), '--release-update', '3', '--updates', '5']), \
                 patch.object(edge.trace, 'wait_live'), patch.object(edge.trace, 'call', return_value=response):
                with self.assertRaisesRegex(ValueError, 'incomplete'):
                    edge.main()
            self.assertFalse(out.exists())
            self.assertTrue(Path(str(out)+'.raw.json').exists())

    def test_invalid_schedule_never_initiates_input(self):
        for press, release, updates in ((0, 2, 5), (3, 2, 5), (1, 5, 5), (1, 2, 256)):
            with self.assertRaises(ValueError): edge.schedule('ArrowUp', press, release, updates)


if __name__ == '__main__':
    unittest.main()
