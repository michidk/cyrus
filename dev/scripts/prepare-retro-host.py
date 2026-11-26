#!/usr/bin/env python3
"""Restore selected host runtime dependencies from a retained Docker archive.

No Docker daemon, chroot or ptrace workaround participates. Pass exact library
basenames (or dosbox-x); symlink targets are recovered inside the owned output
root. Use the recovered dynamic loader with an explicit library search path,
not LD_LIBRARY_PATH on unrelated host programs. This prepares bytes, not proof
of original-game execution or observer capability.
"""
import argparse
import json
import os
from pathlib import Path, PurePosixPath
import posixpath
import shutil
import tarfile

from harness import exclusive_lock
from native_session_capture import atomic, file_digest


def digest(path):
    if path.is_symlink():
        return dict(link=os.readlink(path))
    return dict(sha256=file_digest(path), mode=path.stat().st_mode & 0o777)


def safe_name(name):
    path = PurePosixPath(name)
    if path.is_absolute() or '..' in path.parts or not path.parts:
        raise ValueError(f'unsafe runtime member: {name}')
    if not str(path).startswith(('usr/lib/', 'usr/lib64/', 'lib/', 'lib64/', 'usr/bin/')):
        raise ValueError(f'not a runtime dependency: {name}')
    return str(path)


def destination(root, name):
    target = root / safe_name(name)
    if not target.parent.resolve().is_relative_to(root.resolve()):
        raise ValueError(f'runtime parent escapes output: {name}')
    return target


def prepare(source, root, names):
    if not names or any('/' in name or name in ('.', '..') for name in names):
        raise ValueError('request exact runtime basenames')
    root.mkdir(parents=True, exist_ok=True)
    binding = dict(archive_sha256=file_digest(source), tool_sha256=file_digest(Path(__file__)),
                   names=sorted(set(names)))
    receipt = root / 'host-preparation.json'
    with exclusive_lock(root / 'host-preparation.lock'):
        try:
            old = json.loads(receipt.read_text())
            if old['binding'] == binding and all(
                    digest(destination(root, name)) == value for name, value in old['files'].items()):
                return old
        except (OSError, ValueError, KeyError, TypeError):
            pass
        # A failed refresh must never leave a success receipt for the new request.
        receipt.unlink(missing_ok=True)
        pending, found, files = set(names), set(), {}
        with tarfile.open(source) as outer:
            manifests = json.load(outer.extractfile('manifest.json'))
            if len(manifests) != 1:
                raise ValueError('host recovery requires a single-image retained archive')
            while pending:
                requested, pending = pending, set()
                matched = set()
                for layer_name in manifests[0]['Layers']:
                    with tarfile.open(fileobj=outer.extractfile(layer_name), mode='r|*') as layer:
                        for member in layer:
                            if PurePosixPath(member.name).name not in requested and member.name not in requested:
                                continue
                            name = safe_name(member.name)
                            matched.update({name, PurePosixPath(name).name} & requested)
                            target = destination(root, name)
                            target.parent.mkdir(parents=True, exist_ok=True)
                            if member.issym():
                                resolved = posixpath.normpath(posixpath.join(posixpath.dirname(name), member.linkname)
                                    if not member.linkname.startswith('/') else member.linkname.lstrip('/'))
                                safe_name(resolved)
                                relative = os.path.relpath(root / resolved, target.parent)
                                if not target.is_symlink() or os.readlink(target) != relative:
                                    target.unlink(missing_ok=True)
                                    target.symlink_to(relative)
                                pending.add(resolved)
                            elif member.isfile():
                                temporary = target.with_name(target.name + '.partial')
                                temporary.unlink(missing_ok=True)
                                with layer.extractfile(member) as src, temporary.open('xb') as dst:
                                    shutil.copyfileobj(src, dst)
                                    dst.flush()
                                    os.fsync(dst.fileno())
                                temporary.chmod(member.mode & 0o777)
                                if target.is_file() and not target.is_symlink() and digest(target) == digest(temporary):
                                    temporary.unlink()
                                else:
                                    temporary.replace(target)
                            else:
                                raise ValueError(f'unsupported runtime entry: {name}')
                            files[name] = digest(target)
                            found.add(name)
                missing = requested - matched
                if missing:
                    raise ValueError(f'retained archive missing dependencies: {sorted(missing)}')
                pending -= found
        for name in files:
            target = destination(root, name)
            try:
                resolved = target.resolve(strict=True)
            except (OSError, RuntimeError) as error:
                raise ValueError(f'unresolved runtime dependency: {name}') from error
            if not resolved.is_relative_to(root.resolve()) or not resolved.is_file():
                raise ValueError(f'invalid runtime dependency target: {name}')
        record = dict(binding=binding, files=files)
        atomic(receipt, (json.dumps(record, sort_keys=True) + '\n').encode())
        return record


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('archive', type=Path)
    parser.add_argument('root', type=Path)
    parser.add_argument('names', nargs='+')
    args = parser.parse_args()
    try:
        print(json.dumps(prepare(args.archive, args.root, args.names), sort_keys=True))
    except (OSError, ValueError, KeyError, tarfile.TarError) as error:
        parser.exit(1, f'host runtime preparation: {error}\n')


if __name__ == '__main__':
    main()
