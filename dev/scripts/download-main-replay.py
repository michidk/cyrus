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


def download(commit, destination, fallback_main=False):
    runs = json.loads(subprocess.check_output([
        "gh", "run", "list", "--repo", REPOSITORY, "--workflow", "pr-artifacts.yml",
        "--branch", "main", "--event", "push", "--limit", "50",
        "--json", "databaseId,headSha",
    ], text=True))
    matching = [run for run in runs if run["headSha"] == commit]
    if fallback_main:
        # Run-list order is newest first. Only ancestors of the requested main
        # commit are eligible; a later push must not silently change this run.
        matching = []
        for candidate in runs:
            ancestor = subprocess.run(
                ["git", "merge-base", "--is-ancestor", candidate["headSha"], commit],
                check=False, capture_output=True)
            # A push can arrive after the wrapper fetched main. Its object is
            # then unknown locally (128), and cannot qualify as an ancestor.
            if ancestor.returncode in (1, 128):
                continue
            if ancestor.returncode:
                raise ValueError(f"cannot establish main ancestry for {candidate['headSha']}")
            artifacts = json.loads(subprocess.check_output([
                "gh", "api", f"repos/{REPOSITORY}/actions/runs/{candidate['databaseId']}/artifacts",
                "--jq", '[.artifacts[] | select(.name == "' + ARTIFACT + '" and .expired == false)]',
            ], text=True))
            if artifacts:
                matching = [candidate]
                break
    if not matching:
        detail = "no uploaded Linux artifact among the latest 50 main runs" if fallback_main else "no artifact workflow run found"
        raise ValueError(f"main {commit}: {detail}")
    selected = matching[0]
    run = selected["databaseId"]
    selected_commit = selected["headSha"]
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
    print(f"Replay artifact: main {selected_commit}; https://github.com/{REPOSITORY}/actions/runs/{run}", flush=True)
    return dict(commit=selected_commit, run=run)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("commit")
    parser.add_argument("destination", type=Path)
    parser.add_argument("--fallback-main", action="store_true", help="select the newest ancestor main run with an uploaded Linux artifact")
    parser.add_argument("--receipt", type=Path, help="write the selected binary commit and workflow run")
    args = parser.parse_args()
    try:
        receipt = download(args.commit, args.destination, args.fallback_main)
        if args.receipt:
            args.receipt.write_text(json.dumps(receipt) + "\n")
    except (OSError, ValueError, tarfile.TarError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"main replay artifact unavailable: {error}\n")
