#!/usr/bin/env python3
import importlib.util
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("resolver", Path(__file__).with_name("resolve-wasm-bindgen.py"))
resolver = importlib.util.module_from_spec(spec)
spec.loader.exec_module(resolver)


class BindgenSelection(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.lock = self.root / "Cargo.lock"
        self.lock.write_text('[[package]]\nname="wasm-bindgen"\nversion="0.2.130"\n')
        self.current = self.root / "active-cli"
        self.retained = self.root / "wasm-bindgen-0.2.130" / "wasm-bindgen"

    def cli(self, path, version):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(f'#!/bin/sh\necho "wasm-bindgen {version}"\n')
        path.chmod(0o755)

    def test_current_exact_cli_wins(self):
        self.cli(self.current, "0.2.130")
        self.assertEqual(resolver.resolve(self.lock, self.current, self.root), self.current)

    def test_stale_path_recovers_exact_retained_cli(self):
        self.cli(self.current, "0.2.129")
        self.cli(self.retained, "0.2.130")
        self.assertEqual(resolver.resolve(self.lock, self.current, self.root), self.retained)

    def test_retained_directory_name_does_not_prove_its_schema(self):
        self.cli(self.current, "0.2.129")
        self.cli(self.retained, "0.2.129")
        with self.assertRaisesRegex(ValueError, "0.2.130 CLI required"):
            resolver.resolve(self.lock, self.current, self.root)

    def test_missing_cli_fails_before_build(self):
        with self.assertRaisesRegex(ValueError, "just toolchain"):
            resolver.resolve(self.lock, self.current, self.root)


if __name__ == "__main__":
    unittest.main()
