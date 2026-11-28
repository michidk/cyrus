#!/usr/bin/env python3
"""Export and admit source-bound Cargo workspace test executables without game data."""
import argparse
import gzip
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import platform
import re
import shutil
import subprocess
import tempfile


SCOPE = 'cargo test --locked --workspace --all-features --no-run'


def relative_file(name):
    path = PurePosixPath(name)
    if path.is_absolute() or '..' in path.parts or str(path) != name or name == '.':
        raise ValueError(f'unsafe bundle path: {name}')
    return path


def copy_payload(source, destination):
    """Stream one executable into compressed storage without a raw staging copy."""
    digest = hashlib.sha256()
    size = 0
    with source.open('rb') as incoming, gzip.open(destination, 'wb', compresslevel=1) as outgoing:
        for block in iter(lambda: incoming.read(1024 * 1024), b''):
            digest.update(block)
            size += len(block)
            outgoing.write(block)
    return digest.hexdigest(), size


def expected_targets(metadata):
    members = set(metadata['workspace_members'])
    return {(package['id'], target['name'], tuple(target['kind']))
            for package in metadata['packages'] if package['id'] in members
            for target in package['targets'] if target.get('test')
            and set(target['kind']) & {'lib', 'rlib', 'bin', 'test', 'example', 'cdylib', 'dylib', 'staticlib'}}


def elf_requirements(path):
    """Retain loader requirements; consumers still preflight in their actual runtime."""
    with path.open('rb') as stream:
        if stream.read(4) != b'\x7fELF':
            return None
    dynamic = subprocess.check_output(['readelf', '--wide', '--dynamic', str(path)], text=True)
    versions = subprocess.check_output(['readelf', '--wide', '--version-info', str(path)], text=True)
    return dict(needed=sorted(set(re.findall(r'Shared library: \[(.*?)\]', dynamic))),
                versions=sorted(set(re.findall(r'Name: (\S+)', versions))))


def collect(messages, metadata, destination, identity):
    """Require complete successful no-run output, then publish a bundle atomically."""
    destination = Path(destination)
    if destination.exists():
        raise FileExistsError(destination)
    target_root = Path(metadata['target_directory'])
    if not target_root.is_absolute() or not Path(metadata['workspace_root']).is_absolute():
        raise ValueError('Cargo roots must be absolute')
    packages = {p['id']: p['name'] for p in metadata['packages']}
    members = set(metadata['workspace_members'])
    found, companions, files = set(), set(), {}
    finished = False
    for message in messages:
        if message.get('reason') == 'build-finished':
            if not message.get('success'):
                raise ValueError('Cargo build failed')
            finished = True
        if message.get('reason') != 'compiler-artifact':
            continue
        target = message['target']
        package = message['package_id']
        executable = message.get('executable')
        is_test = bool(message['profile'].get('test'))
        if executable and is_test and package in members:
            found.add((package, target['name'], tuple(target['kind'])))
        if executable and not is_test and package in members:
            companions.add((package, target['name']))
        paths = [(executable, 'test' if is_test else 'companion')] if (
            executable and package in members and (is_test or 'bin' in target['kind'])
        ) else []
        if 'proc-macro' not in target['kind']:
            paths += [(name, 'runtime') for name in message.get('filenames', [])
                      if Path(name).suffix in {'.so', '.dylib', '.dll'}]
        for name, role in paths:
            path = Path(name)
            relative = str(path.relative_to(target_root))
            relative_file(relative)
            if path.is_symlink() or not path.is_file() or path.resolve() != target_root.resolve() / relative:
                raise ValueError(f'not a regular target file: {path}')
            if role != 'runtime' and not path.stat().st_mode & 0o111:
                raise ValueError(f'not executable: {path}')
            record = dict(path=relative, role=role, package=packages.get(package, package),
                          target=target['name'], kind=target['kind'],
                          features=message.get('features', []), profile=message['profile'])
            if relative in files and files[relative] != record:
                raise ValueError(f'conflicting Cargo artifacts: {relative}')
            files[relative] = record
    required_companions = {(p['id'], t['name']) for p in metadata['packages']
                           if p['id'] in members and any('test' in t['kind'] for t in p['targets'])
                           for t in p['targets'] if 'bin' in t['kind']}
    if required_companions - companions:
        raise ValueError(f'missing integration-test binary companions: {sorted(required_companions - companions)}')
    missing = expected_targets(metadata) - found
    if not finished or missing or not any(f['role'] == 'test' for f in files.values()):
        raise ValueError(f'incomplete Cargo no-run output; missing targets: {sorted(missing)}')
    destination.parent.mkdir(parents=True, exist_ok=True)
    stage = Path(tempfile.mkdtemp(prefix='.' + destination.name + '-', dir=destination.parent))
    try:
        (stage / 'payload').mkdir()
        for index, (relative, record) in enumerate(sorted(files.items())):
            record['payload'] = f'payload/{index}.gz'
            record['sha256'], record['size'] = copy_payload(target_root / relative, stage / record['payload'])
            record['elf'] = elf_requirements(target_root / relative)
            record['mode'] = (target_root / relative).stat().st_mode & 0o777
        manifest = dict(version=1, identity=identity, scope=SCOPE,
                        workspace_root=metadata['workspace_root'], target_directory=str(target_root),
                        files=list(files.values()))
        (stage / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
        stage.rename(destination)
        return manifest
    finally:
        if stage.exists():
            shutil.rmtree(stage)


def admit(directory, commit, tree, system, machine):
    """Verify every payload before any executable is extracted or launched.

    Callers must first authenticate the downloaded artifact digest and CI run.
    Architecture/OS admission does not establish libc or shared-library compatibility;
    run the dynamic loader and libtest --list in the selected runtime before tests.
    """
    directory = Path(directory)
    manifest_path = directory / 'manifest.json'
    if manifest_path.is_symlink():
        raise ValueError('manifest must be a regular file')
    manifest = json.loads(manifest_path.read_text())
    identity = manifest['identity']
    if (manifest['version'] != 1 or manifest['scope'] != SCOPE
            or any(identity[key] != value for key, value in
                   [('commit', commit), ('tree', tree), ('system', system), ('machine', machine)])):
        raise ValueError('bundle source, scope or platform does not match')
    if not manifest['files'] or not any(f['role'] == 'test' for f in manifest['files']):
        raise ValueError('bundle has no tests')
    seen = set()
    for entry in manifest['files']:
        relative_file(entry['path'])
        relative_file(entry['payload'])
        if entry['path'] in seen or entry['role'] not in {'test', 'companion', 'runtime'}:
            raise ValueError('duplicate path or unknown role')
        seen.add(entry['path'])
        payload = directory / entry['payload']
        if payload.is_symlink() or not payload.is_file() or payload.resolve() != directory.resolve() / entry['payload']:
            raise ValueError('payload is not a regular bundle file')
        digest = hashlib.sha256()
        size = 0
        with gzip.open(payload, 'rb') as stream:
            for block in iter(lambda: stream.read(1024 * 1024), b''):
                size += len(block)
                if size > entry['size']:
                    raise ValueError('payload exceeds declared size')
                digest.update(block)
        if size != entry['size'] or digest.hexdigest() != entry['sha256']:
            raise ValueError(f'payload hash mismatch: {entry["path"]}')
    return manifest


def extract(directory, manifest, destination, package=None, test=None):
    """Extract admitted tests for one package and all companions to a fresh root."""
    if not any(f['role'] == 'test' and (test is None or f['path'] == test)
               and (package is None or f['package'] == package) for f in manifest['files']):
        raise ValueError('requested tests are absent from the admitted bundle')
    destination = Path(destination)
    if destination.exists():
        raise FileExistsError(destination)
    destination.mkdir(parents=True)
    try:
        for entry in manifest['files']:
            if entry['role'] == 'test' and ((package and entry['package'] != package)
                                          or (test and entry['path'] != test)):
                continue
            path = destination / entry['path']
            path.parent.mkdir(parents=True, exist_ok=True)
            with gzip.open(Path(directory) / entry['payload'], 'rb') as source, path.open('xb') as output:
                shutil.copyfileobj(source, output)
            path.chmod(entry['mode'] & 0o777)
    except BaseException:
        shutil.rmtree(destination)
        raise


def require_installed_output(output):
    """Reject successful libtest exits that did not exercise required local inputs."""
    skipped = re.search(r"\b(?:skipped:|SKIP\b)[^\n]*", output, re.IGNORECASE)
    if skipped:
        raise ValueError(f"installed test skipped required input: {skipped.group()}")


def runtime_install_mounts(install, workspace_root):
    """Preserve the local Files catalog's install and sibling media layout.

    These are read-only runtime mounts, never artifact payloads. Resolve the
    checkout's install link before finding its sibling GOG/CD-edition media.
    """
    install = Path(install).resolve()
    workspace = PurePosixPath(workspace_root)
    if not install.is_dir():
        raise ValueError(f'installed game directory is missing: {install}')
    if not workspace.is_absolute() or '..' in workspace.parts:
        raise ValueError('producer workspace must be an absolute normalized path')
    paths = [(install, workspace / 'redguard')]
    for name, directory in [('game.gog', False), ('game.ins', False), ('CDROM', True),
                            ('Soundtrack', True), ('DOSBOX', True)]:
        path = install.parent / name
        present = path.is_dir() if directory else path.is_file()
        if present:
            paths.append((path.resolve(), workspace / name))
    return [dict(source=str(source), destination=str(destination), read_only=True)
            for source, destination in paths]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest='command', required=True)
    export = commands.add_parser('collect')
    export.add_argument('messages', type=Path)
    export.add_argument('metadata', type=Path)
    export.add_argument('destination', type=Path)
    export.add_argument('commit')
    admission = commands.add_parser('admit')
    admission.add_argument('directory', type=Path)
    admission.add_argument('commit')
    admission.add_argument('tree')
    admission.add_argument('--extract', type=Path)
    admission.add_argument('--package')
    admission.add_argument('--test', help='extract only this admitted test path plus companions')
    mounts = commands.add_parser('runtime-mounts', help='plan read-only local install/media mounts')
    mounts.add_argument('install', type=Path)
    mounts.add_argument('workspace_root')
    output = commands.add_parser('installed-output', help='reject missing-input skips in a test log')
    output.add_argument('log', type=Path)
    args = parser.parse_args()
    if args.command == 'installed-output':
        require_installed_output(args.log.read_text(errors='replace'))
        return
    if args.command == 'runtime-mounts':
        print(json.dumps(runtime_install_mounts(args.install, args.workspace_root), indent=2))
        return
    if args.command == 'collect':
        def git(*arguments):
            return subprocess.check_output(['git', *arguments], text=True).strip()
        subprocess.run(['git', 'diff', '--quiet', 'HEAD'], check=True)
        commit = git('rev-parse', 'HEAD')
        if args.commit != commit:
            raise ValueError('requested commit does not match checkout')
        identity = dict(commit=commit, tree=git('rev-parse', 'HEAD^{tree}'),
                        repository=os.environ['GITHUB_REPOSITORY'], run=os.environ['GITHUB_RUN_ID'],
                        attempt=os.environ['GITHUB_RUN_ATTEMPT'], system=platform.system(),
                        machine=platform.machine(), libc=platform.libc_ver(),
                        os_release=platform.freedesktop_os_release(),
                        runner_image={key: os.environ.get(key) for key in
                                      ['ImageOS', 'ImageVersion', 'RUNNER_OS', 'RUNNER_ARCH']},
                        rustc=subprocess.check_output(['rustc', '-vV'], text=True))
        messages = [json.loads(line) for line in args.messages.read_text().splitlines()]
        collect(messages, json.loads(args.metadata.read_text()), args.destination, identity)
    else:
        manifest = admit(args.directory, args.commit, args.tree, platform.system(), platform.machine())
        if args.extract:
            extract(args.directory, manifest, args.extract, args.package, args.test)
        print(json.dumps(manifest, indent=2))


if __name__ == '__main__':
    main()
