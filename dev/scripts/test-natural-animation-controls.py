#!/usr/bin/env python3
"""Failure controls for the installed source-ownership acceptance oracle."""
import copy
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('natural', Path(__file__).with_name('test-natural-animation.py'))
natural = importlib.util.module_from_spec(spec)
spec.loader.exec_module(natural)


def rows():
    result = []
    for token, effect in enumerate((22, 21, 40), 1):
        slots = [None] * 16
        slots[0] = dict(token=token, effect=effect, owner={'Script': {'generation': 1, 'index': int(token == 3)}})
        result.append(dict(kind='ownership', phase=1 if token == 1 else 3, frame=11 if token == 3 else 6, ownership=dict(samples=dict(slots=slots), animation=[] if token == 3 else [[1, 0]])))
    result += [dict(kind='command', sound=f'Play {{ effect: {effect}, rate_offset_hz: 0 }}', frame=6) for effect in (22, 21)]
    result += [dict(kind='end', frame=12), dict(kind='end', frame=12), dict(kind='replacement_survived'), dict(kind='complete')]
    return result


class Controls(unittest.TestCase):
    def test_continuation(self):
        natural.validate(rows())

    def test_incomplete_runtime_rejects(self):
        with self.assertRaises(AssertionError):
            natural.validate(rows()[:-1])

    def test_duplicate_admission_rejects(self):
        wrong = rows()
        duplicate = copy.deepcopy(wrong[0])
        duplicate['ownership']['samples']['slots'][0]['token'] = 4
        wrong.insert(3, duplicate)
        with self.assertRaises(AssertionError):
            natural.validate(wrong)

    def test_stale_remembered_owner_rejects(self):
        wrong = rows()
        wrong[2]['ownership']['animation'] = [[1, 0]]
        with self.assertRaises(AssertionError):
            natural.validate(wrong)

    def test_late_replacement_cannot_prove_end_continuation(self):
        wrong = rows()
        wrong[2]['frame'] = 12
        with self.assertRaises(AssertionError):
            natural.validate(wrong)

    def test_premature_draw_completion_rejects(self):
        wrong = rows()
        idle = copy.deepcopy(wrong[0])
        idle['frame'] = 11
        idle['ownership']['samples']['slots'][0] = None
        wrong.insert(1, idle)
        with self.assertRaises(AssertionError):
            natural.validate(wrong)

    def test_stale_generation_rejects(self):
        wrong = rows()
        wrong[2]['ownership']['samples']['slots'][0]['owner']['Script']['generation'] = 2
        with self.assertRaises(AssertionError):
            natural.validate(wrong)

    def test_wrong_source_owner_rejects(self):
        wrong = rows()
        wrong[2]['ownership']['samples']['slots'][0]['owner']['Script']['index'] = 0
        with self.assertRaises(AssertionError):
            natural.validate(wrong)

    def test_wrong_animation_timing_rejects(self):
        wrong = rows()
        wrong[3]['frame'] = 7
        with self.assertRaises(AssertionError):
            natural.validate(wrong)


if __name__ == '__main__':
    unittest.main()
