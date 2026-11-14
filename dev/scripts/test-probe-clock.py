#!/usr/bin/env python3
"""Exact acquisition plans use measured integer boundaries, never assumed FPS."""
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('clock', Path(__file__).with_name('probe-clock.py'))
clock = importlib.util.module_from_spec(spec)
spec.loader.exec_module(clock)


def rows(scales, origin=0):
    result = [dict(update=10, generation=1, consumed_scale=origin)]
    for i, scale in enumerate(scales):
        origin += scale
        result.append(dict(update=11+i, generation=1, consumed_scale=origin))
    return result


class ObservedClock(unittest.TestCase):
    def test_changing_steps_do_not_turn_twelve_updates_into_two_seconds(self):
        native = rows([512,438,384,384,384,384,384,384,405,426,437,438])
        with self.assertRaisesRegex(ValueError, 'steady'):
            clock.plan(rows([128]*8), native, normal_frame_rate=12, minimum_scale=6144)

    def test_plan_uses_common_observed_steps_and_integer_endpoints(self):
        plan = clock.plan(rows([128]*8), rows([512]*8), normal_frame_rate=12, minimum_scale=6144)
        self.assertEqual(plan, dict(target_scale=6144, normal_frame_rate=12,
                                   original_updates=48, native_updates=12, original_step=128, native_step=512))
        clock.validate(rows([128]*48), plan, 'original')
        clock.validate(rows([512]*12), plan, 'native')

    def test_125ms_and_sixth_second_plan_share_half_second_without_rounding(self):
        plan = clock.plan(rows([384]*8), rows([512]*8), normal_frame_rate=12, minimum_scale=384)
        self.assertEqual(plan['target_scale'], 1536)
        self.assertEqual((plan['original_updates'], plan['native_updates']), (4,3))

    def test_skipped_target_and_actual_changing_scale_still_fail(self):
        plan = clock.plan(rows([128]*8), rows([512]*8), normal_frame_rate=12, minimum_scale=6144)
        for actual in (rows([512,438]+[384]*10), rows([128]*47+[256])):
            with self.assertRaises(ValueError):
                clock.validate(actual, plan, 'native' if len(actual)==13 else 'original')

    def test_duplicate_gap_restart_or_unknown_clock_is_not_a_measurement(self):
        for change in ('duplicate','gap','restart','float','backwards'):
            sample=rows([128]*8)
            if change=='duplicate':sample[-1]['update']-=1
            if change=='gap':sample[-1]['update']+=1
            if change=='restart':sample[-1]['generation']=2
            if change=='float':sample[-1]['consumed_scale']=1024.0
            if change=='backwards':sample[-1]['consumed_scale']=0
            with self.subTest(change=change),self.assertRaises(ValueError):clock.steady_step(sample)

    def test_pair_actual_observed_endpoints_without_assuming_future_scales(self):
        original = rows([256,256,256,256,256,256,256,256,256,256,192,180,170,161])
        native = rows([512,438,384,384,384])
        self.assertEqual(clock.pair_observed(original,native,original_rate=12,native_rate=12),
                         [dict(consumed_scale=512,original_index=2,native_index=1)])
        with self.assertRaisesRegex(ValueError, 'not observed'):
            clock.pair_observed(original,native,original_rate=12,native_rate=12,required_scale=6144)
        with self.assertRaisesRegex(ValueError, 'not observed'):
            clock.pair_observed(rows([6064,128]),rows([6144]),original_rate=12,native_rate=12,required_scale=6144)

    def test_pairing_rejects_nonzero_input_origins_and_inconsistent_boundaries(self):
        with self.assertRaisesRegex(ValueError, 'origin'):
            clock.pair_observed(rows([128]*8,origin=1),rows([512]*2),original_rate=12,native_rate=12)
        sample=rows([128]*8);sample[-1]['update']+=1
        with self.assertRaises(ValueError):clock.pair_observed(sample,rows([512]*2),original_rate=12,native_rate=12)

    def test_pairing_rejects_different_clock_units_or_fractional_target(self):
        for a,b,target in [(12,24,None),(0,0,None),(12,12,512.0)]:
            with self.assertRaises(ValueError):
                clock.pair_observed(rows([128]*8),rows([512]*2),original_rate=a,native_rate=b,required_scale=target)

    def test_admission_uses_acknowledged_boundaries_after_cadence_changes(self):
        counterpart = rows([512, 512, 310])
        admission = clock.Admission(counterpart, minimum_scale=1024, max_updates=8)
        actual = rows([512, 438, 384])
        self.assertFalse(admission.observe(actual[0]))
        self.assertFalse(admission.observe(actual[1]))
        self.assertFalse(admission.observe(actual[2]))
        self.assertTrue(admission.observe(actual[3]))
        self.assertEqual(admission.endpoint, 1334)
        self.assertEqual(admission.counterpart_index, 3)

    def test_admission_never_rounds_a_skipped_endpoint_or_accepts_a_gap(self):
        for actual in (rows([512, 438, 450]), rows([512, 438, 384])):
            admission = clock.Admission(rows([512, 512, 310]), minimum_scale=1024, max_updates=8)
            if actual[-1]['consumed_scale'] == 1334:
                actual[-1]['update'] += 1
            with self.assertRaises(ValueError):
                for boundary in actual:
                    admission.observe(boundary)

    def test_rejected_acquisition_cannot_resume_by_replacing_a_missing_boundary(self):
        admission = clock.Admission(rows([512,512,310]), minimum_scale=1024, max_updates=8)
        actual = rows([512,438,384])
        for row in actual[:-1]:
            self.assertFalse(admission.observe(row))
        gap = dict(actual[-1], update=actual[-1]['update']+1)
        with self.assertRaises(ValueError):
            admission.observe(gap)
        with self.assertRaisesRegex(ValueError, 'rejected'):
            admission.observe(actual[-1])

    def test_plan_is_bounded_and_has_no_installed_rate_guess(self):
        for rate, minimum in [(0,6144),(12,0),(12,1_000_000)]:
            with self.assertRaises(ValueError):
                clock.plan(rows([128]*8), rows([512]*8), normal_frame_rate=rate, minimum_scale=minimum)


if __name__ == '__main__':unittest.main()
