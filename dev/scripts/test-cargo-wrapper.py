#!/usr/bin/env python3
"""Exercise shell orchestration without compiling or requiring an install."""
import os
import pty
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

SCRIPTS = Path(__file__).resolve().parent


class CargoScripts(unittest.TestCase):
    def test_wrapper_waits_and_preserves_output_and_failure(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            hauler = root / "hauler"
            hauler.write_text('''#!/bin/bash
[[ "$*" == 'exec --host shim -- cargo test example' ]] || exit 99
[[ "$CARGO_HAULER_SHIM_BACKGROUND" == 0 ]] || exit 99
[[ ! -t 1 ]] || exit 99
echo 'test output'
exit 17
''')
            hauler.chmod(0o755)
            env = dict(os.environ, PATH=f"{root}:{os.environ['PATH']}")
            env.pop("CARGO_HAULER_INSIDE", None)
            result = subprocess.run(
                ["bash", "-c", 'source "$1"; cargo test example', "test", str(SCRIPTS / "cargo.sh")],
                env=env, capture_output=True, text=True,
            )
            self.assertEqual(result.returncode, 17, result.stderr)
            self.assertEqual(result.stdout, "test output\n")
            master, slave = pty.openpty()
            try:
                interactive = subprocess.run(
                    ["bash", "-c", 'source "$1"; cargo test example', "test", str(SCRIPTS / "cargo.sh")],
                    env=env, stdout=slave, stderr=subprocess.PIPE, text=True,
                )
                self.assertEqual(interactive.returncode, 17, interactive.stderr)
            finally:
                os.close(slave)
                os.close(master)

    def run_gate(self, explicit=None, skip=False, linked=True, stage=None, fail=""):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            scripts = root / "dev/scripts"
            scripts.mkdir(parents=True)
            install = root / "installed game"
            install.mkdir()
            (install / "RGFX.EXE").touch()
            if linked:
                (root / "redguard").symlink_to(install, target_is_directory=True)
            shutil.copyfile(SCRIPTS / "gates.sh", scripts / "gates.sh")
            (scripts / "toolchain.sh").write_text("")
            for name in ("test-build-provenance.py", "test-surface-audio.py", "capture-provenance.test.mjs",
                         "browser-provenance.test.mjs", "matched-capture.test.mjs"):
                (scripts / name).write_text("")
            (scripts / "cargo.sh").write_text('''cargo() {
    echo "cargo $*"
    [[ "$1" != "$FAIL_COMMAND" ]] || return 17
    [[ "${REDGUARD_DIR-}" == "$EXPECTED_INSTALL" ]] || return 98
    if [[ "$1" == test && "$TEST_SKIP" == yes ]]; then
        echo 'skipped: no install (synthetic test)'
    fi
}
''')
            for name in ("publish-check.sh", "genmap.sh"):
                script = scripts / name
                script.write_text("#!/bin/bash\nexit 0\n")
                script.chmod(0o755)
            env = dict(os.environ, EXPECTED_INSTALL=explicit or (str(install) if linked else ""), TEST_SKIP="yes" if skip else "no", FAIL_COMMAND=fail)
            env.pop("REDGUARD_DIR", None)
            if explicit is not None:
                env["REDGUARD_DIR"] = explicit
            args = ["bash", str(scripts / "gates.sh")]
            if stage is not None:
                args.append(stage)
            return subprocess.run(args, env=env, capture_output=True, text=True)

    def test_split_gates_cover_the_full_gate(self):
        def cargo_calls(result):
            self.assertEqual(result.returncode, 0, result.stderr)
            return [line for line in result.stdout.splitlines() if line.startswith("cargo ")]

        lint = cargo_calls(self.run_gate(stage="lints"))
        test = cargo_calls(self.run_gate(stage="test"))
        self.assertEqual(lint, ["cargo fmt --all -- --check",
                                "cargo clippy --workspace --all-targets -- -D warnings"])
        self.assertEqual(test, ["cargo test --workspace -- --nocapture"])
        self.assertEqual(cargo_calls(self.run_gate()), lint + test)
        self.assertEqual(cargo_calls(self.run_gate(stage="all")), lint + test)

    def test_split_gate_failures_are_not_hidden(self):
        for stage, command in (("lints", "fmt"), ("lints", "clippy"), ("test", "test")):
            with self.subTest(stage=stage, command=command):
                self.assertEqual(self.run_gate(stage=stage, fail=command).returncode, 17)
        self.assertNotEqual(self.run_gate(stage="test", skip=True).returncode, 0)
        self.assertEqual(self.run_gate(stage="unknown").returncode, 2)

    def test_gate_resolves_link_without_overriding_explicit_install(self):
        for explicit in (None, "/explicit/install"):
            with self.subTest(explicit=explicit):
                result = self.run_gate(explicit)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertIn("gates: green", result.stdout)

    def test_gate_allows_reported_skips_without_an_install(self):
        result = self.run_gate(skip=True, linked=False)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("gates: 1 install tests skipped", result.stdout)

    def test_gate_still_rejects_skipped_installed_tests(self):
        result = self.run_gate(skip=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("1 install tests skipped", result.stderr)


if __name__ == "__main__":
    unittest.main()
