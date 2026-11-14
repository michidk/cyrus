#!/usr/bin/env python3
"""Exercise dependency policy against manifests and lockfiles in temporary repos."""

import importlib.util
import tempfile
import unittest
from pathlib import Path

spec = importlib.util.spec_from_file_location(
    "preservation_release", Path(__file__).with_name("check-preservation-release.py")
)
policy = importlib.util.module_from_spec(spec)
spec.loader.exec_module(policy)


class ReleasePolicy(unittest.TestCase):
    def check(self, dependency='"=0.12.4"', *, extra="", version="0.12.4",
              source="registry+https://github.com/rust-lang/crates.io-index", checksum="a" * 64):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "Cargo.toml").write_text(
                f"[workspace.dependencies]\nredguard-preservation = {dependency}\n{extra}"
            )
            (root / "Cargo.lock").write_text(
                f'[[package]]\nname = "redguard-preservation"\nversion = "{version}"\n'
                f'source = "{source}"\nchecksum = "{checksum}"\n'
            )
            policy.check(root)

    def test_accepts_published_release(self):
        self.check()
        self.check('"=0.13.0"', version="0.13.0")

    def test_rejects_git_path_ranges_and_prereleases(self):
        for dependency in ['{ git = "https://example.org/rgpre", rev = "abc" }',
                           '{ path = "../rgpre" }', '"0.12"', '"=0.13.0-beta.1"']:
            with self.subTest(dependency=dependency), self.assertRaises(ValueError):
                self.check(dependency)

    def test_rejects_overridden_sources(self):
        for extra in ['[patch.crates-io]\nredguard-preservation = { path = "../rgpre" }',
                      '[patch.crates-io]\nrgpre = { package = "redguard-preservation", path = "../rgpre" }',
                      '[replace]\n"redguard-preservation:0.12.4" = { path = "../rgpre" }']:
            with self.subTest(extra=extra), self.assertRaises(ValueError):
                self.check(extra=extra)

    def test_rejects_wrong_lockfile(self):
        for change in [dict(version="0.12.3"), dict(source="git+https://example.org/rgpre"),
                       dict(checksum="")]:
            with self.subTest(change=change), self.assertRaises(ValueError):
                self.check(**change)


if __name__ == "__main__":
    unittest.main()
