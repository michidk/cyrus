#!/usr/bin/env python3
"""Check packaged native install resolution without opening a window or exporting assets.

Pass --binary, --install and a fresh --output directory. Every child gets a host
configuration directory and its own live lock; stdout/stderr survive failures.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess

from harness import LIVE_LOCK, exclusive_lock


def run(binary, install, output):
    output.mkdir(parents=True, exist_ok=False)
    missing = output / 'missing-redguard'
    cases = []

    def invoke(name, source, flags, success):
        directory = output / name
        directory.mkdir()
        config = directory / 'config'
        config.mkdir()
        command = [str(binary), '--install', str(source), *flags]
        environment = {**os.environ, 'CYRUS_CONFIG_DIR': str(config)}
        with exclusive_lock(LIVE_LOCK), (directory / 'stdout.log').open('w') as stdout, \
                (directory / 'stderr.log').open('w') as stderr:
            result = subprocess.run(command, env=environment, stdout=stdout,
                                    stderr=stderr, text=True, timeout=30)
        result.stdout = (directory / 'stdout.log').read_text()
        result.stderr = (directory / 'stderr.log').read_text()
        cases.append({'case': name, 'returncode': result.returncode})
        if result.returncode != (0 if success else 1):
            raise RuntimeError(f'{name}: unexpected exit {result.returncode}; see {directory}')
        return result

    worlds = json.loads(invoke('worlds', install, ['--list-worlds'], True).stdout)
    if not worlds or not all(isinstance(world, str) for world in worlds):
        raise RuntimeError('worlds: expected a nonempty installed world-name list')
    combined = invoke('world-list-precedence', install,
                      ['--list-worlds', '--headless', '--export-startup', '--export-all-worlds'], True)
    if json.loads(combined.stdout) != worlds:
        raise RuntimeError('world-list-precedence: installed world selection changed')
    slots = invoke('slots-without-install', missing, ['--list-slots', '--audit-worlds'], True)
    if json.loads(slots.stdout) != []:
        raise RuntimeError('slots-without-install: expected an empty private slot store')

    for name, flags, context in [
        ('audit', ['--audit-worlds'], '--audit-worlds'),
        ('list', ['--list-worlds'], '--list-worlds'),
        ('headless', ['--headless'], 'headless launch or asset export'),
        ('scene', ['--export-scene', '--scene', 'START'], 'headless launch or asset export'),
        ('startup', ['--export-startup'], 'headless launch or asset export'),
        ('all-worlds', ['--export-all-worlds'], 'headless launch or asset export'),
        ('audit-precedence', ['--audit-worlds', '--list-worlds'], '--audit-worlds'),
    ]:
        result = invoke(name, missing, flags, False)
        if context not in result.stderr or str(missing) not in result.stderr:
            raise RuntimeError(f'{name}: missing CLI/source context; see {output / name}')
        if 'panicked at' in result.stderr:
            raise RuntimeError(f'{name}: install error panicked')
    receipt = {'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
               'world_count': len(worlds), 'cases': cases}
    (output / 'result.json').write_text(json.dumps(receipt, indent=2) + '\n')
    print(json.dumps(receipt))


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--install', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    run(args.binary.resolve(strict=True), args.install.resolve(strict=True), args.output.resolve())
