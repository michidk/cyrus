#!/usr/bin/env python3
"""Acceptance oracle controls; these do not substitute for the installed replay."""
import importlib.util
import unittest
from pathlib import Path

spec = importlib.util.spec_from_file_location('dial', Path(__file__).with_name('dial-acceptance.py'))
dial = importlib.util.module_from_spec(spec)
spec.loader.exec_module(dial)


def sample(frame, count=1, tasks=1, lockout=0, failures=()):
    return dict(frame=frame, count=count, tasks=tasks, lockout=lockout, failures=failures)


class Oracle(unittest.TestCase):
    def test_consumption_completion_and_continuation(self):
        run = dial.Acceptance(True)
        self.assertFalse(run.observe(sample(1)))
        self.assertFalse(run.observe(sample(2, count=0, tasks=0, lockout=1)))
        self.assertFalse(run.observe(sample(3, count=0, tasks=0)))
        self.assertTrue(run.observe(sample(15, count=0, tasks=0)))
        with self.assertRaisesRegex(AssertionError, 'returned'):
            run.observe(sample(16, count=1, tasks=0))

    def test_first_active_sample_can_already_show_consumption(self):
        run = dial.Acceptance(True)
        self.assertFalse(run.observe(sample(1, count=0)))
        self.assertFalse(run.observe(sample(2, count=0, tasks=0)))
        self.assertTrue(run.observe(sample(14, count=0, tasks=0)))

    def test_idle_world_and_held_lockout_cannot_pass(self):
        run = dial.Acceptance(False)
        self.assertFalse(run.observe(sample(1, count=0, tasks=0)))
        self.assertFalse(run.observe(sample(20, count=0, tasks=0)))
        self.assertFalse(run.observe(sample(21, count=0)))
        self.assertFalse(run.observe(sample(40, count=0, tasks=0, lockout=1)))
        self.assertFalse(run.observe(sample(80, count=0, tasks=0, lockout=1)))
        self.assertFalse(run.observe(sample(81, count=0, tasks=0)))
        self.assertTrue(run.observe(sample(93, count=0, tasks=0)))

    def test_missing_consumption_never_passes(self):
        run = dial.Acceptance(True)
        run.observe(sample(1))
        self.assertFalse(run.observe(sample(100, tasks=0)))
        self.assertFalse(run.observe(sample(120, tasks=0)))

    def test_failure_remains_fatal_after_continuation(self):
        run = dial.Acceptance(True)
        with self.assertRaisesRegex(AssertionError, 'broken native'):
            run.observe(sample(1, failures=['broken native']))
        with self.assertRaisesRegex(AssertionError, 'broken native'):
            run.observe(sample(20, count=0, tasks=0))

    def test_empty_control_rejects_inventory_change(self):
        run = dial.Acceptance(False)
        run.observe(sample(1, count=0))
        with self.assertRaisesRegex(AssertionError, 'acquired'):
            run.observe(sample(2, count=1))


if __name__ == '__main__':
    unittest.main()
