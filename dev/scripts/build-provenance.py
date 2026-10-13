#!/usr/bin/env python3
"""Bind development build outputs to a stable snapshot of repository build inputs."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess


INPUTS = ("Cargo.toml", "Cargo.lock", "rust-toolchain.toml", ".cargo", "crates", "dev")


def digest(filename):
    with filename.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def source(root):
    files = {}
    for name in INPUTS:
        entry = root / name
        if not entry.exists():
            continue
        paths = [entry, *sorted(entry.rglob("*"))] if entry.is_dir() else [entry]
        for filename in paths:
            if "__pycache__" in filename.parts:
                continue
            if filename.is_symlink():
                raise ValueError(f"build input symlink cannot be fingerprinted: {filename}")
            if filename.is_file():
                files[filename.relative_to(root).as_posix()] = digest(filename)
    encoded = json.dumps(files, sort_keys=True, separators=(",", ":")).encode()
    revision = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip()
    return {"revision": revision, "scope": list(INPUTS), "sha256": hashlib.sha256(encoded).hexdigest(), "files": files}


def seal(root, before, artifacts):
    after = source(root)
    if after != before:
        raise ValueError("source changed during build; rebuild before recording provenance")
    if not artifacts:
        raise ValueError("no build artifacts supplied")
    return {"version": 1, "source": before, "artifacts": {
        name: digest(root / name) for name in sorted(artifacts)
    }}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("operation", choices=("snapshot", "seal", "verify"))
    parser.add_argument("record", type=Path)
    parser.add_argument("artifacts", nargs="*")
    args = parser.parse_args()
    root = Path.cwd()
    if args.operation == "snapshot":
        record = source(root)
    else:
        record = json.loads(args.record.read_text())
        if args.operation == "verify":
            if record.get("version") != 1:
                raise ValueError("unsupported build provenance version")
            expected = seal(root, record["source"], record["artifacts"])
            if record != expected:
                raise ValueError("build artifacts changed; rebuild before capture")
            return
        record = seal(root, record, args.artifacts)
    args.record.write_text(json.dumps(record, sort_keys=True, indent=2) + "\n")


if __name__ == "__main__":
    main()
