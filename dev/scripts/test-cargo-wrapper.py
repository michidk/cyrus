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

    def test_boundary_consumers_preserve_the_inherited_target_directory(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            scripts = root / "dev/scripts"
            scripts.mkdir(parents=True)
            shutil.copyfile(SCRIPTS / "check-core-boundaries.sh", scripts / "check-core-boundaries.sh")
            (root / "Cargo.lock").write_text("version = 4\n")
            (scripts / "toolchain.sh").write_text("")
            (scripts / "cargo.sh").write_text('''cargo() {
    [[ "$CARGO_TARGET_DIR" == "$EXPECTED_TARGET" ]] || return 98
    [[ "$*" != *--target-dir* ]] || return 99
    printf '%s\n' "$1" >> "$CARGO_CALLS"
}
''')
            calls = root / "calls"
            target = str(root / "shared target")
            env = dict(os.environ, CARGO_TARGET_DIR=target, EXPECTED_TARGET=target,
                       CARGO_CALLS=str(calls))
            result = subprocess.run(
                ["bash", str(scripts / "check-core-boundaries.sh")],
                env=env, capture_output=True, text=True,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(calls.read_text().splitlines(), ["tree", "check"] * 5)

    def test_setup_uses_the_same_single_build_limit(self):
        self.assertIn("export CARGO_HAULER_MAX_CONCURRENT=1", (SCRIPTS / "setup-hauler.sh").read_text())

    def run_gate(self, explicit=None, skip=False, linked=True, stage=None, fail="", package=None, script_fail=False, messages=False):
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
            # Exercise orchestration with inert script checks, including newly added
            # checks. Running their real bodies belongs to the real gate, not this
            # synthetic checkout with a mocked Cargo command and install.
            checks = list(SCRIPTS.glob("test-*.py")) + list(SCRIPTS.glob("*.test.mjs")) + [SCRIPTS / "test-local-install.mjs", SCRIPTS / "test-game-assets.mjs"]
            checks.append(SCRIPTS / "check-preservation-release.py")
            for check in checks:
                (scripts / check.name).write_text("")
            (scripts / "test-newly-added.py").write_text(
                "import sys\nprint('new script discovered')\nsys.exit(" +
                ("19" if script_fail else "0") + ")\n")
            (scripts / "cargo.sh").write_text('''cargo() {
    echo "cargo $*"
    [[ "$1" != "$FAIL_COMMAND" ]] || return 17
    [[ "${REDGUARD_DIR-}" == "$EXPECTED_INSTALL" ]] || return 98
    if [[ "$1" == test && "$TEST_SKIP" == yes ]]; then
        echo 'skipped: no install (synthetic test)'
    fi
}
''')
            for name in ("publish-check.sh", "genmap.sh", "check-core-boundaries.sh"):
                script = scripts / name
                script.write_text("#!/bin/bash\nexit 0\n")
                script.chmod(0o755)
            env = dict(os.environ, EXPECTED_INSTALL=explicit or (str(install) if linked else ""), TEST_SKIP="yes" if skip else "no", FAIL_COMMAND=fail)
            env.pop("REDGUARD_DIR", None)
            if explicit is not None:
                env["REDGUARD_DIR"] = explicit
            if messages:
                env["CYRUS_TEST_MESSAGES"] = str(root / "messages.jsonl")
            args = ["bash", str(scripts / "gates.sh")]
            if stage is not None:
                args.append(stage)
            if package is not None:
                args.append(package)
            result = subprocess.run(args, env=env, capture_output=True, text=True)
            result.messages = (root / "messages.jsonl").read_text() if messages and (root / "messages.jsonl").exists() else None
            return result

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

    def test_new_script_is_discovered_and_its_failure_stops_the_gate(self):
        result = self.run_gate(stage="check")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("new script discovered", result.stdout)
        failed = self.run_gate(stage="check", script_fail=True)
        self.assertEqual(failed.returncode, 19)
        self.assertNotIn("cargo fmt", failed.stdout)

    def test_ci_test_stage_retains_all_features_and_artifact_messages(self):
        result = self.run_gate(stage="test", messages=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("cargo test --workspace --all-features --message-format=json", result.messages)
        self.assertEqual(self.run_gate(stage="test", messages=True, fail="test").returncode, 17)

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
