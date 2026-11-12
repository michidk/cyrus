#!/usr/bin/env python3
"""Run one installed authored speech acceptance using retained browser code, without Cargo."""
import argparse
import fcntl
import json
from pathlib import Path
import subprocess
import time
import urllib.request

IMAGE = 'chromedp/headless-shell@sha256:830fa77e0602cb42e5cb2cbd9237ba27b649ad69d5fb82b54713aac8b7ab4f29'
ROOT = Path(__file__).resolve().parents[2]


def ready(url, server=None):
    deadline = time.monotonic() + 30
    while time.monotonic() < deadline:
        if server is not None and server.poll() is not None:
            raise RuntimeError('installed acceptance server exited before readiness')
        try:
            with urllib.request.urlopen(url, timeout=1) as response:
                if response.status == 200:
                    return
        except OSError:
            pass
        time.sleep(.1)
    raise TimeoutError(f'acceptance endpoint unavailable: {url}')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('runtime', type=Path)
    parser.add_argument('install', type=Path)
    parser.add_argument('fixture', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('--driver', choices=['speech', 'music', 'ambient'], default='speech')
    parser.add_argument('--cdp-port', type=int, default=19361)
    parser.add_argument('--http-port', type=int, default=19362)
    args = parser.parse_args()
    runtime = args.runtime.resolve()
    if not runtime.is_relative_to('/home/vibepod/workspaces'):
        parser.error('runtime must be under /home/vibepod/workspaces')
    runtime.mkdir(parents=True, exist_ok=True)
    if not (ROOT / 'target/web/build-provenance.json').is_file():
        parser.error('sealed installed browser package is required; this launcher never compiles')
    with (runtime / 'browser.lock').open('a') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        name = f'cyrus-authored-speech-{args.cdp_port}'
        prior = subprocess.run(['docker', 'inspect', name], capture_output=True, text=True)
        if prior.returncode == 0:
            info = json.loads(prior.stdout)[0]
            if info['Config'].get('Labels', {}).get('cyrus.acceptance') != 'authored-speech':
                raise RuntimeError(f'refusing to replace unowned container {name}')
            subprocess.run(['docker', 'rm', '-f', name], check=True)
        archive = runtime / 'chrome-image.tar'
        if subprocess.run(['docker', 'image', 'inspect', IMAGE], capture_output=True).returncode:
            if archive.is_file():
                subprocess.run([str(ROOT / 'dev/scripts/ensure-runtime-image.sh'), IMAGE, str(archive)], check=True)
            else:
                subprocess.run(['docker', 'pull', IMAGE], check=True)
        with (runtime / 'server.log').open('a') as log:
            server = subprocess.Popen(['python3', str(ROOT / 'dev/scripts/viewer-server.py'),
                '--page', 'index.html', '--bind', '0.0.0.0', '--port', str(args.http_port),
                '--directory', str(ROOT / 'target/web'), '--install', str(args.install.resolve())],
                stdout=log, stderr=log)
            started = False
            try:
                ready(f'http://127.0.0.1:{args.http_port}/', server)
                subprocess.run(['docker', 'run', '-d', '--name', name,
                    '--label', 'cyrus.acceptance=authored-speech',
                    '-p', f'127.0.0.1:{args.cdp_port}:9222',
                    '--add-host=host.docker.internal:host-gateway',
                    '-e', 'CYRUS_CONFIG_DIR=/rig/config', '-v', f'{runtime}:/rig',
                    IMAGE, '--autoplay-policy=no-user-gesture-required'], check=True)
                started = True
                ready(f'http://127.0.0.1:{args.cdp_port}/json/version')
                # Loopback is a secure context without relying on Chromium's
                # version-dependent handling of insecure-origin launch flags.
                subprocess.run(['docker', 'exec', '-d', name, 'socat',
                    f'TCP-LISTEN:{args.http_port},bind=127.0.0.1,reuseaddr,fork',
                    f'TCP:host.docker.internal:{args.http_port}'], check=True)
                base = f'http://127.0.0.1:{args.http_port}/'
                if args.driver == 'ambient':
                    fixture = json.loads(args.fixture.read_text())
                    command = ['node', str(ROOT / 'dev/scripts/test-ambient-admission-browser.mjs'),
                        f'http://127.0.0.1:{args.cdp_port}', base, base + 'original/',
                        str(args.output.resolve()), fixture['emitter'], str(fixture['effect']),
                        fixture.get('scene', 'ISLAND'), fixture.get('destination', 'START'),
                        fixture['source_fixture'], str(fixture['emitter_index']), fixture.get('phase', 'bank'), json.dumps(fixture.get('listener_offset', [0, 0, 0]))]
                else:
                    command = ['node', str(ROOT / ('dev/scripts/browser-authored-speech.mjs'
                        if args.driver == 'speech' else 'dev/scripts/browser-music-world-entry.mjs')),
                        f'http://127.0.0.1:{args.cdp_port}', base, base + 'original/',
                        str(args.fixture.resolve()), str(args.output.resolve())]
                subprocess.run(command, check=True, timeout=900, cwd=ROOT)
            finally:
                if started:
                    subprocess.run(['docker', 'rm', '-f', name], check=False)
                server.terminate()
                server.wait(timeout=5)


if __name__ == '__main__':
    main()
