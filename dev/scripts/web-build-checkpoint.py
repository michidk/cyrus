#!/usr/bin/env python3
"""Retain completed raw WASM for restartable browser profiling builds."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import tempfile


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def identity(snapshot, configuration):
    source = json.loads(snapshot.read_text())
    return {'source': {key: value for key, value in source.items() if key != 'revision'},
            'configuration': configuration}


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
    if {key: record.get(key) for key in expected} != expected:
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
