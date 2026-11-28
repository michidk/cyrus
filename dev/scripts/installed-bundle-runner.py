#!/usr/bin/env python3
"""Run authenticated installed test projections with durable per-harness receipts.

The original manifest remains immutable. Only source-bound raw payloads enter the
cache; task-owned hardlinks are mounted read-only into each fresh target tree.
"""
import argparse
import fcntl
import gzip
import hashlib
import io
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import subprocess
import sys
import tempfile
import tomllib
import urllib.request
import urllib.error
import zipfile
import test_bundle


def sha(path):
    digest = hashlib.sha256()
    with path.open('rb') as stream:
        for block in iter(lambda: stream.read(4 * 1024 * 1024), b''):
            digest.update(block)
    return digest.hexdigest()


def encoded(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':')).encode()


def durable(path, value):
    """Publish only complete fsynced records, including the directory entry."""
    path.parent.mkdir(parents=True, exist_ok=True)
    stage = path.with_suffix(path.suffix + '.partial')
    with stage.open('wb') as stream:
        stream.write(encoded(value) + b'\n'); stream.flush(); os.fsync(stream.fileno())
    stage.replace(path)
    descriptor = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY)
    try: os.fsync(descriptor)
    finally: os.close(descriptor)


def safe(name):
    path = PurePosixPath(name)
    if path.is_absolute() or '..' in path.parts or str(path) != name or name == '.':
        raise ValueError('unsafe bundle member')
    return name


class RemoteZip(io.RawIOBase):
    def __init__(self, url, size, refresh=None):
        self.url, self.size, self.pos = url, size, 0
        self.start, self.cache = 0, b''
        self.refresh = refresh
    def seekable(self): return True
    def tell(self): return self.pos
    def seek(self, offset, whence=0):
        self.pos = offset if whence == 0 else self.pos + offset if whence == 1 else self.size + offset
        return self.pos
    def read(self, count=-1):
        if self.pos >= self.size: return b''
        count = min(count if count >= 0 else self.size, self.size - self.pos)
        if not count: return b''
        end = self.pos + count
        if not (self.start <= self.pos and end <= self.start + len(self.cache)):
            finish = min(self.size, max(end, self.pos + 8 * 1024 * 1024))
            for attempt in range(2):
                request = urllib.request.Request(self.url, headers={'Range': f'bytes={self.pos}-{finish - 1}'})
                try:
                    response = urllib.request.urlopen(request, timeout=120)
                    break
                except urllib.error.HTTPError as error:
                    if error.code != 403 or not self.refresh or attempt:
                        raise
                    self.url = self.refresh()
            with response:
                if (response.status != 206 or response.headers.get('Content-Range')
                        != f'bytes {self.pos}-{finish - 1}/{self.size}'):
                    raise ValueError('archive range response mismatch')
                self.cache = response.read(finish - self.pos + 1)
                if len(self.cache) != finish - self.pos: raise ValueError('short archive range')
                self.start = self.pos
        result = self.cache[self.pos - self.start:end - self.start]
        self.pos = end
        return result


def refresh_artifact_url(artifact, url_file):
    """Renew only the managed GitHub artifact capability, without logging its URL."""
    result = subprocess.run(['gh', 'api', '--method', 'HEAD',
                             f'repos/michidk/cyrus/actions/artifacts/{artifact}/zip'],
                            env=dict(os.environ, GH_DEBUG='api'), text=True,
                            stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
    match = re.search(r'(?im)^< location: (https://[^\s]+)', result.stderr)
    if result.returncode or not match:
        raise ValueError('managed GitHub artifact URL renewal failed')
    url = match.group(1)
    url_file.write_text(url); url_file.chmod(0o600)
    return url


def admit_metadata(archive, expected, digest_receipt):
    archive_identity = {key: value for key, value in expected.items() if key not in {'commit', 'tree'}}
    if digest_receipt != dict(identity=archive_identity, sha256=expected['sha256'], bytes=expected['size']):
        raise ValueError('complete archive digest acceptance is absent or mismatched')
    raw = archive.read('manifest.json')
    manifest = json.loads(raw)
    provenance = json.loads(archive.read('source-provenance.json'))
    identity = manifest['identity']
    if (manifest['version'] != 1
            or manifest['scope'] != 'cargo test --locked --workspace --all-features --no-run'
            or identity['system'] != 'Linux' or identity['machine'] != 'x86_64'
            or any(provenance[key] != expected[other] for key, other in
                   [('checkout_commit', 'commit'), ('checkout_tree', 'tree'), ('run', 'run')])
            or provenance.get('repository') != 'michidk/cyrus'
            or provenance.get('head') != expected['commit']
            or int(identity.get('attempt', 0)) != provenance.get('attempt')
            or identity['commit'] != expected['commit'] or identity['tree'] != expected['tree']
            or int(identity['run']) != expected['run']
            or provenance['packages']['manifest.json'] != hashlib.sha256(raw).hexdigest()):
        raise ValueError('manifest/source provenance does not match exact artifact')
    paths = [safe(entry['path']) for entry in manifest['files']]
    if len(set(paths)) != len(paths): raise ValueError('duplicate bundle paths')
    for entry in manifest['files']: safe(entry['payload'])
    return manifest, hashlib.sha256(raw).hexdigest()


def verify_source(source, records, empty_files=()):
    """Verify every exported Git blob and reject an incomplete/modified source tree."""
    expected = set()
    for record in records.split(b'\0'):
        if not record: continue
        metadata, name = record.split(b'\t', 1)
        mode, kind, object_id = metadata.split()
        if kind != b'blob': raise ValueError('source contains unsupported non-blob entry')
        name = name.decode(); safe(name); expected.add(name); path = source / name
        if mode == b'120000':
            if not path.is_symlink(): raise ValueError('source symlink differs')
            content = os.readlink(path).encode()
        else:
            if path.is_symlink() or not path.is_file(): raise ValueError('source file absent')
            content = path.read_bytes()
            if bool(path.stat().st_mode & 0o111) != (mode == b'100755'):
                raise ValueError('source executable mode differs')
        actual = hashlib.sha1(b'blob ' + str(len(content)).encode() + b'\0' + content).hexdigest()
        if actual != object_id.decode(): raise ValueError(f'source blob differs: {name}')
    for name in empty_files:
        path = source / safe(name)
        if path.exists() and (path.is_symlink() or not path.is_file() or path.stat().st_size):
            raise ValueError('media mountpoint must be an empty placeholder')
    actual = {str(p.relative_to(source)) for p in source.rglob('*')
              if (p.is_file() or p.is_symlink()) and str(p.relative_to(source)) not in empty_files}
    if actual != expected: raise ValueError('source file census differs')


def prepare_mountpoints(source):
    """Create empty nested bind targets before the source parent becomes read-only."""
    for mountpoint in ['redguard', 'target', 'magic', 'game-config']:
        (source / mountpoint).mkdir(exist_ok=True)


def verify_daemon_source(image, source):
    """Reject a bind path that exists locally but is absent in the Docker daemon."""
    result = subprocess.run(['docker', 'run', '--rm', '--network', 'none',
                             '--mount', f'type=bind,src={source},dst=/source,readonly',
                             '--entrypoint', '/bin/bash', image, '-c',
                             'sha256sum /source/Cargo.toml'], text=True,
                            stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    if result.returncode or result.stdout.split()[0] != sha(source / 'Cargo.toml'):
        raise ValueError('Docker daemon cannot see the exact source bind path; use daemon-shared workspace storage')


def projection(manifest, test):
    if test['role'] != 'test' or test not in manifest['files']: raise ValueError('test absent')
    selected = [entry for entry in manifest['files'] if entry == test or entry['role'] == 'runtime'
                or (entry['role'] == 'companion' and entry['target'] in {'cyrus', 'cyrus-mcp'})]
    return selected, hashlib.sha256(encoded(selected)).hexdigest()


def cache_payload(archive, entry, cache):
    key = entry['sha256']; raw = cache / key; receipt = cache / (key + '.json')
    if raw.is_file() and receipt.is_file():
        saved = json.loads(receipt.read_text()); stat = raw.stat()
        if saved == dict(sha256=key, size=stat.st_size, inode=stat.st_ino, mtime=stat.st_mtime_ns):
            return raw
    # A retained raw file can move across mounts; rehash it instead of downloading again.
    if raw.is_file() and not raw.is_symlink() and raw.stat().st_size == entry['size'] and sha(raw) == key:
        raw.chmod(entry['mode'] & 0o555); stat = raw.stat()
        durable(receipt, dict(sha256=key, size=stat.st_size, inode=stat.st_ino, mtime=stat.st_mtime_ns))
        return raw
    cache.mkdir(parents=True, exist_ok=True)
    stage = cache / (key + '.partial'); digest = hashlib.sha256(); count = 0
    try:
        with archive.open(entry['payload']) as zipped, gzip.GzipFile(fileobj=zipped) as source, stage.open('wb') as output:
            while block := source.read(4 * 1024 * 1024):
                count += len(block)
                if count > entry['size']: raise ValueError('payload exceeds admitted size')
                digest.update(block); output.write(block)
            output.flush(); os.fsync(output.fileno())
        if count != entry['size'] or digest.hexdigest() != key: raise ValueError('raw payload hash mismatch')
        stage.chmod(entry['mode'] & 0o555); stage.replace(raw); stat = raw.stat()
        durable(receipt, dict(sha256=key, size=stat.st_size, inode=stat.st_ino, mtime=stat.st_mtime_ns))
        return raw
    finally:
        stage.unlink(missing_ok=True)


def fixture_directories(manifest, overrides):
    """Bind an explicit save group to one package/target, never synthesize task state."""
    tests = {entry['package'] + ':' + entry['target'] for entry in manifest['files'] if entry['role'] == 'test'}
    result = {}
    for value in overrides:
        target, separator, directory = value.partition('=')
        if not separator or target not in tests or target in result:
            raise ValueError('fixture override must uniquely name an exported package:target')
        path = Path(directory).resolve()
        if not path.is_dir() or not any(p.is_dir() and p.name.upper().startswith('SAVEGAME.') for p in path.iterdir()):
            raise ValueError('fixture override has no retained original slots')
        result[target] = path
    return result


def fixture_hash(directory):
    return hashlib.sha256(encoded([(str(p.relative_to(directory)), sha(p))
                                   for p in sorted(directory.rglob('*')) if p.is_file()])).hexdigest()


def receipt_path(results, test_hash, binding):
    """Keep prior runtime/fixture evidence when the same harness gets a new binding."""
    return results / (test_hash + '-' + hashlib.sha256(encoded(binding)).hexdigest()[:16] + '.json')


def accepted(receipt, binding, manifest, entry):
    if not receipt.is_file(): return False
    try:
        record = json.loads(receipt.read_text())
        if not isinstance(record, dict): return False
        log_name = record.get('log_name', receipt.with_suffix('.log').name)
        if Path(log_name).name != log_name: return False
        log = receipt.parent / log_name
        if not (record.get('binding') == binding and record.get('exit') == 0
                and record.get('install_skips') == 0 and log.is_file()
                and record.get('log_sha256') == sha(log)):
            return False
        if Path(record['admission']['tests']['log']).resolve() != log.resolve(): return False
        test_bundle.admit_installed_receipt(record['admission'], manifest, entry,
                                           binding['manifest'], binding['image'])
        return True
    except (ValueError, KeyError, TypeError, OSError):
        return False


def check_output(result, output):
    if result != 0: raise ValueError(f'test command failed: exit {result}')
    test_bundle.require_installed_output(output)
    summaries = re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored', output)
    if not summaries or any(int(failed) for _, failed, _ in summaries):
        raise ValueError('missing successful test summary')
    return sum(int(passed) for passed, _, _ in summaries)


def selected_tests(manifest, package, target):
    """Retry one authenticated harness without changing its invocation or receipts."""
    if target and not package:
        raise ValueError('target retry requires a package')
    tests = [entry for entry in manifest['files'] if entry['role'] == 'test'
             and (not package or entry['package'] == package)
             and (not target or entry['target'] == target)]
    if target and len(tests) != 1:
        raise ValueError('target retry requires exactly one exported test')
    return tests


def recorded_test(command, log, attempt, binding):
    """Retain source/input identity before execution, including interrupted failures."""
    record = dict(binding=binding, command=command, log_name=log.name, stage='running')
    durable(attempt, record)
    with log.open('w') as output:
        with subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True) as process:
            for line in process.stdout:
                output.write(line); output.flush(); os.fsync(output.fileno())
            result = subprocess.CompletedProcess(command, process.wait())
    durable(attempt, dict(record, stage='completed', exit=result.returncode, log_sha256=sha(log)))
    return result


def runtime_image(root, image):
    """Recover a task's pinned runtime after daemon loss without rebuilding it."""
    descriptor_path = root / 'runtime-image.json'
    descriptor = json.loads(descriptor_path.read_text()) if descriptor_path.is_file() else None
    inspect = ['docker', 'image', 'inspect', image, '--format', '{{.Id}}']
    result = subprocess.run(inspect, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    if result.returncode:
        if descriptor is None:
            raise ValueError('runtime image missing and no retained runtime descriptor supplied')
        archive = root / 'runtime-image.tar.gz'
        if (not archive.is_file() or archive.stat().st_size != descriptor['bytes']
                or sha(archive) != descriptor['sha256']):
            raise ValueError('retained runtime archive digest or size differs')
        subprocess.run([str(Path(__file__).with_name('ensure-runtime-image.sh')),
                        descriptor['image'], str(archive)], check=True)
        result = subprocess.run(inspect, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    if result.returncode:
        raise ValueError('runtime image unavailable after retained archive recovery')
    actual = result.stdout.strip()
    if descriptor is not None and actual != descriptor['image']:
        raise ValueError('runtime image differs from pinned retained identity')
    return actual


def libtest_arguments(test_threads):
    """Bound harness concurrency without filtering any exported tests."""
    if test_threads is not None and test_threads <= 0:
        raise ValueError('test thread count must be positive')
    return ['--nocapture'] + ([] if test_threads is None else [f'--test-threads={test_threads}'])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ['directory', 'install', 'save_directory']: parser.add_argument(name, type=Path)
    parser.add_argument('--image', default='cyrus-acceptance:419')
    parser.add_argument('--runtime-plan-tool', type=Path, default=Path(__file__).with_name('test_bundle.py'))
    parser.add_argument('--package')
    parser.add_argument('--target', help='retry one exact exported harness; requires --package')
    parser.add_argument('--test-threads', type=int, help='positive libtest worker count; defaults to libtest concurrency')
    parser.add_argument('--fixture-directory', action='append', default=[], metavar='PACKAGE:TARGET=PATH')
    parser.add_argument('--commit', required=True)
    parser.add_argument('--tree', required=True)
    args = parser.parse_args(); root = args.directory.resolve()
    arguments = libtest_arguments(args.test_threads)
    runner_hash = sha(Path(__file__))
    expected = dict(json.loads((root / 'artifact-identity.json').read_text()), commit=args.commit, tree=args.tree)
    digest = json.loads((root / 'artifact-digest.json').read_text())
    image = runtime_image(root, args.image)
    with zipfile.ZipFile(RemoteZip((root / 'download-url.txt').read_text().strip(), expected['size'],
                             lambda: refresh_artifact_url(expected['artifact'], root / 'download-url.txt'))) as archive:
        manifest, manifest_hash = admit_metadata(archive, expected, digest)
        workspace, target = manifest['workspace_root'], manifest['target_directory']
        source = root / 'source'
        tree = subprocess.check_output(['git', 'rev-parse', args.commit + '^{tree}'], text=True).strip()
        if tree != args.tree: raise ValueError('local source tree identity differs')
        records = subprocess.check_output(['git', 'ls-tree', '-rz', '--full-tree', args.commit])
        install = args.install.resolve(); saves = args.save_directory.resolve()
        media = json.loads(subprocess.check_output([sys.executable, str(args.runtime_plan_tool),
                           'runtime-mounts', str(install), workspace], text=True))
        empty_files = []
        for mount in media:
            if mount.get('read_only') is not True: raise ValueError('game media mount must be read-only')
            relative = str(PurePosixPath(mount['destination']).relative_to(workspace)); safe(relative)
            if Path(mount['source']).is_file(): empty_files.append(relative)
        verify_source(source, records, empty_files)
        prepare_mountpoints(source)
        for mount in media:
            destination = source / PurePosixPath(mount['destination']).relative_to(workspace)
            if Path(mount['source']).is_dir(): destination.mkdir(parents=True, exist_ok=True)
            else: destination.parent.mkdir(parents=True, exist_ok=True); destination.touch(exist_ok=True)
        verify_daemon_source(image, source)
        paths = {}
        for filename in source.rglob('Cargo.toml'):
            package = tomllib.loads(filename.read_text()).get('package', {}).get('name')
            if package: paths[package] = filename.parent.relative_to(source)
        install = args.install.resolve(); saves = args.save_directory.resolve()
        if not (install / 'ENGLISH.RTX').is_file() or not (saves / 'SAVEGAME.000').is_dir():
            raise ValueError('required explicit installation/save fixture is absent')
        install_identity = hashlib.sha256(encoded([(str(p.relative_to(install)), p.stat().st_size, p.stat().st_mtime_ns)
                                                  for p in sorted(install.rglob('*')) if p.is_file()])).hexdigest()
        default_fixture = fixture_hash(saves)
        overrides = fixture_directories(manifest, args.fixture_directory)
        cache = root / 'raw-cache'; results = root / 'results'; results.mkdir(exist_ok=True)
        for test in selected_tests(manifest, args.package, args.target):
            entries, projection_hash = projection(manifest, test)
            selected_saves = overrides.get(test['package'] + ':' + test['target'], saves)
            fixture = default_fixture if selected_saves == saves else hashlib.sha256(encoded(
                dict(group=fixture_hash(selected_saves), original_slot=default_fixture))).hexdigest()
            binding = dict(artifact=expected, manifest=manifest_hash, projection=projection_hash,
                           image=image, test=test, install=str(install), install_identity=install_identity, fixture=fixture,
                           arguments=arguments, package_cwd=str(paths[test['package']]))
            if test['package'] not in {'cyrus-script', 'cyrus-soup'}:
                binding['media_plan'] = media
            receipt = receipt_path(results, test['sha256'], binding)
            legacy = results / (test['sha256'] + '.json')
            if accepted(receipt, binding, manifest, test) or accepted(legacy, binding, manifest, test):
                print('retained', test['package'], test['target'], flush=True); continue
            print('preparing', test['package'], test['target'], flush=True)
            fresh = root / 'test-target'
            if fresh.exists(): shutil.rmtree(fresh)
            fresh.mkdir()
            try:
                for entry in entries:
                    cached = cache_payload(archive, entry, cache); destination = fresh / safe(entry['path'])
                    destination.parent.mkdir(parents=True, exist_ok=True); os.link(cached, destination)
                runs = root / 'runs'; runs.mkdir(exist_ok=True)
                context = Path(tempfile.mkdtemp(prefix=test['sha256'][:12] + '-', dir=runs))
                for name in ['config', 'magic', 'tmp']: (context / name).mkdir()
                base = ['docker', 'run', '--rm', '--network', 'none', '--user', f'{os.getuid()}:{os.getgid()}',
                        '-v', f'{source}:{workspace}:ro', '-v', f'{fresh}:{target}:ro',
                        '-v', f'{selected_saves}:/original-saves:ro',
                        '-v', f'{context}/config:{workspace}/game-config', '-v', f'{context}/magic:{workspace}/magic',
                        '-v', f'{context}/tmp:/test-tmp', '-w', f'{workspace}/{paths[test["package"]]}',
                        '-e', f'REDGUARD_DIR={workspace}/redguard', '-e', f'CYRUS_CONFIG_DIR={workspace}/game-config',
                        '-e', 'TMPDIR=/test-tmp', '-e', 'RUST_MIN_STACK=8388608',
                        '-e', 'CYRUS_ORIGINAL_SAVE=/original-saves/SAVEGAME.000',
                        '-e', 'REDGUARD_ORIGINAL_SAVE_DIR=/original-saves',
                        '-e', 'LD_LIBRARY_PATH=' + ':'.join(sorted({str(PurePosixPath(target) / PurePosixPath(e['path']).parent) for e in entries if e['role'] == 'runtime'}))]
                print('waiting for per-harness lock', test['target'], flush=True)
                for mount in media:
                    base += ['--mount', f'type=bind,src={mount["source"]},dst={mount["destination"]},readonly']
                if selected_saves != saves:
                    base += ['-v', f'{saves}/SAVEGAME.000:/original-slot:ro',
                             '-e', 'CYRUS_ORIGINAL_SAVE=/original-slot']
                with open('/tmp/cyrus-live.lock', 'a') as lock:
                    fcntl.flock(lock, fcntl.LOCK_EX)
                    for entry in entries:
                        loader_set = dict(image=image, manifest=manifest_hash, raw_sha256=entry['sha256'])
                        loader_key = hashlib.sha256(encoded(loader_set)).hexdigest()
                        loader_receipt = results / ('loader-' + loader_key + '.json')
                        if entry != test and loader_receipt.exists() and json.loads(loader_receipt.read_text()) == loader_set:
                            continue
                        executable = f'{target}/{entry["path"]}'
                        result = subprocess.run(base + ['--entrypoint', '/bin/bash', image, '-c',
                            'ldd "$1"', 'loader', executable], text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
                        if result.returncode or re.search(r'not found|version .* not found', result.stdout):
                            raise ValueError(f'loader rejection: {entry["path"]}: {result.stdout}')
                        durable(loader_receipt, loader_set)
                        if entry == test:
                            loader_log = results / (test['sha256'] + '-' + context.name + '.loader.log')
                            with loader_log.open('w') as output:
                                output.write(result.stdout); output.flush(); os.fsync(output.fileno())
                    executable = f'{target}/{test["path"]}'
                    listed = subprocess.run(base + ['--entrypoint', '/bin/bash', image, '-c', 'exec "$1" --list', 'list', executable],
                                            text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
                    if listed.returncode: raise ValueError('libtest enumeration failed: ' + listed.stdout)
                    list_log = results / (test['sha256'] + '-' + context.name + '.list.log')
                    with list_log.open('w') as output:
                        output.write(listed.stdout); output.flush(); os.fsync(output.fileno())
                    log = results / (test['sha256'] + '-' + context.name + '.log')
                    attempt = results / ('attempt-' + test['sha256'] + '-' + context.name + '.json')
                    result = recorded_test(base + [image, executable, *arguments], log, attempt, binding)
                    output = log.read_text(); passed = check_output(result.returncode, output)
                    admission = dict(accepted=True, commit=manifest['identity']['commit'],
                                     tree=manifest['identity']['tree'], original_manifest_sha256=manifest_hash,
                                     sha256=test['sha256'], image=image,
                                     loader=dict(exit=0, log=str(loader_log)),
                                     list=dict(exit=listed.returncode, log=str(list_log)),
                                     tests=dict(exit=result.returncode, log=str(log)))
                    test_bundle.admit_installed_receipt(admission, manifest, test, manifest_hash, image)
                    durable(receipt, dict(admission=admission, binding=binding, exit=0, install_skips=0, passed=passed, log_sha256=sha(log), log_name=log.name, runner_sha256=runner_hash,
                                          command=base + [image, executable, *arguments],
                                          enumeration=listed.stdout,
                                          ignored=sum(int(n) for n in re.findall(r'test result: ok\. \d+ passed; \d+ failed; (\d+) ignored', output))))
                print('accepted', test['package'], test['target'], passed, flush=True)
            finally:
                shutil.rmtree(fresh)
                # Only companions/runtime remain cached across independent harnesses.
                if accepted(receipt, binding, manifest, test):
                    (cache / test['sha256']).unlink(missing_ok=True)
                    (cache / (test['sha256'] + '.json')).unlink(missing_ok=True)


if __name__ == '__main__': main()
