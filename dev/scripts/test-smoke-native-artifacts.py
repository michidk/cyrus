#!/usr/bin/env python3
"""Exercise release smoke checks with executable synthetic archive payloads."""
from pathlib import Path
import os
import subprocess
import tarfile
import tempfile
import unittest
import zipfile

SCRIPT = Path(__file__).with_name("smoke-native-artifacts.py")


class SmokeTest(unittest.TestCase):
    def test_ci_runs_the_packaged_smoke_regressions(self):
        workflow = (SCRIPT.parents[2] / ".github/workflows/ci.yml").read_text()
        artifact_step = workflow.split("      - name: Check artifact packaging\n", 1)[1]
        artifact_step = artifact_step.split("\n      - ", 1)[0]
        self.assertIn("python3 dev/scripts/test-build-artifacts.py", artifact_step)
        self.assertIn("python3 dev/scripts/test-smoke-native-artifacts.py", artifact_step)

    def test_both_workflows_smoke_shipped_payloads(self):
        for name in ("pr-artifacts.yml", "release.yml"):
            workflow = (SCRIPT.parents[2] / ".github/workflows" / name).read_text()
            self.assertIn('run: python3 dev/scripts/smoke-native-artifacts.py target/artifacts "$BUILD_TARGET"', workflow)
            self.assertNotIn('binary="target/$BUILD_TARGET/release/', workflow)

    def test_packaged_executables_and_broken_bundle(self):
        for target in ("x86_64-unknown-linux-gnu", "x86_64-pc-windows-msvc"):
            with self.subTest(target=target), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                windows = "windows" in target
                suffix = ".exe" if windows else ""
                payload = root / "payload"
                payload.mkdir()
                binary = payload / f"cyrus{suffix}"
                binary.write_text('#!/bin/sh\n[ "$1" = --help ] || exit 7\necho "$0" >> "$SMOKE_LOG"\n')
                binary.chmod(0o755)
                artifacts = root / "artifacts"
                artifacts.mkdir()
                if windows:
                    with zipfile.ZipFile(artifacts / f"cyrus-{target}.zip", "w", zipfile.ZIP_DEFLATED) as archive:
                        archive.write(binary, binary.name)
                else:
                    with tarfile.open(artifacts / f"cyrus-{target}.tar.gz", "w:gz") as archive:
                        archive.add(binary, arcname="cyrus")
                bundle = artifacts / f"cyrus-with-mods-{target}{'.zip' if windows else '.tar.gz'}"

                def pack():
                    if windows:
                        with zipfile.ZipFile(bundle, "w") as archive:
                            archive.write(binary, binary.name)
                    else:
                        with tarfile.open(bundle, "w:gz") as archive:
                            archive.add(binary, arcname=binary.name)

                pack()
                env = dict(os.environ, SMOKE_LOG=str(root / "calls"))
                command = ["python3", str(SCRIPT), str(artifacts), target]
                good = subprocess.run(command, env=env, capture_output=True, text=True)
                self.assertEqual(good.returncode, 0, good.stderr)
                calls = (root / "calls").read_text().splitlines()
                self.assertEqual(len(calls), 2)
                self.assertTrue(any("bundle" in call for call in calls))
                binary.write_text("#!/bin/sh\nexit 9\n")
                pack()
                bad = subprocess.run(command, env=env, capture_output=True, text=True)
                self.assertNotEqual(bad.returncode, 0)
                self.assertIn("cyrus-with-mods", bad.stderr)
                bundle.unlink()
                missing = subprocess.run(command, env=env, capture_output=True, text=True)
                self.assertNotEqual(missing.returncode, 0)
                self.assertIn("cyrus-with-mods", missing.stderr)


if __name__ == "__main__":
    unittest.main()
