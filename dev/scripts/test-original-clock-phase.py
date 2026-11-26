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

    def test_restore_baseline_and_second_timer_read_bind_the_forthcoming_step(self):
        # A blocking restore moved the baseline from 100 to 140. The sample
        # store read 141; the remainder addition independently confirms one tick.
        sample = row(20, 1200, 200, 141)
        sample['bios_measurement'] = dict(phase='before_publication', sample=141,
            baseline=140, remainder_before=3, remainder_after=4)
        self.assertEqual(recorder.next_bios_input(sample), 1)
        rows = [sample, row(21, 1402, 202, 190)]
        rows[1]['bios_measurement'] = dict(phase='before_publication', sample=190,
            baseline=190, remainder_before=5, remainder_after=5)
        output = []
        recorder.record(Session(rows), 200, output.append, clock_only=True)
        self.assertEqual([r['next_elapsed_bios_ticks'] for r in output], [1, 0])
        self.assertEqual(output[1]['elapsed_bios_ticks'], 49)
        tape = recorder.replay_clock_inputs(output)
        self.assertEqual(tape, dict(frame_scales=[200], bios_ticks=[1]))
        class Control:
            update = 0
            commands = []
            def request(self, command):
                if command['type'] == 'state':
                    return dict(scripts=dict(ordinary_updates=self.update, failed=0),
                                clock=dict(continuation=dict(step=dict(frame_scale=200))))
                if command['type'] == 'scripts':
                    return dict(actors=[], total=0)
                if command['type'] == 'replay_frame':
                    self.commands.append(command.copy())
                    self.update += 1
                    return {}
                raise AssertionError(command)
        control = Control()
        native = load('compare-segment').NativeFrameSession(control, lambda *_: {}, **tape)
        native.advance_to(1)
        self.assertEqual(control.commands, [dict(type='replay_frame', frame_scale=200,
                                                 elapsed_bios_ticks=1)])

    def test_replay_rejects_missing_measurement_wrong_phase_or_unconsumed_tail(self):
        sample = row(20, 1200, 200, 141)
        with self.assertRaisesRegex(ValueError, 'measurement'):
            recorder.next_bios_input(sample)
        measurement = dict(phase='before_publication', sample=141, baseline=140,
                           remainder_before=3, remainder_after=4)
        for change in [dict(phase='after_publication'), dict(sample=0),
                       dict(remainder_after=2), dict(remainder_after=5), dict(baseline=True)]:
            with self.subTest(change=change), self.assertRaises(ValueError):
                recorder.next_bios_input(sample | dict(bios_measurement=measurement | change))
        with self.assertRaises(ValueError):
            recorder.replay_clock_inputs([sample])

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
