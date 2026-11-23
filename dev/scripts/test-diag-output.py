#!/usr/bin/env python3
"""Controls for durable diagnostic output, independent of the original game."""
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))
import diag_output as out


class DurableLog(unittest.TestCase):
    def test_rows_reach_disk_before_the_run_is_sealed(self):
        with tempfile.TemporaryDirectory() as root:
            log = out.DurableLog(Path(root) / "run.jsonl")
            log.append({"ev": "state", "frame": 1})
            # No seal yet (a killed run): the row is already on disk, the verifier says unsealed.
            self.assertEqual(json.loads((Path(root) / "run.jsonl").read_text()), {"ev": "state", "frame": 1})
            self.assertEqual(out.verify(Path(root) / "run.jsonl"), (False, "no manifest: the run did not finish or was cut off"))

    def test_a_sealed_log_verifies_and_a_mirror_is_identical(self):
        with tempfile.TemporaryDirectory() as root:
            primary, mirror = Path(root) / "a" / "run.jsonl", Path(root) / "b" / "run.jsonl"
            log = out.DurableLog(primary, mirror)
            for frame in range(3):
                log.append({"frame": frame})
            manifest = log.seal("done")
            self.assertEqual((manifest["rows"], manifest["note"]), (3, "done"))
            self.assertEqual(out.verify(primary), (True, "ok"))
            self.assertEqual(out.verify(mirror), (True, "ok"))
            self.assertEqual(primary.read_bytes(), mirror.read_bytes())

    def test_a_missing_truncated_or_edited_log_is_not_trusted(self):
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / "run.jsonl"
            self.assertEqual(out.verify(path), (False, "log missing"))
            log = out.DurableLog(path)
            log.append({"frame": 1})
            log.append({"frame": 2})
            log.seal()
            path.write_bytes(path.read_bytes()[:-3])
            ok, reason = out.verify(path)
            self.assertFalse(ok)
            self.assertIn("size", reason)
            path.write_bytes(b'{"frame": 9}\n{"frame": 8}\n')
            self.assertFalse(out.verify(path)[0])

    def test_write_atomic_leaves_no_partial_file_and_replaces_in_place(self):
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / "sub" / "rec.json"
            out.write_atomic(path, b"one")
            out.write_atomic(path, b"two")
            self.assertEqual(path.read_bytes(), b"two")
            self.assertEqual([p.name for p in path.parent.iterdir()], ["rec.json"])


if __name__ == "__main__":
    unittest.main()
