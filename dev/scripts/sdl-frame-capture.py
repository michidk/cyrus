#!/usr/bin/env python3
"""Request a frozen SDL software presentation from the optional native interposer.

The local probe supplies process memory bindings. No game layouts are built in.
This captures a software surface, not OpenGL, GPU submission or mixer samples.
"""
import argparse
import fcntl
import json
import math
from pathlib import Path
import re
import time


def bindings(rows):
    if not isinstance(rows, dict) or not 1 <= len(rows) <= 16:
        raise ValueError('require one to sixteen named bindings')
    for name, row in rows.items():
        if (not isinstance(name, str) or not re.fullmatch(r'[a-zA-Z0-9_]{1,63}', name)
                or not isinstance(row, dict)
                or type(row.get('address')) is not int or not 0 < row['address'] < 2**63
                or type(row.get('size')) is not int or not 1 <= row['size'] <= 4096):
            raise ValueError(f'invalid presentation binding: {name}')
    return rows


def frozen_result(result, expected):
    if (not isinstance(result, dict) or result.get('ok') is not True or result.get('owner') != 'sdl_surface_present'
            or type(result.get('sequence')) is not int or result['sequence'] <= 0
            or type(result.get('thread')) is not int or result['thread'] <= 0
            or type(result.get('monotonic_ns')) is not int or result['monotonic_ns'] <= 0
            or not isinstance(result.get('fields'), dict)
            or result['fields'].keys() != expected.keys()):
        raise ValueError('capture has no accepted SDL presentation boundary')
    for name, row in expected.items():
        value = result['fields'][name]
        if not isinstance(value, str) or not re.fullmatch(r'[0-9a-f]+', value) or len(value) != row['size'] * 2:
            raise ValueError(f'presentation binding has incomplete bytes: {name}')
    return result


def capture(directory, output, rows, timeout=30, pcm_path=None, at_clock=None):
    if type(timeout) not in (int, float) or not math.isfinite(timeout) or timeout <= 0:
        raise ValueError('capture deadline must be finite and positive')
    rows = bindings(rows)
    if at_clock is not None:
        if (not isinstance(at_clock, dict) or set(at_clock) != {'field', 'value'}
                or not isinstance(at_clock['field'], str) or at_clock['field'] not in rows or rows[at_clock['field']]['size'] != 4
                or type(at_clock['value']) is not int or not 0 <= at_clock['value'] < 2**32):
            raise ValueError('scheduled capture requires a bound unsigned little-endian 32-bit clock')
    directory, output = Path(directory).resolve(), Path(output).resolve()
    directory.mkdir(parents=True, exist_ok=True)
    if len(str(output)) >= 4095 or '\n' in str(output) or '\r' in str(output):
        raise ValueError('capture output must be one path line')
    with (directory / 'capture.lock').open('a') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        output.mkdir(parents=True, exist_ok=False)
        request = directory / 'request'
        if request.exists():
            raise ValueError('another presentation request is pending')
        temporary = directory / 'request.tmp'
        text = str(output) + '\n' + ''.join(f'{name} {row["address"]} {row["size"]}\n' for name, row in rows.items())
        if pcm_path is not None:
            pcm_path = str(Path(pcm_path).resolve())
            if len(pcm_path) > 240 or '\n' in pcm_path or '\r' in pcm_path:
                raise ValueError('PCM path exceeds bounded request line')
            text += f'pcm {pcm_path}\n'
        if at_clock is not None:
            text += f'@clock {at_clock["field"]} {at_clock["value"]} {time.monotonic_ns() + int(timeout * 1e9)}\n'
        temporary.write_text(text)
        temporary.replace(request)
        deadline = time.monotonic() + timeout
        result_path = output / 'result.json'
        while not result_path.exists():
            if time.monotonic() >= deadline:
                # Do not reuse this output or redefine its boundary after a late completion.
                raise TimeoutError('SDL presentation capture unavailable before deadline')
            time.sleep(.01)
        result = frozen_result(json.loads(result_path.read_text()), rows)
        if at_clock is not None:
            actual = int.from_bytes(bytes.fromhex(result['fields'][at_clock['field']]), 'little')
            if actual != at_clock['value']:
                raise ValueError('scheduled presentation clock differs from requested boundary')
        # Encoding uses the worker's copied surface. It never samples the live window.
        from PIL import Image
        with Image.open(output / 'frame.bmp') as image:
            image.save(output / 'frame.png')
        return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('request_directory', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('bindings', type=Path)
    parser.add_argument('--at-clock', nargs=2, metavar=('FIELD', 'VALUE'))
    args = parser.parse_args()
    print(json.dumps(capture(args.request_directory, args.output, json.loads(args.bindings.read_text()),
        at_clock=dict(field=args.at_clock[0], value=int(args.at_clock[1])) if args.at_clock else None)))


if __name__ == '__main__':
    main()
