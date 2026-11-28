#!/usr/bin/env python3
"""Retain completed Cargo units while an application build is still in progress.

Cargo remains responsible for dependency freshness after restoration. Only files
reported by completed compilation units, their dep-info and Cargo fingerprints,
and completed build-script output are retained. No profile/cache sweep is used.
"""
import argparse
from concurrent.futures import ThreadPoolExecutor
import gzip
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
    """Match compiler inputs by content, independent of commit and orchestration."""
    source = expected['source']
    if isinstance(source, dict) and 'files' in source:
        source = {**{k: v for k, v in source.items() if k not in ('files', 'sha256', 'revision')},
                  'files': {k: v for k, v in source['files'].items() if not k.startswith(('dev/scripts/', 'dev/web/'))}}
    return {**expected, 'source': source}


class HashingReader:
    """Hash the exact bytes consumed by the archive, without a second copy."""

    def __init__(self, source):
        self.source = source
        self.digest = checkpoint.hashlib.sha256()

    def read(self, size):
        block = self.source.read(size)
        self.digest.update(block)
        return block


def write_archive(destination, root, entries):
    """Stream completed files and reject a unit that changed during capture."""
    with destination.open('wb') as output:
        with gzip.GzipFile(fileobj=output, mode='wb', compresslevel=1) as compressed, \
                tarfile.open(fileobj=compressed, mode='w|') as archive:
            for name, digest in entries.items():
                with (root / name).open('rb') as source:
                    member = archive.gettarinfo(fileobj=source, arcname=name)
                    if not member.isfile():
                        raise ValueError('invalid compilation output: ' + name)
                    reader = HashingReader(source)
                    archive.addfile(member, reader)
                    if reader.digest.hexdigest() != digest:
                        raise ValueError('compilation output changed during checkpoint: ' + name)
        output.flush()
        os.fsync(output.fileno())


def pack(unit):
    """Migrate an older unpacked unit without changing it if compression fails."""
    if (unit / 'files.tar.gz').exists():
        return
    with tempfile.NamedTemporaryFile(dir=unit, delete=False) as temporary:
        staged = Path(temporary.name)
    try:
        root = unit / 'files'
        entries = {str(path.relative_to(root)): checkpoint.digest(path)
                   for path in sorted(root.rglob('*')) if path.is_file() and not path.is_symlink()}
        write_archive(staged, root, entries)
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
    # Cargo emits fresh dependencies again after restoration. Detect their exact
    # immutable checkpoint before copying and compressing them, so replay cannot
    # delay recording newly completed units until after another interruption.
    current_entries = {str(path.relative_to(target)): checkpoint.digest(path) for path in files}
    current_key = key + '-' + checkpoint.hashlib.sha256(json.dumps(current_entries, sort_keys=True).encode()).hexdigest()
    if (units / current_key).exists():
        return
    with tempfile.TemporaryDirectory(dir=units, prefix='.staging-') as temporary:
        staged = Path(temporary)
        # The compiler outputs are already complete. Archive them directly;
        # staging raw files would retain a full unit beside its archive.
        write_archive(staged / 'files.tar.gz', target, current_entries)
        # A failed/unfinished compressor never gets a completion receipt.
        with (staged / 'record.json').open('w') as record:
            json.dump({'identity': compiler_identity(expected), 'files': current_entries}, record)
            record.flush()
            os.fsync(record.fileno())
        # A complete existing immutable unit needs no rewrite. Different bytes
        # are published under a new content identity, never over an old unit.
        destination = units / current_key
        if not destination.exists():
            os.rename(staged, destination)


def restore_unit(unit, target, expected, retain_unpacked):
    """Validate a complete unit before publishing any of its missing files."""
    restored = 0
    record = json.loads((unit / 'record.json').read_text())
    if compiler_identity(record['identity']) != compiler_identity(expected):
        return 0
    pack(unit)
    if retain_unpacked:
        unpacked = unit / 'unpacked'
        if not unpacked.exists():
            # Publish only a whole validated unit. A reset leaves an ignored
            # temporary directory, never a reusable partial expansion.
            with tempfile.TemporaryDirectory(dir=unit) as temporary:
                view = Path(temporary)
                (view / 'units').mkdir()
                (view / 'units' / unit.name).symlink_to(unit.resolve(), target_is_directory=True)
                restore(view, view / 'files', expected, retain_unpacked=False)
                os.rename(view / 'files', unpacked)
        target.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(dir=target) as temporary:
            staged_root = Path(temporary)
            pending = []
            for name, digest in record['files'].items():
                relative = Path(name)
                if relative.is_absolute() or '..' in relative.parts:
                    raise ValueError('unsafe compilation checkpoint path')
                source = unpacked / relative
                if source.is_symlink() or not source.is_file() or checkpoint.digest(source) != digest:
                    raise ValueError('corrupt compilation checkpoint: ' + name)
                output = target / relative
                if not output.exists():
                    staged = staged_root / relative
                    staged.parent.mkdir(parents=True, exist_ok=True)
                    shutil.copy2(source, staged)
                    pending.append((staged, output))
            for staged, output in pending:
                output.parent.mkdir(parents=True, exist_ok=True)
                try:
                    os.link(staged, output)
                except FileExistsError:
                    continue
                restored += 1
        return restored
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


def restore(directory, target, expected, retain_unpacked=None):
    """Restore validated units concurrently within the caller's hauler lane.

    CYRUS_COMPILE_RETAIN_UNPACKED=1 retains validated expansion across target
    resets. Retained bytes are revalidated and live target files are preserved.
    """
    if retain_unpacked is None:
        retain_unpacked = os.environ.get('CYRUS_COMPILE_RETAIN_UNPACKED') == '1'
    units = [unit for unit in sorted((directory / 'units').glob('*'))
             if re.fullmatch(r'[0-9a-f]{64}-[0-9a-f]{64}', unit.name)]
    with ThreadPoolExecutor(max_workers=4) as workers:
        return sum(workers.map(lambda unit: restore_unit(unit, target, expected, retain_unpacked), units))


def restore_executable(directory, output, expected):
    """Promote a completed native unit when reset interrupted the final build seal."""
    if output.name != 'cyrus':
        raise ValueError('native executable output must be named cyrus')
    relative = 'debug/cyrus'
    candidates = []
    for unit in sorted((directory / 'units').glob('*')):
        if not re.fullmatch(r'[0-9a-f]{64}-[0-9a-f]{64}', unit.name):
            continue
        record = json.loads((unit / 'record.json').read_text())
        if (compiler_identity(record['identity']) == compiler_identity(expected)
                and relative in record['files']):
            candidates.append((unit, record['files'][relative]))
    if not candidates:
        return False
    if len({digest for _, digest in candidates}) != 1:
        raise ValueError('ambiguous completed native executable checkpoints')
    unit, digest = candidates[0]
    pack(unit)
    with tempfile.TemporaryDirectory(dir=directory) as temporary:
        staged = Path(temporary) / 'cyrus'
        with tarfile.open(unit / 'files.tar.gz', 'r|gz') as archive:
            for member in archive:
                if member.name != relative:
                    continue
                if staged.exists() or not member.isfile() or not member.mode & 0o111:
                    raise ValueError('invalid completed native executable member')
                with archive.extractfile(member) as source, staged.open('wb') as destination:
                    shutil.copyfileobj(source, destination)
                os.chmod(staged, member.mode)
        if not staged.exists() or checkpoint.digest(staged) != digest:
            raise ValueError('corrupt completed native executable checkpoint')
        checkpoint.store(directory, 'cyrus', staged, expected)
    return checkpoint.restore(directory, 'cyrus', output, expected)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('operation', choices=('restore', 'record', 'restore-executable'))
    parser.add_argument('directory', type=Path)
    parser.add_argument('snapshot', type=Path)
    parser.add_argument('configuration')
    parser.add_argument('target', type=Path)
    args = parser.parse_args()
    expected = checkpoint.identity(args.snapshot, args.configuration)
    if args.operation == 'restore-executable':
        if not restore_executable(args.directory, args.target, expected):
            raise SystemExit(3)
    elif args.operation == 'restore':
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
