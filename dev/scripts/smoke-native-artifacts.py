#!/usr/bin/env python3
"""Run --help from native release payloads, without a game install or window."""
from pathlib import Path
import os
import subprocess
import sys
import tarfile
import tempfile
import zipfile


def smoke(artifacts: Path, target: str) -> None:
    windows = "windows" in target
    suffix = ".exe" if windows else ""
    with tempfile.TemporaryDirectory(prefix="cyrus-package-smoke-") as directory:
        root = Path(directory)
        env = dict(os.environ, CYRUS_CONFIG_DIR=str(root / "config"))

        def run(package: Path, app: str, folder: str) -> None:
            destination = root / folder
            destination.mkdir()
            try:
                if package.suffix == ".exe":
                    binary = package
                else:
                    binary = destination / f"{app}{suffix}"
                    if package.suffix == ".zip":
                        with zipfile.ZipFile(package) as archive:
                            binary.write_bytes(archive.read(binary.name))
                        binary.chmod(0o755)
                    else:
                        with tarfile.open(package) as archive:
                            member = next((entry for entry in archive
                                           if entry.name.removeprefix("./") == binary.name
                                           and entry.isfile()), None)
                            if member is None:
                                raise RuntimeError(f"{package}: missing regular executable {binary.name}")
                            with archive.extractfile(member) as source:
                                binary.write_bytes(source.read())
                            binary.chmod(member.mode & 0o777)
                subprocess.run([str(binary), "--help"], cwd=destination,
                               env=env, check=True, timeout=30)
            except (OSError, subprocess.SubprocessError, tarfile.TarError,
                    zipfile.BadZipFile, KeyError) as error:
                raise RuntimeError(f"{package}: packaged {app} smoke failed: {error}") from error

        extension = ".zip" if windows else ".tar.gz"
        run(artifacts / f"cyrus-{target}{extension}", "cyrus", "game")
        viewer = artifacts / f"cyrus-viewer-{target}{extension}"
        if viewer.exists():
            run(viewer, "cyrus-viewer", "viewer")
        bundle_extension = ".zip" if windows else ".tar.gz"
        run(artifacts / f"cyrus-with-mods-{target}{bundle_extension}", "cyrus", "bundle")


if __name__ == "__main__":
    if len(sys.argv) != 3:
        sys.exit("usage: smoke-native-artifacts.py ARTIFACT_DIRECTORY RUST_TARGET")
    try:
        smoke(Path(sys.argv[1]).resolve(), sys.argv[2])
    except RuntimeError as error:
        sys.exit(str(error))
