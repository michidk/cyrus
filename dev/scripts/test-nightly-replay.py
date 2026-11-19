#!/usr/bin/env python3
"""Exercise scheduled replay and failure publication without an install or network."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


class Nightly(unittest.TestCase):
    def run_nightly(self, status, pull_status=0, hauler=True, interrupted=False,
                    replace_script=False, old_runs=0):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            commands = root / "bin"
            commands.mkdir()
            script_path = root / "repo/dev/scripts/nightly-replay.sh"
            script_path.parent.mkdir(parents=True)
            script_path.write_text(Path(__file__).with_name("nightly-replay.sh").read_text())
            for name in ["toolchain.sh", "cargo.sh"]:
                script_path.with_name(name).write_text("")
            for index in range(old_runs):
                old = root / f"config/nightly/run-2000010{index}T000000Z"
                old.mkdir(parents=True)
                (old / "replay.log").write_text("old log")
            if interrupted:
                pending = root / "config/nightly/unfinished.issue.md"
                pending.parent.mkdir(parents=True)
                pending.write_text("Previous run interrupted; not counted as passing.\n")
            for name, body in {
                "git": f'if [ "$1" = rev-parse ]; then echo test-commit; else exit {pull_status}; fi',
                "cargo": 'echo "cargo $*" >> "$CALLS"',
                "just": ('echo "$*" >> "$CALLS"; if [ "$1" = replay-chain ]; then '
                         + 'test "$CYRUS_REPLAY_BINARY" = "$CARGO_TARGET_DIR/debug/cyrus" || exit 93; '
                         + 'mkdir -p "$CYRUS_CONFIG_DIR/cache"; echo process > "$CYRUS_CONFIG_DIR/process.log"; '
                         + 'echo generated > "$CYRUS_CONFIG_DIR/cache/generated"; '
                         + ('awk \'BEGIN {for(i=0;i<10000;i++) print ""; print "exit 99"}\' > "$SOURCE_SCRIPT"; '
                            if replace_script else '') + f'exit {status}; fi'),
                "gh": 'echo "$*" >> "$CALLS"; cat "$7" > "$REPORT"',
                **({"cargo-hauler": "exit 0"} if hauler else {}),
            }.items():
                script = commands / name
                script.write_text("#!/bin/sh\n" + body + "\n")
                script.chmod(0o755)
            env = dict(os.environ, PATH=f"{commands}:/usr/bin:/bin",
                       CYRUS_CONFIG_DIR=str(root / "config"), REDGUARD_DIR=str(root / "install"),
                       CALLS=str(root / "calls"), REPORT=str(root / "report"),
                       SOURCE_SCRIPT=str(script_path), CARGO_TARGET_DIR=str(root / "stub-shared-target"))
            result = subprocess.run(["bash", str(script_path)],
                                    env=env, capture_output=True, text=True)
            calls = (root / "calls").read_text() if (root / "calls").exists() else ""
            report = (root / "report").read_text() if (root / "report").exists() else ""
            self.assertFalse((root / "config/runtime").exists(), "runtime caches must be discarded")
            retained = sorted((root / "config/nightly").glob("run-*"))
            self.assertLessEqual(len(retained), 2)
            self.assertTrue(all(path.suffix == ".log" for run in retained for path in run.iterdir()))
            return result.returncode, calls, report

    def test_success_runs_chain_without_posting(self):
        status, calls, report = self.run_nightly(0)
        self.assertEqual(status, 0)
        self.assertEqual(calls, "cargo build -p cyrus --bin cyrus\nreplay-chain\n")
        self.assertEqual(report, "")

    def test_failure_posts_once_and_preserves_exit_status(self):
        status, calls, report = self.run_nightly(7)
        self.assertEqual(status, 7)
        self.assertEqual(calls.count("issue comment 322"), 1)
        self.assertIn("Commit: test-commit. Exit status: 7.", report)
        self.assertLessEqual(len(report.splitlines()), 15)

    def test_pull_failure_does_not_run_old_chain(self):
        status, calls, report = self.run_nightly(0, pull_status=8)
        self.assertEqual(status, 8)
        self.assertNotIn("replay-chain", calls)
        self.assertIn("Exit status: 8.", report)

    def test_missing_hauler_restores_setup(self):
        status, calls, _ = self.run_nightly(0, hauler=False)
        self.assertEqual(status, 0)
        self.assertTrue(calls.startswith("setup\ncargo build -p cyrus --bin cyrus\nreplay-chain\n"))

    def test_next_run_reports_interruption_before_replaying(self):
        status, calls, report = self.run_nightly(0, interrupted=True)
        self.assertEqual(status, 0)
        self.assertEqual(calls.count("issue comment 322"), 1)
        self.assertTrue(calls.endswith("replay-chain\n"))
        self.assertIn("Previous run interrupted", report)

    def test_source_replacement_during_run_does_not_change_execution(self):
        status, calls, report = self.run_nightly(0, replace_script=True)
        self.assertEqual(status, 0)
        self.assertEqual(calls, "cargo build -p cyrus --bin cyrus\nreplay-chain\n")
        self.assertEqual(report, "")

    def test_retains_only_last_two_runs_and_discards_generated_state(self):
        status, _, _ = self.run_nightly(0, old_runs=4)
        self.assertEqual(status, 0)


if __name__ == "__main__":
    unittest.main()
