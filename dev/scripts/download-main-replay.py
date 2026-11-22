#!/usr/bin/env python3
"""Fetch the Linux replay executable built for an exact main commit, without Cargo."""
import argparse
import json
from pathlib import Path
import subprocess
import tarfile
import tempfile

REPOSITORY = "michidk/cyrus"
ARTIFACT = "native-x86_64-unknown-linux-gnu"


def download(commit, destination):
    runs = json.loads(subprocess.check_output([
        "gh", "run", "list", "--repo", REPOSITORY, "--workflow", "pr-artifacts.yml",
        "--branch", "main", "--event", "push", "--limit", "50",
        "--json", "databaseId,headSha",
    ], text=True))
    matching = [run for run in runs if run["headSha"] == commit]
    if not matching:
        raise ValueError(f"main {commit}: no artifact workflow run found")
    run = matching[0]["databaseId"]
    destination.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=destination.parent) as temporary:
        subprocess.run([
            "gh", "run", "download", str(run), "--repo", REPOSITORY,
            "--name", ARTIFACT, "--dir", temporary,
        ], check=True)
        package = Path(temporary) / "cyrus-x86_64-unknown-linux-gnu.tar.gz"
        with tarfile.open(package) as archive:
            binaries = [member for member in archive.getmembers()
                        if member.name in ("cyrus", "./cyrus") and member.isfile()]
            if len(binaries) != 1:
                raise ValueError(f"main {commit}: package must contain one cyrus executable")
            staged = Path(temporary) / "cyrus"
            with archive.extractfile(binaries[0]) as source, staged.open("wb") as output:
                while chunk := source.read(1024 * 1024):
                    output.write(chunk)
            staged.chmod(0o755)
            staged.replace(destination)
    print(f"Replay artifact: main {commit}; https://github.com/{REPOSITORY}/actions/runs/{run}", flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("commit")
    parser.add_argument("destination", type=Path)
    args = parser.parse_args()
    try:
        download(args.commit, args.destination)
    except (OSError, ValueError, tarfile.TarError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"main replay artifact unavailable: {error}\n")
