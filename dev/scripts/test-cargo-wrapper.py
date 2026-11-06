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
[[ "$CARGO_HAULER_MAX_CONCURRENT" == 1 ]] || exit 99
[[ ! -t 1 ]] || exit 99
[[ "$CARGO_HAULER_MAX_CONCURRENT" == 1 ]] || exit 99
[[ "$CARGO_HAULER_OVERLAP_EXECUTION" == 0 ]] || exit 99
echo 'test output'
exit 17
''')
            hauler.chmod(0o755)
            env = dict(os.environ, PATH=f"{root}:{os.environ['PATH']}")
            env.pop("CARGO_HAULER_INSIDE", None)
            env["CARGO_HAULER_MAX_CONCURRENT"] = "3"
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

    def test_setup_uses_the_same_single_build_limit(self):
        self.assertIn("export CARGO_HAULER_MAX_CONCURRENT=1", (SCRIPTS / "setup-hauler.sh").read_text())

    def run_gate(self, explicit=None, skip=False, linked=True, stage=None, fail="", package=None):
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
            if package is not None:
                args.append(package)
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

    def test_check_does_not_compile(self):
        result = self.run_gate(stage="check")
        self.assertEqual(result.returncode, 0, result.stderr)
        calls = [line for line in result.stdout.splitlines() if line.startswith("cargo ")]
        self.assertEqual(calls, ["cargo fmt --all -- --check"])

    def test_verify_runs_all_package_tests_and_preserves_failures(self):
        for package, resolved in (("game", "cyrus-game"), ("cyrus-assets", "cyrus-assets"), ("cyrus", "cyrus")):
            with self.subTest(package=package):
                result = self.run_gate(stage="verify", package=package)
                self.assertEqual(result.returncode, 0, result.stderr)
                calls = [line for line in result.stdout.splitlines() if line.startswith("cargo ")]
                self.assertEqual(calls, [
                    f"cargo clippy -p {resolved} --all-targets -- -D warnings",
                    f"cargo test -p {resolved} -- --nocapture",
                ])
        for command in ("clippy", "test"):
            self.assertEqual(self.run_gate(stage="verify", package="game", fail=command).returncode, 17)
        self.assertNotEqual(self.run_gate(stage="verify", package="game", skip=True).returncode, 0)
        self.assertEqual(self.run_gate(stage="verify").returncode, 2)

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

    def toolchain_flags(self, *, source_twice=False, **environment):
        env = {key: value for key, value in os.environ.items()
               if key not in ("CARGO_HOME", "RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "CARGO_BUILD_RUSTFLAGS")}
        env.update(environment)
        with tempfile.TemporaryDirectory() as directory:
            # A cargo on PATH keeps toolchain.sh from substituting its own CARGO_HOME.
            stub = Path(directory) / "cargo"
            stub.write_text("#!/bin/sh\n")
            stub.chmod(0o755)
            env["PATH"] = f"{directory}:{env['PATH']}"
            result = subprocess.run(
                ["bash", "-c", 'source "$1"; if [ "$2" = twice ]; then source "$1"; fi; printf %s "${CARGO_BUILD_RUSTFLAGS-}"', "test",
                 str(SCRIPTS / "toolchain.sh"), "twice" if source_twice else "once"],
                capture_output=True, text=True, env=env)
        self.assertEqual(result.returncode, 0, result.stderr)
        return result.stdout

    def test_toolchain_remaps_dependency_paths_so_shared_targets_stay_consistent(self):
        flags = self.toolchain_flags(CARGO_HOME="/tools-a/cargo")
        self.assertIn("--remap-path-prefix=/tools-a/cargo/registry/src=/cargo/registry/src", flags)
        self.assertIn("--remap-path-prefix=/tools-a/cargo/git/checkouts=/cargo/git/checkouts", flags)
        other = self.toolchain_flags(CARGO_HOME="/tools-b/cargo")
        self.assertEqual(
            flags.replace("/tools-a/", "/tools-b/"), other,
            "only the CARGO_HOME prefix may differ between checkouts",
        )

    def test_toolchain_aliases_produce_identical_flags_and_repeated_sourcing_is_stable(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            canonical = root / "cargo"
            canonical.mkdir()
            alias = root / "alias"
            alias.symlink_to(canonical, target_is_directory=True)
            flags = self.toolchain_flags(CARGO_HOME=str(canonical), CARGO_BUILD_RUSTFLAGS="-Dwarnings")
            self.assertEqual(self.toolchain_flags(CARGO_HOME=str(alias), CARGO_BUILD_RUSTFLAGS="-Dwarnings"), flags)
            self.assertEqual(self.toolchain_flags(source_twice=True, CARGO_HOME=str(alias), CARGO_BUILD_RUSTFLAGS="-Dwarnings"), flags)
            self.assertEqual(flags.count("--remap-path-prefix="), 2)
            self.assertTrue(flags.startswith("-Dwarnings "))

    def test_setup_hooks_keep_single_execution_and_preserve_other_hooks(self):
        import json
        source = (SCRIPTS / "setup-hauler.sh").read_text()
        fragment = source.split("<<'PY'\n", 1)[1].split("\nPY", 1)[0]
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "hooks.json"
            path.write_text(json.dumps({"hooks": {"Stop": [{"hooks": [{"type": "command", "command": "other-hook"}]}]}}))
            prefix = "import os; os.path.expanduser = lambda _: " + repr(str(path)) + "\n"
            result = subprocess.run(["python3", "-c", prefix + fragment, "/shared/hooks"],
                env=dict(os.environ, CARGO_HAULER_MAX_CONCURRENT="1", CARGO_HAULER_OVERLAP_EXECUTION="0"),
                text=True, capture_output=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            hooks = json.loads(path.read_text())["hooks"]
            commands = [hook["command"] for groups in hooks.values() for group in groups for hook in group["hooks"]]
            self.assertIn("other-hook", commands)
            generated = [command for command in commands if "cargo-hauler" in command or "/shared/hooks/" in command]
            self.assertEqual(len(generated), 4)
            for command in generated:
                self.assertIn("CARGO_HAULER_MAX_CONCURRENT=1 ", command)
                self.assertIn("CARGO_HAULER_OVERLAP_EXECUTION=0 ", command)

    def test_toolchain_leaves_explicit_rustflags_alone(self):
        self.assertEqual(self.toolchain_flags(CARGO_HOME="/tools/cargo", RUSTFLAGS="-Dwarnings"), "")
        self.assertEqual(self.toolchain_flags(), "")


if __name__ == "__main__":
    unittest.main()
