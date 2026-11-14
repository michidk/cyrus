#!/usr/bin/env python3
"""Require an exact crates.io release of preservation, without source overrides."""

import re
import sys
import tomllib
from pathlib import Path


def check(root: Path) -> None:
    manifest = tomllib.loads((root / "Cargo.toml").read_text())
    dependency = manifest["workspace"]["dependencies"]["redguard-preservation"]
    if not isinstance(dependency, str) or not re.fullmatch(r"=\d+\.\d+\.\d+", dependency):
        raise ValueError("Cargo.toml: redguard-preservation must pin an exact stable crates.io release (=X.Y.Z)")
    for overrides in manifest.get("patch", {}).values():
        if any(name == "redguard-preservation" or value.get("package") == "redguard-preservation"
               for name, value in overrides.items()):
            raise ValueError("Cargo.toml: preservation source overrides are forbidden")
    if any(name.split(":")[0] == "redguard-preservation" for name in manifest.get("replace", {})):
        raise ValueError("Cargo.toml: preservation replacements are forbidden")
    lock = tomllib.loads((root / "Cargo.lock").read_text())
    packages = [package for package in lock["package"] if package["name"] == "redguard-preservation"]
    if len(packages) != 1:
        raise ValueError("Cargo.lock: expected one preservation release")
    package = packages[0]
    if (package.get("source") != "registry+https://github.com/rust-lang/crates.io-index"
            or package["version"] != dependency[1:]
            or not re.fullmatch(r"[0-9a-f]{64}", package.get("checksum", ""))):
        raise ValueError("Cargo.lock: preservation must match the pinned crates.io release and checksum")


if __name__ == "__main__":
    try:
        check(Path(__file__).resolve().parents[2])
    except (ValueError, KeyError) as error:
        sys.exit(f"preservation-release: {error}")
    print("preservation-release: exact crates.io release, no source overrides")
