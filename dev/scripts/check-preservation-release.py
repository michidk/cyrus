#!/usr/bin/env python3
"""Require an exact crates.io release of preservation, without source overrides."""

import re
import sys
import tomllib
from pathlib import Path


def check_overrides(manifest: dict, label: str) -> None:
    for overrides in manifest.get("patch", {}).values():
        if any(name == "redguard-preservation" or value.get("package") == "redguard-preservation"
               for name, value in overrides.items()):
            raise ValueError(f"{label}: preservation source overrides are forbidden")
    if any(name.split(":")[0] == "redguard-preservation" for name in manifest.get("replace", {})):
        raise ValueError(f"{label}: preservation replacements are forbidden")


def check_lock(path: Path, label: str, version: str, *, required: bool,
               checksum: str | None = None) -> str | None:
    if not path.exists():
        if required:
            raise ValueError(f"{label}: missing lockfile for preservation dependency")
        return None
    lock = tomllib.loads(path.read_text())
    packages = [package for package in lock.get("package", [])
                if package["name"] == "redguard-preservation"]
    if not packages and not required:
        return None
    if len(packages) != 1:
        raise ValueError(f"{label}: expected one preservation release")
    package = packages[0]
    actual_checksum = package.get("checksum", "")
    if (package.get("source") != "registry+https://github.com/rust-lang/crates.io-index"
            or package["version"] != version
            or not re.fullmatch(r"[0-9a-f]{64}", actual_checksum)
            or checksum is not None and actual_checksum != checksum):
        raise ValueError(f"{label}: preservation must match the pinned crates.io release and checksum")
    return actual_checksum


def preservation_dependencies(manifest: dict):
    """Include aliases and target-specific build, normal and test dependencies."""
    for table in [manifest, *manifest.get("target", {}).values()]:
        for section in ("dependencies", "dev-dependencies", "build-dependencies"):
            for name, value in table.get(section, {}).items():
                if (name == "redguard-preservation"
                        or isinstance(value, dict) and value.get("package") == "redguard-preservation"):
                    yield value


def check(root: Path) -> None:
    manifest = tomllib.loads((root / "Cargo.toml").read_text())
    dependency = manifest["workspace"]["dependencies"]["redguard-preservation"]
    if not isinstance(dependency, str) or not re.fullmatch(r"=\d+\.\d+\.\d+", dependency):
        raise ValueError("Cargo.toml: redguard-preservation must pin an exact stable crates.io release (=X.Y.Z)")
    check_overrides(manifest, "Cargo.toml")
    checksum = check_lock(root / "Cargo.lock", "Cargo.lock", dependency[1:], required=True)
    for path in sorted((root / "mods").glob("*/Cargo.toml")):
        label = path.relative_to(root).as_posix()
        isolated = tomllib.loads(path.read_text())
        check_overrides(isolated, label)
        dependencies = list(preservation_dependencies(isolated))
        for value in dependencies:
            if isinstance(value, dict):
                if any(key in value for key in ("git", "path", "registry", "registry-index")):
                    raise ValueError(f"{label}: preservation source overrides are forbidden")
                value = value.get("version")
            if value != dependency:
                raise ValueError(f"{label}: preservation must match the workspace exact release {dependency}")
        lock = path.with_name("Cargo.lock")
        check_lock(lock, lock.relative_to(root).as_posix(), dependency[1:],
                   required=bool(dependencies), checksum=checksum)


if __name__ == "__main__":
    try:
        check(Path(__file__).resolve().parents[2])
    except (ValueError, KeyError) as error:
        sys.exit(f"preservation-release: {error}")
    print("preservation-release: exact crates.io release, no source overrides")
