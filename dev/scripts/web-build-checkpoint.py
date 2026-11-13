#!/usr/bin/env python3
"""Retain completed raw WASM for restartable browser profiling builds."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import tempfile
import tomllib


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def identity(snapshot, configuration):
    source = json.loads(snapshot.read_text())
    return {'source': {key: value for key, value in source.items() if key != 'revision'},
            'configuration': configuration}


def compiler_source(source, name):
    """The worker is compiled alone; application-only changes cannot affect it.

    Retain every manifest and build script conservatively, along with workspace
    configuration, lockfile, vendored code and the local dependency closure.
    Runtime/build orchestration scripts do not enter the standalone worker WASM.
    Historical checkpoints retain their full acquisition snapshot on disk.
    """
    if name != 'cyrus_assets_worker.wasm' or 'files' not in source:
        return source
    root = Path.cwd()
    manifests = {}
    for path in (root / 'crates').glob('*/Cargo.toml'):
        manifest = tomllib.loads(path.read_text())
        if 'package' in manifest:
            manifests[manifest['package']['name']] = (path.parent.relative_to(root).as_posix(), manifest)
    if 'cyrus-assets-worker' not in manifests:
        return source

    workspace_manifest = root / "Cargo.toml"
    workspace = (tomllib.loads(workspace_manifest.read_text()).get("workspace", {}).get("dependencies", {})
                 if workspace_manifest.exists() else {})

    def dependencies(table):
        for key, value in table.items():
            if key in ('dependencies', 'build-dependencies', 'dev-dependencies'):
                for alias, dependency in value.items():
                    if isinstance(dependency, dict) and dependency.get('workspace'):
                        dependency = workspace.get(alias, dependency)
                    yield dependency.get('package', alias) if isinstance(dependency, dict) else alias
            elif isinstance(value, dict):
                yield from dependencies(value)

    pending, included = ['cyrus-assets-worker'], set()
    while pending:
        package = pending.pop()
        if package in included or package not in manifests:
            continue
        included.add(package)
        pending.extend(dependencies(manifests[package][1]))
    prefixes = tuple(manifests[package][0] + '/' for package in included)
    files = {path: digest for path, digest in source['files'].items()
             if not path.startswith('dev/scripts/') and
             (not path.startswith('crates/') or path.endswith(('/Cargo.toml', '/build.rs'))
              or path.startswith(prefixes))}
    return {**{key: value for key, value in source.items() if key not in ('files', 'sha256')}, 'files': files}


def store(directory, name, artifact, expected):
    directory.mkdir(parents=True, exist_ok=True)
    # Publish the manifest last. An interrupted copy never becomes reusable.
    with tempfile.TemporaryDirectory(dir=directory) as temporary:
        staged = Path(temporary) / name
        shutil.copyfile(artifact, staged)
        record = {**expected, 'sha256': digest(staged)}
        manifest = Path(temporary) / 'record.json'
        manifest.write_text(json.dumps(record, sort_keys=True) + '\n')
        os.replace(staged, directory / name)
        os.replace(manifest, directory / (name + '.json'))


def restore(directory, name, artifact, expected):
    manifest = directory / (name + '.json')
    retained = directory / name
    if not manifest.exists():
        return False
    record = json.loads(manifest.read_text())
    actual = {key: record.get(key) for key in expected}
    wanted = dict(expected)
    actual['source'] = compiler_source(actual['source'], name)
    wanted['source'] = compiler_source(wanted['source'], name)
    if actual != wanted:
        return False
    if not retained.is_file() or digest(retained) != record['sha256']:
        raise ValueError(f'corrupt WASM checkpoint: {retained}')
    artifact.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(dir=artifact.parent, delete=False) as temporary:
        staged = Path(temporary.name)
    try:
        shutil.copyfile(retained, staged)
        os.replace(staged, artifact)
    finally:
        staged.unlink(missing_ok=True)
    return True


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('operation', choices=('store', 'restore'))
    parser.add_argument('directory', type=Path)
    parser.add_argument('snapshot', type=Path)
    parser.add_argument('configuration')
    parser.add_argument('artifact', type=Path)
    args = parser.parse_args()
    expected = identity(args.snapshot, args.configuration)
    if args.operation == 'store':
        store(args.directory, args.artifact.name, args.artifact, expected)
    elif not restore(args.directory, args.artifact.name, args.artifact, expected):
        raise SystemExit(3)


if __name__ == '__main__':
    main()
