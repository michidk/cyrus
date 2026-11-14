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
    prior = {}
    for name, row in rows.items():
        if (not isinstance(name, str) or not re.fullmatch(r'[a-zA-Z0-9_]{1,63}', name)
                or not isinstance(row, dict)
                or type(row.get('size')) is not int or not 1 <= row['size'] <= 4096):
            raise ValueError(f'invalid presentation binding: {name}')
        if 'pointer' in row:
            pointer = row['pointer']
            if (set(row) != {'pointer', 'size'} or not isinstance(pointer, dict)
                    or set(pointer) != {'field', 'offset', 'width', 'addend'}
                    or not isinstance(pointer['field'], str) or pointer['field'] not in prior
                    or type(pointer['offset']) is not int or pointer['offset'] < 0
                    or type(pointer['width']) is not int or pointer['width'] not in (4, 8)
                    or pointer['offset'] + pointer['width'] > prior[pointer['field']]['size']
                    or type(pointer['addend']) is not int or not -(2**63) <= pointer['addend'] < 2**63):
                raise ValueError(f'invalid presentation pointer binding: {name}')
        elif (set(row) != {'address', 'size'} or type(row.get('address')) is not int
                or not 0 < row['address'] < 2**63):
            raise ValueError(f'invalid presentation binding: {name}')
        prior[name] = row
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
        text = str(output) + '\n'
        for name, row in rows.items():
            if 'pointer' in row:
                pointer = row['pointer']
                text += (f'@pointer {name} {pointer["field"]} {pointer["offset"]} '
                         f'{pointer["width"]} {pointer["addend"]} {row["size"]}\n')
            else:
                text += f'{name} {row["address"]} {row["size"]}\n'
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


def capture_progress(directory, output, rows, clock_field, progress, timeout=30,
                     max_captures=128, require_pcm=False, minimum_pcm_frames=1):
    """Persist presented states until the caller-bound clock advances by progress.

    The caller supplies the playback clock binding, never a wall-time conversion.
    PCM-required series also wait for the requested generated sample-frame span.
    Rejected or interrupted series retain their individual captures for diagnosis.
    """
    rows = bindings(rows)
    if (clock_field not in rows or rows[clock_field]['size'] != 4
            or type(progress) is not int or not 0 < progress < 2**32
            or type(max_captures) is not int or not 2 <= max_captures <= 4096
            or type(timeout) not in (int, float) or not math.isfinite(timeout) or timeout <= 0):
        raise ValueError('progress requires a bound u32 clock and finite capture budget')
    if (type(minimum_pcm_frames) is not int or minimum_pcm_frames <= 0
            or (not require_pcm and minimum_pcm_frames != 1)):
        raise ValueError('minimum PCM frames requires a positive integer and PCM capture')
    output = Path(output)
    output.mkdir(parents=True, exist_ok=False)
    deadline = time.monotonic() + timeout
    samples = []
    for index in range(max_captures):
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise TimeoutError('presentation clock or generated PCM did not reach requested progress')
        result = capture(directory, output / str(index), rows, timeout=remaining)
        clock = int.from_bytes(bytes.fromhex(result['fields'][clock_field]), 'little')
        if samples and (clock < samples[-1]['clock']
                or result['sequence'] <= samples[-1]['sequence']
                or result['monotonic_ns'] < samples[-1]['monotonic_ns']):
            raise ValueError('presentation clock or sequence reversed')
        if require_pcm:
            pcm = result.get('pcm_generation', {})
            if (any(type(pcm.get(key)) is not int or pcm[key] <= 0
                    for key in ('frames', 'sequence', 'monotonic_ns', 'rate'))
                    or pcm['monotonic_ns'] > result['monotonic_ns']):
                raise ValueError('presentation has no preceding generated PCM boundary')
            if samples:
                previous = samples[-1]['pcm_generation']
                if (pcm['rate'] != previous['rate'] or pcm['frames'] < previous['frames']
                        or pcm['sequence'] < previous['sequence']
                        or pcm['monotonic_ns'] < previous['monotonic_ns']):
                    raise ValueError('generated PCM boundary reversed or changed sample domain')
        samples.append(dict(result, clock=clock, capture=str(index)))
        temporary = output / 'progress.tmp'
        temporary.write_text(json.dumps(samples, indent=2))
        temporary.replace(output / 'progress.json')
        pcm_span = (samples[-1]['pcm_generation']['frames']
                    - samples[0]['pcm_generation']['frames']) if require_pcm else None
        if (clock - samples[0]['clock'] >= progress
                and (not require_pcm or pcm_span >= minimum_pcm_frames)):
            receipt = dict(clock_field=clock_field, first=samples[0]['clock'], last=clock,
                           captures=len(samples), progress=clock - samples[0]['clock'])
            if require_pcm:
                receipt.update(pcm_frames=pcm_span, pcm_rate=samples[0]['pcm_generation']['rate'],
                               minimum_pcm_frames=minimum_pcm_frames)
            temporary = output / 'complete.tmp'
            temporary.write_text(json.dumps(receipt, indent=2))
            temporary.replace(output / 'complete.json')
            return receipt
    raise TimeoutError('presentation clock or generated PCM did not reach requested progress within capture budget')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('request_directory', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('bindings', type=Path)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument('--at-clock', nargs=2, metavar=('FIELD', 'VALUE'))
    mode.add_argument('--progress', nargs=2, metavar=('FIELD', 'TICKS'))
    parser.add_argument('--timeout', type=float, default=30)
    parser.add_argument('--require-pcm', action='store_true')
    parser.add_argument('--minimum-pcm-frames', type=int, default=1,
                        help='generated sample frames required in addition to clock progress')
    args = parser.parse_args()
    rows = json.loads(args.bindings.read_text())
    if args.minimum_pcm_frames != 1 and not args.require_pcm:
        parser.error('--minimum-pcm-frames requires --require-pcm')
    if args.progress:
        result = capture_progress(args.request_directory, args.output, rows,
            args.progress[0], int(args.progress[1]), timeout=args.timeout, require_pcm=args.require_pcm,
            minimum_pcm_frames=args.minimum_pcm_frames)
    else:
        if args.require_pcm:
            parser.error('--require-pcm requires --progress')
        result = capture(args.request_directory, args.output, rows, timeout=args.timeout,
            at_clock=dict(field=args.at_clock[0], value=int(args.at_clock[1])) if args.at_clock else None)
    print(json.dumps(result))


if __name__ == '__main__':
    main()
