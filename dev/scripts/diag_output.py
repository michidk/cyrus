"""Durable diagnostic output for long original-game runs.

A pod replacement or a cleaned artifacts directory has twice removed finished diagnostic runs
before they were read. `DurableLog` writes each JSON row with flush and fsync, optionally
mirrors it to a second directory on another path, and seals the run with a manifest (row count,
bytes, sha256) written atomically. `verify` re-reads a log against its manifest, so a missing,
truncated or edited log is reported instead of trusted.
"""
import hashlib
import json
import os
from pathlib import Path


def write_atomic(path, data):
    """Write bytes to `path` through a temporary file, fsync, then rename into place."""
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(path.name + ".tmp")
    with temporary.open("wb") as stream:
        stream.write(data)
        stream.flush()
        os.fsync(stream.fileno())
    temporary.replace(path)


def manifest_path(path):
    return Path(str(path) + ".manifest.json")


class DurableLog:
    """Append JSON rows to `path` (and `mirror`, if given) with an fsync per row."""

    def __init__(self, path, mirror=None):
        self.paths = [Path(path)] + ([Path(mirror)] if mirror else [])
        self.streams = []
        for target in self.paths:
            target.parent.mkdir(parents=True, exist_ok=True)
            self.streams.append(target.open("w"))
        self.rows = 0

    def append(self, row):
        line = json.dumps(row) + "\n"
        for stream in self.streams:
            stream.write(line)
            stream.flush()
            os.fsync(stream.fileno())
        self.rows += 1

    def seal(self, note=""):
        """Close the streams and write each log's manifest; returns the manifest."""
        for stream in self.streams:
            stream.close()
        manifest = {}
        for target in self.paths:
            data = target.read_bytes()
            manifest = {"rows": self.rows, "bytes": len(data),
                        "sha256": hashlib.sha256(data).hexdigest(), "note": note}
            write_atomic(manifest_path(target), (json.dumps(manifest, indent=1) + "\n").encode())
        return manifest


def verify(path):
    """(ok, reason): the log exists, has a manifest, and matches its rows, bytes and digest."""
    path = Path(path)
    if not path.exists():
        return False, "log missing"
    manifest = manifest_path(path)
    if not manifest.exists():
        return False, "no manifest: the run did not finish or was cut off"
    expected = json.loads(manifest.read_text())
    data = path.read_bytes()
    if len(data) != expected["bytes"]:
        return False, f"size {len(data)} differs from the sealed {expected['bytes']}"
    if hashlib.sha256(data).hexdigest() != expected["sha256"]:
        return False, "digest differs from the sealed log"
    rows = sum(1 for line in data.splitlines() if line.strip())
    if rows != expected["rows"]:
        return False, f"{rows} rows differ from the sealed {expected['rows']}"
    return True, "ok"
