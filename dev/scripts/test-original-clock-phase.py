#!/usr/bin/env python3
"""Publication timing admission with synthetic measurements, no executable bindings."""
import copy
import importlib.util
from pathlib import Path
import unittest


def load(name):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(name + '.py'))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


watch = load('original-update-watchpoint')
recorder = load('original-update-recorder')
CONFIG = dict(clock_phase='published_pending_update', clock='total', step='movement',
              pending_step='pending', warmup='warmup', bios='candidate', measurement_bios='sample')


def row(update, total, step, sample):
    raw = dict(total=total, movement=step, pending=step, warmup=0,
               sample=sample, candidate=sample)
    return dict(execution='stopped', boundary='ordinary_update_start', update=update,
                **watch.clock_snapshot(raw, CONFIG))


class Session:
    def __init__(self, rows):
        self.rows = iter(rows)

    def next_boundary(self, timeout):
        self.current = next(self.rows)

    def snapshot(self):
        return copy.deepcopy(self.current)


class ClockPhase(unittest.TestCase):
    def test_publication_excludes_pending_increment_before_comparing_completed_updates(self):
        rows = [row(20, 1200, 200, 100), row(21, 1402, 202, 102), row(22, 1606, 204, 105)]
        output = []
        recorder.record(Session(rows), 400, output.append, require_bios=True, clock_only=True)
        self.assertEqual([r['clock_scale'] for r in output], [1000, 1200, 1402])
        self.assertEqual([r['consumed_clock_step'] for r in output], [0, 200, 202])
        self.assertEqual([r['elapsed_bios_ticks'] for r in output], [None, 2, 3])
        self.assertTrue(all(r['acquisition_scope'] == 'clock_only' for r in output))

    def test_zero_candidate_cannot_replace_the_engine_measurement_sample(self):
        raw = dict(total=1200, movement=200, pending=200, warmup=0, sample=100, candidate=0)
        with self.assertRaisesRegex(ValueError, 'BIOS candidate'):
            watch.clock_snapshot(raw, CONFIG)

    def test_counter_entry_is_not_a_known_forthcoming_clock_step(self):
        with self.assertRaisesRegex(ValueError, 'clock phase'):
            watch.clock_snapshot({}, CONFIG | dict(clock_phase='before_measurement'))

    def test_wrong_publication_phase_rejects_before_recording(self):
        for change in [dict(clock_scale=1200), dict(next_step=202),
                       dict(bios_ticks=0), dict(clock_publication={})]:
            with self.subTest(change=change), self.assertRaisesRegex(ValueError, 'publication'):
                recorder.record(Session([row(20, 1200, 200, 100) | change]), 1,
                                self.fail, clock_only=True)

    def test_warmup_or_different_calculated_and_movement_steps_remain_unsupported(self):
        raw = dict(total=1200, movement=200, pending=200, warmup=0, sample=100, candidate=100)
        for change in [dict(warmup=1), dict(pending=202), dict(total=100)]:
            with self.subTest(change=change), self.assertRaisesRegex(ValueError, 'settled'):
                watch.clock_snapshot(raw | change, CONFIG)


if __name__ == '__main__':
    unittest.main()
