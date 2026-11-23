#!/usr/bin/env python3
"""Controls for the original actor-record diff, independent of game bytes."""
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location("rd", Path(__file__).with_name("original-record-diff.py"))
rd = importlib.util.module_from_spec(spec)
spec.loader.exec_module(rd)


class RecordDiff(unittest.TestCase):
    def test_changed_bytes_report_offset_old_and_new(self):
        self.assertEqual(rd.changed_bytes("000102", "000902"), [(1, 1, 9)])
        self.assertEqual(rd.changed_bytes("0001", "0001"), [])

    def test_a_length_difference_is_reported_not_hidden(self):
        self.assertEqual(rd.changed_bytes("0001", "000102"), [(2, None, 2)])
        self.assertEqual(rd.changed_bytes("000102", "0001"), [(2, 2, None)])

    def test_ignore_ranges_are_inclusive_and_accept_hex_or_decimal(self):
        self.assertEqual(rd.parse_ranges("0x2-0x3, 7".replace(" ", "")), [(2, 3), (7, 7)])
        self.assertEqual(rd.parse_ranges(""), [])
        rows = rd.changed_bytes("00000000", "01010101", rd.parse_ranges("1-2"))
        self.assertEqual([offset for offset, _, _ in rows], [0, 3])

    def test_dump_diff_masks_only_the_record_and_reports_a_missing_dump(self):
        before = {"rec": "0000", "attrs": "0000"}
        after = {"rec": "0101", "attrs": "0101"}
        masked = rd.diff_dumps(before, after, [(0, 0)])
        self.assertEqual([o for o, _, _ in masked["rec"]], [1])
        self.assertEqual([o for o, _, _ in masked["attrs"]], [0, 1])  # attributes are never masked
        self.assertIn("error", rd.diff_dumps(None, after))


if __name__ == "__main__":
    unittest.main()
