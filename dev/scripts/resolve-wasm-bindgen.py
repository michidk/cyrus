#!/usr/bin/env python3
"""Select an exact-schema bindgen CLI before spending time compiling WASM."""
import subprocess
import sys
import tomllib
from pathlib import Path


def resolve(lock, current, tools):
    packages = tomllib.loads(Path(lock).read_text())["package"]
    versions = {p["version"] for p in packages if p["name"] == "wasm-bindgen"}
    if len(versions) != 1:
        raise ValueError(f"expected one wasm-bindgen schema version, got {sorted(versions)}")
    version = versions.pop()
    candidates = [Path(current), Path(tools) / f"wasm-bindgen-{version}" / "wasm-bindgen"]
    for candidate in candidates:
        try:
            result = subprocess.run([str(candidate), "--version"], capture_output=True, text=True, check=True)
        except (OSError, subprocess.CalledProcessError):
            continue
        if result.stdout.strip().split() == ["wasm-bindgen", version]:
            return candidate.resolve()
    raise ValueError(f"wasm-bindgen {version} CLI required by {lock}; run just toolchain before building")


if __name__ == "__main__":
    try:
        print(resolve(*sys.argv[1:]))
    except (ValueError, TypeError) as error:
        sys.exit(str(error))
