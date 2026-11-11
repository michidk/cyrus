#!/usr/bin/env python3
"""Recover the minimal installed transient acceptance runtime without Cargo."""
import argparse
from datetime import datetime, timezone
import fcntl
import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import uuid

ROOT = Path(__file__).resolve().parents[2]


def docker(*args, **kwargs):
    return subprocess.run(['docker', *map(str, args)], check=True, **kwargs)


def ensure_image(image, log, archive=None):
    if subprocess.run(['docker', 'image', 'inspect', image], stdout=subprocess.DEVNULL,
                      stderr=subprocess.DEVNULL).returncode:
        if archive is not None and archive.exists():
            docker('load', '-i', archive, stdout=log, stderr=subprocess.STDOUT)
        else:
            docker('build', '-t', image, '-f', ROOT / 'dev/docker/dev/Dockerfile.acceptance',
                   ROOT / 'dev/docker/dev', stdout=log, stderr=subprocess.STDOUT)
    if archive is not None and not archive.exists():
        temporary = archive.with_suffix('.tmp')
        docker('save', '-o', temporary, image, stdout=log, stderr=subprocess.STDOUT)
        temporary.replace(archive)


def retain_previous(output, attempt):
    previous = attempt / 'previous-state'
    previous.mkdir()
    for path in output.iterdir():
        if path.is_file() and path.suffix in ('.json', '.session', '.png'):
            target = previous / path.name
            try:
                os.link(path, target)
            except OSError:
                shutil.copy2(path, target)


def run_driver(command, log):
    process = subprocess.Popen(command, stdout=log, stderr=subprocess.STDOUT)
    try:
        result = process.wait()
        if result:
            raise subprocess.CalledProcessError(result, command)
    finally:
        if process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()


def interrupted(signum, _frame):
    raise SystemExit(128 + signum)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--runtime', required=True, type=Path)
    parser.add_argument('--binary', required=True, type=Path)
    parser.add_argument('--sha256', required=True)
    parser.add_argument('--install', required=True, type=Path)
    parser.add_argument('--cache', required=True, type=Path)
    parser.add_argument('--port', type=int, default=7126)
    parser.add_argument('--gpu-pixels', action='store_true', help='run paired GPU-output acceptance')
    parser.add_argument('--seed-session', type=Path)
    args = parser.parse_args()
    if args.gpu_pixels and args.seed_session is None:
        parser.error('--gpu-pixels requires --seed-session')
    signal.signal(signal.SIGTERM, interrupted)
    for name in ('runtime', 'binary', 'install', 'cache'):
        path = getattr(args, name).resolve(strict=True)
        if not path.is_relative_to('/home/vibepod/workspaces'):
            parser.error(f'{name} must be inside /home/vibepod/workspaces')
        setattr(args, name, path)
    if not args.binary.is_relative_to(args.runtime):
        parser.error('binary must be inside runtime mount')
    with args.binary.open('rb') as binary:
        actual = hashlib.file_digest(binary, 'sha256').hexdigest()
    if actual != args.sha256:
        parser.error(f'executable digest mismatch: {actual}')
    with (args.runtime / 'transient-runtime.lock').open('a') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        attempt = datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%S') + '-' + uuid.uuid4().hex[:8]
        output_name = 'transients-pixels' if args.gpu_pixels else 'transients-current'
        logs = args.runtime / 'evidence' / output_name / 'attempts' / attempt
        logs.mkdir(parents=True)
        retain_previous(logs.parent.parent, logs)
        config = args.runtime / 'transient-config'
        config.mkdir(exist_ok=True)
        link = config / 'cache'
        if not link.exists() and not link.is_symlink():
            link.symlink_to('/cache')
        if not link.is_symlink() or str(link.readlink()) != '/cache':
            raise ValueError('runtime config cache must link to /cache')
        recipe = b''.join((ROOT / 'dev/docker/dev' / name).read_bytes()
                          for name in ('Dockerfile.acceptance', 'entrypoint-acceptance.sh'))
        recipe_id = hashlib.sha256(recipe).hexdigest()[:16]
        image = 'cyrus-dev:transient-' + recipe_id
        archive = args.runtime / ('runtime-image-' + recipe_id + '.tar')
        name = 'cyrus-transient-' + uuid.uuid4().hex[:12]
        with (logs / 'image.log').open('w') as log:
            ensure_image(image, log, archive)
        image_id = docker('image', 'inspect', '--format', '{{.Id}}', image,
                          capture_output=True, text=True).stdout.strip()
        (logs / 'provenance.json').write_text(json.dumps({
            'binary_sha256': actual, 'image': image_id, 'binary': str(args.binary)}, indent=2))
        started = False
        follower = None
        with (logs / 'runtime.log').open('w') as runtime_log:
            try:
                docker('run', '-d', '--rm', '--name', name, '-p', f'127.0.0.1:{args.port}:7117',
                       '-e', 'SCREEN=640x480x24', '-e', 'CYRUS_CONFIG_DIR=/run/transient-config',
                       '-e', 'BEVY_ASSET_ROOT=/run', '-e', 'REDGUARD_DIR=/game',
                       '-e', 'CYRUS_CONTROL_ADDR=0.0.0.0:7117',
                       '-v', f'{args.runtime}:/run', '-v', f'{args.cache}:/cache',
                       '-v', f'{args.install}:/game:ro', '-w', '/run', image_id,
                       '/run/' + str(args.binary.relative_to(args.runtime)),
                       '--install', '/game', '--scene', 'START', '--volume', '0')
                started = True
                follower = subprocess.Popen(['docker', 'logs', '-f', name], stdout=runtime_log,
                                            stderr=subprocess.STDOUT)
                with (logs / 'driver.log').open('w') as driver_log:
                    run_driver([sys.executable, str(ROOT / 'dev/scripts' / ('native-transient-pixels.py' if args.gpu_pixels else 'native-transient-session.py')),
                                    str(args.port), str(args.runtime / 'evidence' / output_name),
                                    '--binary', str(args.binary), '--runtime-output',
                                    '/run/evidence/' + output_name,
                                    *(['--seed-session', str(args.seed_session)] if args.seed_session else [])], driver_log)
            finally:
                if started:
                    subprocess.run(['docker', 'stop', name], stdout=subprocess.DEVNULL)
                if follower:
                    follower.wait(timeout=20)


if __name__ == '__main__':
    main()
