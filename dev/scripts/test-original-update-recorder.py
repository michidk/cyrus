#!/usr/bin/env python3
"""Synthetic stopped-boundary controls; no installed data or executable bindings."""
import copy
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('recorder', Path(__file__).with_name('original-update-recorder.py'))
recorder = importlib.util.module_from_spec(spec)
spec.loader.exec_module(recorder)


def snapshot(update, clock, step=32):
    return dict(execution='stopped', boundary='ordinary_update_start', update=update,
                clock_scale=clock, next_step=step,
                player=dict(position_q8=[update, 0, 0], animation=0, fall=0,
                            support_owner=None, link=None, previous_local_q8=[0, 0, 0]))


class Session:
    def __init__(self, rows, torn=False):
        self.rows = iter(rows)
        self.current = None
        self.reads = 0
        self.torn = torn

    def next_boundary(self, timeout):
        self.current = next(self.rows)
        self.reads = 0

    def snapshot(self):
        result = copy.deepcopy(self.current)
        self.reads += 1
        if self.torn and self.reads == 2:
            result['player']['position_q8'][0] += 1
        return result


class Recorder(unittest.TestCase):
    def test_stops_on_consumed_clock_not_host_time_or_sample_count(self):
        rows = [snapshot(7, 100, 46), snapshot(8, 146, 32), snapshot(9, 178, 18)]
        output = []
        self.assertEqual(recorder.record(Session(rows), 70, output.append), 2)
        self.assertEqual([x['elapsed_scale'] for x in output], [0, 46, 78])
        self.assertEqual([x['consumed_clock_step'] for x in output], [0, 46, 32])

    def test_duplicate_skipped_and_reset_counters_rejected(self):
        for update in [7, 6, 9]:
            with self.subTest(update=update), self.assertRaisesRegex(ValueError, 'counter'):
                recorder.record(Session([snapshot(7, 100), snapshot(update, 132)]), 64, lambda _: None)

    def test_equal_clock_is_not_completion(self):
        for clock in [100, 99, 133]:
            with self.subTest(clock=clock), self.assertRaisesRegex(ValueError, 'consumed'):
                recorder.record(Session([snapshot(7, 100), snapshot(8, clock)]), 64, lambda _: None)

    def test_torn_stopped_snapshot_rejected(self):
        with self.assertRaisesRegex(ValueError, 'changed'):
            recorder.record(Session([snapshot(7, 100)], torn=True), 64, lambda _: None)

    def test_running_partial_and_malformed_snapshots_rejected(self):
        for change in [dict(execution='running'), dict(boundary='clock_read'), dict(next_step=0),
                       dict(update=True), dict(player={})]:
            with self.subTest(change=change), self.assertRaises(ValueError):
                recorder.record(Session([snapshot(7, 100) | change]), 64, lambda _: None)

    def test_backend_stall_propagates_without_accepting_sample(self):
        class Stalled(Session):
            def next_boundary(self, timeout):
                raise TimeoutError('boundary stalled')
        output = []
        with self.assertRaises(TimeoutError):
            recorder.record(Stalled([]), 64, output.append)
        self.assertEqual(output, [])


class WatchpointBackend(unittest.TestCase):
    def test_projection_uses_reader_state_and_explicit_bindings(self):
        import json
        import types
        backend_spec = importlib.util.spec_from_file_location(
            'watchpoint', Path(__file__).with_name('original-update-watchpoint.py'))
        backend = importlib.util.module_from_spec(backend_spec)
        backend_spec.loader.exec_module(backend)
        actor = dict(selected=True, **snapshot(7, 100)['player'])
        state = dict(actors=[actor], console_mode='closed', frame=7, clock=100, step=32)
        session = object.__new__(backend.Watchpoint)
        session.config = dict(counter='frame', clock='clock', step='step',
                              player_fields={name: name for name in recorder.FIELDS})
        session.reader = types.SimpleNamespace(main=lambda: print(json.dumps(state)))
        session.stopped = True
        self.assertEqual(session.snapshot(), snapshot(7, 100))
        session.stopped = False
        with self.assertRaisesRegex(RuntimeError, 'running'):
            session.snapshot()
        session.stopped = True
        state['console_mode'] = 'open'
        with self.assertRaisesRegex(RuntimeError, 'unavailable'):
            session.snapshot()


if __name__ == '__main__':
    unittest.main()
