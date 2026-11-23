#!/usr/bin/env python3
"""Exercise scheduled replay and failure publication without an install or network."""
import os
import importlib.util
import io
import json
import tarfile
from unittest import mock
from pathlib import Path
import subprocess
import tempfile
import unittest


class Nightly(unittest.TestCase):
    def run_nightly(self, status, pull_status=0, hauler=True, interrupted=False,
                    replace_script=False, old_runs=0, dirty=False, artifact_status=0):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            commands = root / "bin"
            commands.mkdir()
            script_path = root / "repo/dev/scripts/nightly-replay.sh"
            script_path.parent.mkdir(parents=True)
            script_path.write_text(Path(__file__).with_name("nightly-replay.sh").read_text())
            for name in ["toolchain.sh", "cargo.sh"]:
                script_path.with_name(name).write_text("")
            script_path.with_name("download-main-replay.py").write_text(
                'import os, pathlib, sys\n'
                'with open(os.environ["CALLS"], "a") as calls: calls.write("artifact " + sys.argv[1] + "\\n")\n'
                f'sys.exit({artifact_status}) if {artifact_status} else None\n'
                'assert "--fallback-main" in sys.argv\n'
                'receipt = pathlib.Path(sys.argv[sys.argv.index("--receipt") + 1]); receipt.write_text("uploaded-main artifact run 7\\n")\n'
                'binary = pathlib.Path(sys.argv[2]); binary.parent.mkdir(parents=True, exist_ok=True)\n'
                'binary.write_text("executable")\n')
            for index in range(old_runs):
                old = root / f"config/nightly/run-2000010{index}T000000Z"
                old.mkdir(parents=True)
                (old / "replay.log").write_text("old log")
            if interrupted:
                pending = root / "config/nightly/unfinished.issue.md"
                pending.parent.mkdir(parents=True)
                pending.write_text("Previous run interrupted; not counted as passing.\n")
            for name, body in {
                "git": (f'if [ "$1" = rev-parse ]; then echo test-commit; '
                        + f'elif [ "$1" = status ]; then {"echo modified-file" if dirty else ":"}; '
                        + f'else echo pull >> "$GIT_CALLS"; exit {pull_status}; fi'),
                "cargo": 'echo "cargo $*" >> "$CALLS"',
                "just": ('echo "$*" >> "$CALLS"; if [ "$1" = replay-chain ]; then '
                         + 'test "$CYRUS_REPLAY_BINARY" = "$CYRUS_CONFIG_DIR/cyrus" || exit 93; '
                         + 'mkdir -p "$CYRUS_CONFIG_DIR/cache"; echo process > "$CYRUS_CONFIG_DIR/process.log"; '
                         + 'echo generated > "$CYRUS_CONFIG_DIR/cache/generated"; '
                         + 'mkdir -p "$CYRUS_CONFIG_DIR/segments/example"; '
                         + 'echo segment > "$CYRUS_CONFIG_DIR/segments/example/process.log"; '
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
                       GIT_CALLS=str(root / "git-calls"), CALLS=str(root / "calls"), REPORT=str(root / "report"),
                       SOURCE_SCRIPT=str(script_path), CARGO_TARGET_DIR=str(root / "stub-shared-target"))
            result = subprocess.run(["bash", str(script_path)],
                                    env=env, capture_output=True, text=True)
            if dirty:
                self.assertFalse((root / "git-calls").exists(), "dirty checkout must not be pulled")
                self.assertIn("checkout is dirty", result.stdout + result.stderr)
            calls = (root / "calls").read_text() if (root / "calls").exists() else ""
            report = (root / "report").read_text() if (root / "report").exists() else ""
            self.assertFalse((root / "config/runtime").exists(), "runtime caches must be discarded")
            retained = sorted((root / "config/nightly").glob("run-*"))
            self.assertLessEqual(len(retained), 2)
            self.assertTrue(all(path.suffix == ".log" for run in retained for path in run.iterdir()))
            if "replay-chain\n" in calls:
                self.assertTrue((retained[-1] / "segments-example-process.log").exists())
            return result.returncode, calls, report

    def test_success_runs_chain_and_posts_result(self):
        status, calls, report = self.run_nightly(0)
        self.assertEqual(status, 0)
        self.assertTrue(calls.startswith("artifact test-commit\nreplay-chain\n"))
        self.assertIn("passed end to end", report)

    def test_failure_posts_once_and_preserves_exit_status(self):
        status, calls, report = self.run_nightly(7)
        self.assertEqual(status, 7)
        self.assertEqual(calls.count("issue comment 322"), 1)
        self.assertIn("Commit: test-commit. Exit status: 7.", report)
        self.assertLessEqual(len(report.splitlines()), 15)

    def test_missing_main_artifact_is_reported_without_falling_back_to_a_build(self):
        status, calls, report = self.run_nightly(0, artifact_status=9)
        self.assertEqual(status, 9)
        self.assertNotIn("replay-chain", calls)
        self.assertNotIn("cargo", calls)
        self.assertIn("Exit status: 9.", report)

    def test_pull_failure_does_not_run_old_chain(self):
        status, calls, report = self.run_nightly(0, pull_status=8)
        self.assertEqual(status, 8)
        self.assertNotIn("replay-chain", calls)
        self.assertIn("Exit status: 8.", report)

    def test_dirty_checkout_is_reported_without_pull_or_build(self):
        status, calls, report = self.run_nightly(0, dirty=True)
        self.assertEqual(status, 65)
        self.assertNotIn("cargo build", calls)
        self.assertNotIn("replay-chain", calls)
        self.assertIn("Exit status: 65.", report)

    def test_artifact_replay_needs_no_hauler_or_local_build(self):
        status, calls, _ = self.run_nightly(0, hauler=False)
        self.assertEqual(status, 0)
        self.assertTrue(calls.startswith("artifact test-commit\nreplay-chain\n"))

    def test_next_run_reports_interruption_before_replaying(self):
        status, calls, report = self.run_nightly(0, interrupted=True)
        self.assertEqual(status, 0)
        self.assertEqual(calls.count("issue comment 322"), 2)
        self.assertIn("replay-chain\n", calls)
        self.assertIn("passed end to end", report)

    def test_source_replacement_during_run_does_not_change_execution(self):
        status, calls, report = self.run_nightly(0, replace_script=True)
        self.assertEqual(status, 0)
        self.assertTrue(calls.startswith("artifact test-commit\nreplay-chain\n"))
        self.assertIn("passed end to end", report)

    def test_retains_only_last_two_runs_and_discards_generated_state(self):
        status, _, _ = self.run_nightly(0, old_runs=4)
        self.assertEqual(status, 0)


class MainArtifact(unittest.TestCase):
    def test_fallback_skips_pending_expired_nonancestor_and_unfetched_runs(self):
        spec = importlib.util.spec_from_file_location("main_artifact", Path(__file__).with_name("download-main-replay.py"))
        artifact = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(artifact)
        runs = [{"headSha": sha, "databaseId": number}
                for sha, number in [("unfetched", 11), ("later", 10), ("wanted", 9), ("expired", 8), ("uploaded", 7)]]
        queried = []
        def query(command, text):
            if command[1:3] == ["run", "list"]:
                return json.dumps(runs)
            queried.append(command[2])
            return json.dumps([{"id": 1}] if '/7/' in command[2] else [])
        def execute(command, **kwargs):
            if command[0] == 'git':
                return subprocess.CompletedProcess(command, 128 if command[-2] == 'unfetched' else int(command[-2] == 'later'))
            self.assertEqual(command[1:4], ['run', 'download', '7'])
            package = Path(command[-1]) / 'cyrus-x86_64-unknown-linux-gnu.tar.gz'
            with tarfile.open(package, 'w:gz') as archive:
                info = tarfile.TarInfo('cyrus'); info.size = 4
                archive.addfile(info, io.BytesIO(b'test'))
            return subprocess.CompletedProcess(command, 0)
        with tempfile.TemporaryDirectory() as temporary, \
                mock.patch.object(artifact.subprocess, 'check_output', side_effect=query), \
                mock.patch.object(artifact.subprocess, 'run', side_effect=execute):
            destination = Path(temporary) / 'cyrus'
            receipt = artifact.download('wanted', destination, fallback_main=True)
            self.assertEqual(receipt, {'commit': 'uploaded', 'run': 7})
            self.assertEqual(destination.read_bytes(), b'test')
            self.assertEqual(len(queried), 3)
            self.assertNotIn('/10/', ''.join(queried))
            self.assertNotIn('/11/', ''.join(queried))

    def test_download_selects_exact_main_commit_and_extracts_only_the_binary(self):
        spec = importlib.util.spec_from_file_location("main_artifact", Path(__file__).with_name("download-main-replay.py"))
        artifact = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(artifact)
        runs = [{"headSha": "newer", "databaseId": 9}, {"headSha": "wanted", "databaseId": 7}]
        with tempfile.TemporaryDirectory() as temporary:
            destination = Path(temporary) / "runtime/cyrus"
            def download(command, check):
                self.assertIn("7", command)
                self.assertIn(artifact.ARTIFACT, command)
                package = Path(command[-1]) / "cyrus-x86_64-unknown-linux-gnu.tar.gz"
                with tarfile.open(package, "w:gz") as archive:
                    for name in ["./cyrus", "../unwanted"]:
                        info = tarfile.TarInfo(name)
                        info.size = 4
                        archive.addfile(info, io.BytesIO(b"test"))
            with mock.patch.object(artifact.subprocess, "check_output", return_value=json.dumps(runs)), \
                    mock.patch.object(artifact.subprocess, "run", side_effect=download):
                artifact.download("wanted", destination)
                self.assertEqual(destination.read_bytes(), b"test")
                self.assertTrue(os.access(destination, os.X_OK))
                self.assertEqual(list(destination.parent.iterdir()), [destination])
                with self.assertRaisesRegex(ValueError, "no artifact workflow run"):
                    artifact.download("missing", destination)
                self.assertEqual(destination.read_bytes(), b"test")


if __name__ == "__main__":
    unittest.main()
