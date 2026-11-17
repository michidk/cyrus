#!/usr/bin/env python3
"""Check the real capture runtime's non-root DOSBox interposer loading.

Usage: test-retro-capture-loader.py IMAGE
"""
import subprocess
import sys

image = sys.argv[1]
for library in ('sdl-frame-capture.so', 'sdl-mixer-capture.so'):
    result = subprocess.run([
        'docker', 'run', '--rm', '--user', '1000:1000',
        '-e', 'LD_DEBUG=libs', '-e', f'LD_PRELOAD=/usr/local/lib/{library}',
        '--entrypoint', '/usr/bin/dosbox-x', image, '-version',
    ], capture_output=True, text=True, timeout=30)
    if 'DOSBox-X version' not in result.stdout + result.stderr:
        raise AssertionError(f'DOSBox version probe failed ({result.returncode}): {result.stderr}')
    if f'calling init: /usr/local/lib/{library}' not in result.stderr:
        raise AssertionError(f'non-root DOSBox did not load {library}:\n{result.stderr}')
print('Non-root DOSBox loads both presentation and mixer capture interposers')
