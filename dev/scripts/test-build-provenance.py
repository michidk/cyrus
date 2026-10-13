#!/usr/bin/env python3
"""Provenance rejects source races, stale artifacts and untracked source edits."""
import json
from pathlib import Path
import subprocess
import tempfile
import unittest


SCRIPT = Path(__file__).with_name("build-provenance.py").resolve()


class BuildProvenance(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        subprocess.run(["git", "-c", "core.hooksPath=/dev/null", "init", "-q", str(self.root)], check=True)
        (self.root / ".git/HEAD").write_text("1" * 40 + "\n")
        (self.root / "crates").mkdir()
        (self.root / "crates/source.rs").write_text("source")
        (self.root / "artifact").write_bytes(b"compiled output")

    def run_command(self, *args, succeeds=True):
        result = subprocess.run(["python3", str(SCRIPT), *args], cwd=self.root,
                                capture_output=True, text=True)
        if succeeds:
            self.assertEqual(result.returncode, 0, result.stderr)
        else:
            self.assertNotEqual(result.returncode, 0)
        return result

    def build(self):
        self.run_command("snapshot", "record.json")
        self.run_command("seal", "record.json", "artifact")

    def test_verifies_unchanged_build_and_ignores_capture_outputs(self):
        self.build()
        (self.root / "magic").mkdir()
        (self.root / "magic/capture.png").write_bytes(b"untracked art")
        self.run_command("verify", "record.json")
        record = json.loads((self.root / "record.json").read_text())
        self.assertEqual(record["artifacts"]["artifact"],
                         "9124deeb89b39206c628003a2fc1d19096ab65695e0d571c2acb1e214484758d")

    def test_source_edit_during_build_is_rejected(self):
        self.run_command("snapshot", "record.json")
        (self.root / "crates/source.rs").write_text("changed")
        result = self.run_command("seal", "record.json", "artifact", succeeds=False)
        self.assertIn("source changed during build", result.stderr)

    def test_untracked_source_addition_invalidates_build(self):
        self.build()
        (self.root / "crates/new.rs").write_text("new module")
        self.run_command("verify", "record.json", succeeds=False)

    def test_replaced_artifact_is_rejected(self):
        self.build()
        (self.root / "artifact").write_bytes(b"different build")
        result = self.run_command("verify", "record.json", succeeds=False)
        self.assertIn("build artifacts changed", result.stderr)

    def test_external_source_symlink_is_rejected(self):
        (self.root / "crates/link").symlink_to(self.root / "artifact")
        result = self.run_command("snapshot", "record.json", succeeds=False)
        self.assertIn("build input symlink", result.stderr)


if __name__ == "__main__":
    unittest.main()
