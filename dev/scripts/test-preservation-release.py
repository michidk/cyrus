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


class IsolatedReleasePolicy(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)
        (self.root / 'Cargo.toml').write_text(
            '[workspace.dependencies]\nredguard-preservation = "=0.12.4"\n')
        self.lock(self.root)
        self.mod = self.root / 'mods/ragdolls'
        self.mod.mkdir(parents=True)

    def lock(self, folder, version='0.12.4', checksum='a' * 64):
        (folder / 'Cargo.lock').write_text(
            '[[package]]\nname = "redguard-preservation"\n'
            f'version = "{version}"\n'
            'source = "registry+https://github.com/rust-lang/crates.io-index"\n'
            f'checksum = "{checksum}"\n')

    def manifest(self, dependency='"=0.12.4"', section='dev-dependencies', extra=''):
        (self.mod / 'Cargo.toml').write_text(
            f'[workspace]\n[{section}]\nredguard-preservation = {dependency}\n{extra}')

    def test_matching_isolated_release_and_feature_table_are_accepted(self):
        for dependency in ['"=0.12.4"', '{ version = "=0.12.4", default-features = false }']:
            self.manifest(dependency)
            self.lock(self.mod)
            policy.check(self.root)

    def test_stale_isolated_pins_are_rejected_in_all_dependency_sections(self):
        for section in ['dependencies', 'dev-dependencies', 'build-dependencies',
                        'target.\'cfg(unix)\'.dev-dependencies']:
            with self.subTest(section=section):
                self.manifest('"=0.12.3"', section)
                self.lock(self.mod, '0.12.3')
                with self.assertRaisesRegex(ValueError, 'mods/ragdolls/Cargo.toml'):
                    policy.check(self.root)

    def test_stale_or_corrupted_isolated_lock_is_rejected(self):
        self.manifest()
        for change in [dict(version='0.12.3'), dict(checksum='b' * 64)]:
            with self.subTest(change=change):
                self.lock(self.mod, **change)
                with self.assertRaisesRegex(ValueError, 'mods/ragdolls/Cargo.lock'):
                    policy.check(self.root)

    def test_isolated_sources_cannot_override_the_release(self):
        for dependency in ['{ version = "=0.12.4", path = "../../upstream" }',
                           '{ version = "=0.12.4", git = "https://example.org/upstream" }']:
            with self.subTest(dependency=dependency):
                self.manifest(dependency)
                self.lock(self.mod)
                with self.assertRaisesRegex(ValueError, 'mods/ragdolls/Cargo.toml'):
                    policy.check(self.root)
        self.manifest(extra='[patch.crates-io]\nrgpre = { package = "redguard-preservation", path = "../../upstream" }')
        with self.assertRaisesRegex(ValueError, 'mods/ragdolls/Cargo.toml'):
            policy.check(self.root)

    def test_aliases_follow_the_same_release_policy(self):
        self.lock(self.mod)
        for version in ['0.12.4', '0.12.3']:
            (self.mod / 'Cargo.toml').write_text(
                '[workspace]\n[target.\'cfg(unix)\'.build-dependencies]\n'
                f'rgpre = {{ package = "redguard-preservation", version = "={version}" }}\n')
            if version == '0.12.4':
                policy.check(self.root)
            else:
                with self.assertRaisesRegex(ValueError, 'mods/ragdolls/Cargo.toml'):
                    policy.check(self.root)

    def test_isolated_replacements_and_alternate_registries_are_rejected(self):
        self.lock(self.mod)
        self.manifest(extra='[replace]\n"redguard-preservation:0.12.4" = { path = "../../upstream" }')
        with self.assertRaisesRegex(ValueError, 'mods/ragdolls/Cargo.toml'):
            policy.check(self.root)
        self.manifest('{ version = "=0.12.4", registry = "other" }')
        with self.assertRaisesRegex(ValueError, 'mods/ragdolls/Cargo.toml'):
            policy.check(self.root)

    def test_required_lock_must_exist_but_unrelated_mod_needs_none(self):
        (self.mod / 'Cargo.toml').write_text('[workspace]\n')
        policy.check(self.root)
        self.manifest()
        with self.assertRaisesRegex(ValueError, 'mods/ragdolls/Cargo.lock'):
            policy.check(self.root)

    def test_transitive_isolated_lock_cannot_retain_an_old_release(self):
        (self.mod / 'Cargo.toml').write_text('[workspace]\n')
        self.lock(self.mod, '0.12.3')
        with self.assertRaisesRegex(ValueError, 'mods/ragdolls/Cargo.lock'):
            policy.check(self.root)


if __name__ == "__main__":
    unittest.main()
