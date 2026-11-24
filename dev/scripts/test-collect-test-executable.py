#!/usr/bin/env python3
"""Exercise Cargo-reported test discovery independently of its output layout."""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location(
    "collector", Path(__file__).with_name("collect-test-executable.py")
)
collector = importlib.util.module_from_spec(spec)
spec.loader.exec_module(collector)


def artifact(path, target="original_runtime", test=True):
    return json.dumps({
        "reason": "compiler-artifact", "target": {"name": target},
        "profile": {"test": test}, "executable": str(path),
    })


class CollectorTest(unittest.TestCase):
    def test_reported_layouts_and_mixed_test_output(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for layout in ("debug/deps/original_runtime-hash", "debug/build/pkg/hash/out/original_runtime-hash"):
                with self.subTest(layout=layout):
                    executable = root / layout
                    executable.parent.mkdir(parents=True, exist_ok=True)
                    executable.write_bytes(b"retained test executable")
                    executable.chmod(0o755)
                    destination = root / "artifact/original-runtime-test"
                    messages = ["running 1 test", "null", "42", "[]", artifact(executable, test=False),
                                artifact(executable, target="other"), artifact(executable),
                                artifact(executable), "test result: ok"]
                    collector.collect(messages, "original_runtime", destination)
                    self.assertEqual(destination.read_bytes(), executable.read_bytes())
                    self.assertTrue(destination.stat().st_mode & 0o111)

    def test_missing_ambiguous_and_nonexecutable_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            first, second = root / "first", root / "second"
            first.write_text("not executable")
            for messages in ([], [artifact(first), artifact(second)], [artifact(first)], [artifact(second)]):
                with self.subTest(messages=messages), self.assertRaises(ValueError):
                    collector.collect(messages, "original_runtime", root / "artifact")
            self.assertFalse((root / "artifact").exists())


if __name__ == "__main__":
    unittest.main()
