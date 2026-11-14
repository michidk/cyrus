#!/usr/bin/env python3
"""Run installed animation replacement controls without starting a preview."""
import argparse
import fcntl
import json
import shutil
from pathlib import Path
import subprocess
import time
import urllib.request

IMAGE = 'chromedp/headless-shell@sha256:830fa77e0602cb42e5cb2cbd9237ba27b649ad69d5fb82b54713aac8b7ab4f29'


def run(*args):
    return subprocess.run(args, check=True)


def ready(url, process=None):
    deadline = time.monotonic() + 30
    while time.monotonic() < deadline:
        if process is not None and process.poll() is not None:
            raise RuntimeError('acceptance HTTP server exited before readiness')
        try:
            with urllib.request.urlopen(url, timeout=1) as response:
                if response.status == 200:
                    return
        except OSError:
            pass
        time.sleep(.1)
    raise TimeoutError(f'acceptance endpoint did not become ready: {url}')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('rig', type=Path, help='persistent host directory containing web/ bindings and fixture.json')
    parser.add_argument('--cdp-port', type=int, default=19421)
    parser.add_argument('--http-port', type=int, default=19422)
    args = parser.parse_args()
    rig = args.rig.resolve()
    if not rig.is_relative_to('/home/vibepod/workspaces'):
        parser.error('rig must reside under /home/vibepod/workspaces for Docker bind mounts')
    for name in ['fixture.json', 'replacement_lifecycle.js', 'replacement_lifecycle_bg.wasm']:
        if not (rig / 'web' / name).is_file():
            parser.error(f'missing retained artifact: web/{name}; this launcher never recompiles')
    with (rig / 'browser.lock').open('a') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        shutil.copyfile(Path(__file__).resolve().parent.parent / 'fixtures' / 'animation-replacement.html',
                        rig / 'web' / 'index.html')
        (rig / 'config').mkdir(exist_ok=True)
        name = f'cyrus-animation-replacement-{args.cdp_port}'
        existing = subprocess.run(['docker', 'inspect', name], capture_output=True, text=True)
        if existing.returncode == 0:
            info = json.loads(existing.stdout)[0]
            if info['Config'].get('Labels', {}).get('cyrus.acceptance') != 'animation-replacement':
                raise RuntimeError(f'refusing to replace unowned container {name}')
            run('docker', 'rm', '-f', name)
        if subprocess.run(['docker', 'image', 'inspect', IMAGE], capture_output=True).returncode:
            run('docker', 'pull', IMAGE)
        log = (rig / 'browser-server.log').open('a')
        server = subprocess.Popen(['python3', '-m', 'http.server', str(args.http_port),
                                   '--bind', '0.0.0.0', '--directory', str(rig / 'web')],
                                  stdout=log, stderr=log)
        started = False
        try:
            ready(f'http://127.0.0.1:{args.http_port}/', server)
            run('docker', 'run', '-d', '--name', name, '--label', 'cyrus.acceptance=animation-replacement',
                '-p', f'127.0.0.1:{args.cdp_port}:9222',
                '--add-host=host.docker.internal:host-gateway',
                '-e', 'CYRUS_CONFIG_DIR=/rig/config', '-v', f'{rig}:/rig',
                IMAGE, '--autoplay-policy=no-user-gesture-required')
            started = True
            ready(f'http://127.0.0.1:{args.cdp_port}/json/version')
            run('docker', 'exec', '-d', name, 'socat',
                f'TCP-LISTEN:{args.http_port},bind=127.0.0.1,reuseaddr,fork',
                f'TCP:host.docker.internal:{args.http_port}')
            subprocess.run(['node', 'dev/scripts/test-animation-replacement-browser.mjs',
                            f'http://127.0.0.1:{args.cdp_port}', f'http://127.0.0.1:{args.http_port}/',
                            str(rig / 'web/replacement_lifecycle_bg.wasm'), str(rig / 'browser-result.json')],
                           check=True, timeout=110)
        finally:
            if started:
                subprocess.run(['docker', 'rm', '-f', name], check=False)
            server.terminate()
            server.wait(timeout=5)
            log.close()


if __name__ == '__main__':
    main()
