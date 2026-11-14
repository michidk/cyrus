#!/usr/bin/env python3
"""Retain completed Cargo units while an application build is still in progress.

Cargo remains responsible for dependency freshness after restoration. Only files
reported by completed compilation units, their dep-info and Cargo fingerprints,
and completed build-script output are retained. No profile/cache sweep is used.
"""
import argparse
import importlib.util
import json
import os
import re
from pathlib import Path
import shutil
import sys
import tarfile
import tempfile

spec = importlib.util.spec_from_file_location('wasm_checkpoint', Path(__file__).with_name('web-build-checkpoint.py'))
checkpoint = importlib.util.module_from_spec(spec)
spec.loader.exec_module(checkpoint)


def compiler_identity(expected):
    """Build orchestration is not a rustc input; retain compatibility with old units."""
    source = expected['source']
    if isinstance(source, dict) and 'files' in source:
        source = {**{k: v for k, v in source.items() if k not in ('files', 'sha256')},
                  'files': {k: v for k, v in source['files'].items() if not k.startswith(('dev/scripts/', 'dev/web/'))}}
    return {**expected, 'source': source}


def pack(unit):
    """Compress only this task's completed units, preserving compiler timestamps."""
    if (unit / 'files.tar.gz').exists():
        return
    with tempfile.NamedTemporaryFile(dir=unit, delete=False) as temporary:
        staged = Path(temporary.name)
    try:
        with tarfile.open(staged, 'w:gz', compresslevel=1) as archive:
            for path in sorted((unit / 'files').rglob('*')):
                if path.is_file():
                    archive.add(path, arcname=str(path.relative_to(unit / 'files')), recursive=False)
        os.replace(staged, unit / 'files.tar.gz')
        shutil.rmtree(unit / 'files')
    finally:
        staged.unlink(missing_ok=True)


def completed_files(event, target):
    files = set()
    if event.get('reason') == 'compiler-artifact':
        for name in event.get('filenames', []):
            path = Path(name)
            files.add(path)
            if path.parent.name != 'deps' and path.parent.parent.name != 'build':
                package = event.get('package_id', '').rsplit('#', 1)[-1].split('@', 1)[0]
                if package:
                    files.update(path.parent.glob('.fingerprint/' + package + '-*/**/*'))
            if path.parent.parent.name == 'build':
                files.update(path.parent.glob('*'))
                files.update((path.parent.parent.parent / '.fingerprint' / path.parent.name).glob('**/*'))
            stem = path.stem.removeprefix('lib')
            files.add(path.parent / (stem + '.d'))
            if '-' in stem:
                unit = stem.rsplit('-', 1)[1]
                fingerprint = path.parent.parent / '.fingerprint' / (event['target']['name'].replace('_', '-') + '-' + unit)
                # Package names and target names may differ. Match Cargo's hash.
                files.update(path.parent.parent.glob('.fingerprint/*-' + unit + '/**/*'))
                files.update(fingerprint.glob('**/*'))
    elif event.get('reason') == 'build-script-executed' and event.get('out_dir'):
        build = Path(event['out_dir']).parent
        files.update(build.glob('**/*'))
        files.update((build.parent.parent / '.fingerprint' / build.name).glob('**/*'))
    return sorted(path for path in files if path.is_file() and not path.is_symlink()
                  and path.is_relative_to(target))


def retain(directory, target, event, expected):
    files = completed_files(event, target)
    if not files:
        return
    units = directory / 'units'
    units.mkdir(parents=True, exist_ok=True)
    key = checkpoint.hashlib.sha256(json.dumps({'files': [str(p.relative_to(target)) for p in files], 'identity': compiler_identity(expected)}, sort_keys=True).encode()).hexdigest()
    with tempfile.TemporaryDirectory(dir=units) as temporary:
        staged = Path(temporary)
        entries = {}
        for path in files:
            relative = path.relative_to(target)
            output = staged / 'files' / relative
            output.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(path, output)
            entries[str(relative)] = checkpoint.digest(output)
        (staged / 'record.json').write_text(json.dumps({'identity': compiler_identity(expected), 'files': entries}))
        pack(staged)
        # A complete existing immutable unit needs no rewrite. Different bytes
        # are published under a new content identity, never over an old unit.
        key += '-' + checkpoint.hashlib.sha256(json.dumps(entries, sort_keys=True).encode()).hexdigest()
        destination = units / key
        if not destination.exists():
            os.rename(staged, destination)


def restore(directory, target, expected):
    restored = 0
    for unit in sorted((directory / 'units').glob('*')):
        if not re.fullmatch(r'[0-9a-f]{64}-[0-9a-f]{64}', unit.name):
            continue  # An interrupted staging directory is never a completion.
        record = json.loads((unit / 'record.json').read_text())
        if compiler_identity(record['identity']) != compiler_identity(expected):
            continue
        pack(unit)
        # Gzip random access repeatedly decompresses large compiler libraries.
        # Validate and stage the entire unit in one forward pass before linking
        # any member into the shared target. Staging shares the target filesystem.
        target.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(dir=target) as temporary:
            staged_root = Path(temporary)
            staged_files = []
            seen = set()
            with tarfile.open(unit / 'files.tar.gz', 'r|gz') as archive:
                for member in archive:
                    name = member.name
                    if name not in record['files']:
                        continue
                    relative = Path(name)
                    if relative.is_absolute() or '..' in relative.parts:
                        raise ValueError('unsafe compilation checkpoint path')
                    if name in seen or not member.isfile():
                        raise ValueError('invalid compilation checkpoint member: ' + name)
                    seen.add(name)
                    output = target / relative
                    staged = staged_root / relative
                    write = not output.exists()
                    if write:
                        staged.parent.mkdir(parents=True, exist_ok=True)
                    digest = checkpoint.hashlib.sha256()
                    with archive.extractfile(member) as source:
                        if write:
                            with staged.open('wb') as destination:
                                while block := source.read(1024 * 1024):
                                    digest.update(block)
                                    destination.write(block)
                        else:
                            while block := source.read(1024 * 1024):
                                digest.update(block)
                    if digest.hexdigest() != record['files'][name]:
                        raise ValueError('corrupt compilation checkpoint: ' + name)
                    if write:
                        os.chmod(staged, member.mode)
                        os.utime(staged, (member.mtime, member.mtime))
                        staged_files.append((staged, output))
            if seen != record['files'].keys():
                raise ValueError('incomplete compilation checkpoint')
            for staged, output in staged_files:
                output.parent.mkdir(parents=True, exist_ok=True)
                try:
                    os.link(staged, output)
                except FileExistsError:
                    continue  # Another shared-target owner published first.
                restored += 1
    return restored


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('operation', choices=('restore', 'record'))
    parser.add_argument('directory', type=Path)
    parser.add_argument('snapshot', type=Path)
    parser.add_argument('configuration')
    parser.add_argument('target', type=Path)
    args = parser.parse_args()
    expected = checkpoint.identity(args.snapshot, args.configuration)
    if args.operation == 'restore':
        print(f'restored {restore(args.directory, args.target, expected)} completed compilation files', flush=True)
    else:
        for line in sys.stdin:
            try:
                event = json.loads(line)
            except json.JSONDecodeError:
                print(line, end='', flush=True)
                continue
            if event.get('reason') == 'compiler-message':
                print(event.get('message', {}).get('rendered', ''), end='', flush=True)
            else:
                retain(args.directory, args.target, event, expected)
                if event.get('reason') == 'compiler-artifact':
                    print('checkpointed ' + event['target']['name'], flush=True)


if __name__ == '__main__':
    main()
