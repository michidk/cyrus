#!/usr/bin/env python3
"""Synthetic negative controls for the original/mixer report comparison."""
import copy
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location("comparison", Path(__file__).with_name("compare-surface-audio.py"))
comparison = importlib.util.module_from_spec(spec)
spec.loader.exec_module(comparison)


def report():
    return {"version": 1, "world": 1, "cases": [{"name": "synthetic", "steps": [{
        "position_q8": [0, 0, 0], "prior_surface": 0, "surface": 7, "veto": 0,
        "support": "Terrain", "events": [{"kind": "play", "authored": 9,
        "rate_offset_hz": 0, "captured_surface": 7, "started": [{"target_candidates": [10], "speed": 1.0}],
        "remaining": 1}]}]}]}


class Comparison(unittest.TestCase):
    def setUp(self):
        self.actual = report()
        self.original = copy.deepcopy(self.actual)
        self.original["evidence"] = {"kind": "original-runtime", "record": "synthetic test only"}

    def test_matching_observations(self):
        comparison.compare(self.actual, self.original)

    def test_wrong_surface(self):
        self.actual["cases"][0]["steps"][0]["surface"] = 6
        with self.assertRaisesRegex(ValueError, r"steps\[0\].surface"):
            comparison.compare(self.actual, self.original)

    def test_wrong_mixer_target(self):
        self.actual["cases"][0]["steps"][0]["events"][0]["started"][0]["target_candidates"] = [9]
        with self.assertRaisesRegex(ValueError, r"target_candidates\[0\]"):
            comparison.compare(self.actual, self.original)

    def test_missing_event(self):
        self.actual["cases"][0]["steps"][0]["events"] = []
        with self.assertRaisesRegex(ValueError, "no animation Play"):
            comparison.compare(self.actual, self.original)

    def test_ambiguous_pcm_is_not_a_match(self):
        self.actual["cases"][0]["steps"][0]["events"][0]["started"][0]["target_candidates"] = [9, 10]
        with self.assertRaisesRegex(ValueError, "ambiguous PCM"):
            comparison.compare(self.actual, self.original)

    def test_file_table_is_not_runtime_evidence(self):
        self.original["evidence"]["kind"] = "file-table"
        with self.assertRaisesRegex(ValueError, "independent runtime"):
            comparison.compare(self.actual, self.original)


if __name__ == "__main__":
    unittest.main()
