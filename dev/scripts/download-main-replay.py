#!/usr/bin/env python3
"""Fetch a Linux replay executable for a main commit or explicit workflow run."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil
import subprocess
import tarfile
import tempfile
import zipfile

REPOSITORY = "michidk/cyrus"
ARTIFACT = "native-x86_64-unknown-linux-gnu"


class ArtifactPending(ValueError):
    """The producing job has not finished publishing checkout evidence."""


def extract_binary(package, staged, context):
    with tarfile.open(package) as archive:
        binaries = [member for member in archive.getmembers()
                    if member.name in ("cyrus", "./cyrus") and member.isfile()]
        if len(binaries) != 1:
            raise ValueError(f"{context}: package must contain one cyrus executable")
        with archive.extractfile(binaries[0]) as source, staged.open("wb") as output:
            shutil.copyfileobj(source, output)
    staged.chmod(0o755)


def checkout_source(run_id, commit, expected_tree):
    """Bind a completed native job's checkout to an explicitly expected source tree."""
    if not re.fullmatch(r'[0-9a-f]{40}', expected_tree):
        raise ValueError('expected checkout tree must be a full Git tree SHA')
    prefix = f'repos/{REPOSITORY}'
    jobs = json.loads(subprocess.check_output(
        ['gh', 'api', f'{prefix}/actions/runs/{run_id}/jobs?per_page=100'], text=True))
    native = [job for job in jobs['jobs'] if job['name'] == 'Native (x86_64-unknown-linux-gnu)']
    if len(native) != 1:
        raise ValueError(f'run {run_id}: requires one standard Linux native job')
    job = native[0]['id']
    if native[0].get('status') != 'completed':
        raise ArtifactPending(
            f'run {run_id} job {job}: checkout evidence pending; retry after job completion')
    if native[0].get('conclusion') != 'success':
        raise ValueError(f'run {run_id} job {job}: native artifact job did not succeed')
    log = subprocess.check_output(
        ['gh', 'api', f'{prefix}/actions/jobs/{job}/logs', '--allow-escape-sequences'], text=True)
    checkouts = re.findall(
        r'^\S+ \[command\].*/git log -1 --format=%H\r?\n\S+ ([0-9a-f]{40})\r?$', log, re.MULTILINE)
    if len(checkouts) != 1:
        raise ValueError(f'job {job}: requires one recorded checkout commit')
    source = json.loads(subprocess.check_output(
        ['gh', 'api', f'{prefix}/git/commits/{checkouts[0]}'], text=True))
    parents = [parent['sha'] for parent in source['parents']]
    if source['sha'] != commit and (len(parents) != 2 or commit not in parents):
        raise ValueError(f'job {job}: checkout does not contain the requested workflow head')
    if source['tree']['sha'] != expected_tree:
        raise ValueError(f'job {job}: checkout tree differs from requested {expected_tree}')
    return dict(checkout_job=job, checkout_commit=source['sha'], checkout_tree=expected_tree)


def artifact_source(record, run, commit, expected_tree, package_name, package_digest):
    """Validate source carried inside the digest-verified standard artifact."""
    if (not isinstance(record, dict) or type(record.get('version')) is not int or record['version'] != 1
            or type(record.get('run')) is not int or record['run'] <= 0
            or type(record.get('attempt')) is not int or record['attempt'] <= 0
            or record.get('repository') != REPOSITORY
            or record.get('run') != run['id']
            or record.get('attempt') != run.get('run_attempt')
            or record.get('head') != commit
            or record.get('checkout_tree') != expected_tree
            or not isinstance(record.get('packages'), dict)
            or record['packages'].get(package_name) != package_digest):
        raise ValueError('artifact source provenance missing or inconsistent with run/head/tree/package')
    checkout = record.get('checkout_commit')
    if not isinstance(checkout, str) or not re.fullmatch(r'[0-9a-f]{40}', checkout):
        raise ValueError('artifact source provenance requires an immutable checkout commit')
    source = json.loads(subprocess.check_output(
        ['gh', 'api', f'repos/{REPOSITORY}/git/commits/{checkout}'], text=True))
    parents = [parent['sha'] for parent in source['parents']]
    if (source['sha'] != checkout or source['tree']['sha'] != expected_tree
            or (checkout != commit and (len(parents) != 2 or commit not in parents))):
        raise ValueError('artifact source provenance checkout differs from immutable source')
    jobs = json.loads(subprocess.check_output(
        ['gh', 'api', f'repos/{REPOSITORY}/actions/runs/{run["id"]}/jobs?per_page=100'], text=True))
    native = [job for job in jobs['jobs'] if job['name'] == 'Native (x86_64-unknown-linux-gnu)']
    if len(native) != 1:
        raise ValueError('artifact source provenance requires one Linux producer job')
    return dict(checkout_job=native[0]['id'], checkout_commit=checkout,
                checkout_tree=expected_tree, source_provenance='artifact')


def download_run(commit, destination, run_id, source_tree=None):
    prefix = f"repos/{REPOSITORY}/actions"
    run = json.loads(subprocess.check_output(
        ["gh", "api", f"{prefix}/runs/{run_id}"], text=True))
    if run["id"] != run_id or run["head_sha"] != commit:
        raise ValueError(f"run {run_id}: workflow head differs from requested {commit}")
    if run["path"] != ".github/workflows/pr-artifacts.yml":
        raise ValueError(f"run {run_id}: not the native artifact workflow")
    listing = json.loads(subprocess.check_output(
        ["gh", "api", f"{prefix}/runs/{run_id}/artifacts"], text=True))
    candidates = [artifact for artifact in listing["artifacts"]
                  if artifact["name"] == ARTIFACT and not artifact["expired"]]
    if len(candidates) != 1:
        raise ValueError(f"run {run_id}: requires one unexpired {ARTIFACT} artifact")
    artifact = candidates[0]
    expected = artifact.get("digest", "")
    if not isinstance(expected, str) or not re.fullmatch(r"sha256:[0-9a-f]{64}", expected):
        raise ValueError(f"artifact {artifact['id']}: missing SHA256 digest")
    checkout = {}
    print(f"Downloading artifact {artifact['id']} from workflow run {run_id}", flush=True)
    destination.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=destination.parent) as temporary:
        folder = Path(temporary)
        bundle = folder / "artifact.zip"
        with bundle.open("wb") as output:
            subprocess.run(["gh", "api", f"{prefix}/artifacts/{artifact['id']}/zip",
                            "--allow-escape-sequences"], stdout=output, check=True)
        with bundle.open("rb") as source:
            archive_hash = hashlib.file_digest(source, "sha256").hexdigest()
        if expected != f"sha256:{archive_hash}":
            raise ValueError(f"artifact {artifact['id']}: archive digest differs")
        package = folder / "cyrus-x86_64-unknown-linux-gnu.tar.gz"
        with zipfile.ZipFile(bundle) as archive:
            if archive.namelist().count(package.name) != 1:
                raise ValueError(f"artifact {artifact['id']}: requires one standard Linux package")
            with archive.open(package.name) as source, package.open("wb") as output:
                shutil.copyfileobj(source, output)
            if source_tree is not None:
                names = archive.namelist()
                if 'source-provenance.json' in names:
                    if names.count('source-provenance.json') != 1:
                        raise ValueError('artifact source provenance must occur exactly once')
                    record = json.loads(archive.read('source-provenance.json'))
                    checkout = artifact_source(record, run, commit, source_tree, package.name,
                                               hashlib.sha256(package.read_bytes()).hexdigest())
                else:
                    # Historical artifacts predate the bundled source record. Their
                    # completed producer logs remain an independent exact-source path.
                    checkout = checkout_source(run_id, commit, source_tree)
        staged = folder / "cyrus"
        extract_binary(package, staged, f"run {run_id} head {commit}")
        with staged.open("rb") as source:
            executable_hash = hashlib.file_digest(source, "sha256").hexdigest()
        staged.replace(destination)
    print(f"Replay artifact: workflow head {commit}; https://github.com/{REPOSITORY}/actions/runs/{run_id}", flush=True)
    return dict(commit=commit, run=run_id, artifact=artifact["id"],
                archive_sha256=archive_hash, executable_sha256=executable_hash, **checkout)


def download(commit, destination, fallback_main=False, run_id=None, source_tree=None):
    if run_id is not None:
        if fallback_main:
            raise ValueError("explicit run cannot use main ancestor fallback")
        return download_run(commit, destination, run_id, source_tree)
    if source_tree is not None:
        raise ValueError("checkout-tree binding requires an explicit run")
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
        staged = Path(temporary) / "cyrus"
        extract_binary(package, staged, f"main {commit}")
        staged.replace(destination)
    print(f"Replay artifact: main {selected_commit}; https://github.com/{REPOSITORY}/actions/runs/{run}", flush=True)
    return dict(commit=selected_commit, run=run)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("commit")
    parser.add_argument("destination", type=Path)
    selection = parser.add_mutually_exclusive_group()
    selection.add_argument("--fallback-main", action="store_true", help="select the newest ancestor main run with an uploaded Linux artifact")
    selection.add_argument("--run", type=int, help="download from this artifact workflow run only, requiring the exact requested head")
    parser.add_argument("--source-tree", help="require this exact checkout tree, validated from the native job checkout record")
    parser.add_argument("--receipt", type=Path, help="write the selected binary commit and workflow run")
    args = parser.parse_args()
    try:
        receipt = download(args.commit, args.destination, args.fallback_main, args.run, args.source_tree)
        if args.receipt:
            args.receipt.write_text(json.dumps(receipt) + "\n")
    except ArtifactPending as error:
        parser.exit(75, f"replay artifact pending: {error}\n")
    except (OSError, ValueError, tarfile.TarError, zipfile.BadZipFile, subprocess.CalledProcessError) as error:
        label = "replay" if args.run is not None else "main replay"
        parser.exit(1, f"{label} artifact unavailable: {error}\n")
