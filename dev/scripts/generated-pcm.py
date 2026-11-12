#!/usr/bin/env python3
"""Validate and export completed SDL mixer-output blocks, independent of file delivery.

A cursor identifies a completed producer callback. Its timestamp and sample
prefix are frozen at generation; they are never inferred from recorder length.
The output is stereo s16le at the actual device rate, without resampling.
"""
import array
import json
from pathlib import Path
import sys
import time
import wave


def cursor(value):
    if (not isinstance(value, dict) or any(type(value.get(key)) is not int or value[key] <= 0
            for key in ('frames', 'sequence', 'monotonic_ns', 'rate'))):
        raise ValueError('require a completed mixer-generation cursor')
    return value


def validate(blocks, first, last):
    first, last = cursor(first), cursor(last)
    if first['rate'] != last['rate'] or first['sequence'] >= last['sequence']:
        raise ValueError('mixer generation reset or no completed interval')
    end = sequence = ns = 0
    endpoints = {}
    for block in blocks:
        if (not isinstance(block, dict) or any(type(block.get(key)) is not int
                for key in ('sequence', 'first', 'last', 'monotonic_ns', 'rate'))
                or block['sequence'] != sequence + 1 or block['first'] != end
                or block['last'] <= end or block['monotonic_ns'] <= ns
                or block['rate'] != first['rate']):
            raise ValueError('generated PCM discontinuity, format change or delivery failure')
        if (first['sequence'] < block['sequence'] <= last['sequence']
                and abs((block['monotonic_ns']-ns)/1e9 - (block['last']-end)/first['rate']) > .25):
            raise ValueError('mixer-generation pacing discontinuity')
        sequence, end, ns = block['sequence'], block['last'], block['monotonic_ns']
        endpoints[sequence] = dict(frames=end, sequence=sequence, monotonic_ns=ns, rate=block['rate'])
        if sequence == last['sequence']:
            break
    if endpoints.get(first['sequence']) != first or endpoints.get(last['sequence']) != last:
        raise ValueError('generation cursor does not match recorded producer blocks')
    count = last['frames'] - first['frames']
    seconds = count / first['rate']
    wall = (last['monotonic_ns'] - first['monotonic_ns']) / 1e9
    if abs(seconds - wall) > .25:
        raise ValueError('mixer-generation pacing mismatch')
    return dict(frames=count, seconds=seconds, generation_seconds=wall,
                rate=first['rate'], pcm_mapping='completed_mixer_callbacks',
                first=first, last=last)


def extract(directory, first, last, output, timeout=5):
    directory, output = Path(directory), Path(output)
    first, last = cursor(first), cursor(last)
    deadline = time.monotonic() + timeout
    while True:
        data = (directory / 'generated.jsonl').read_bytes()
        # A writer may currently be appending its final line. It is not a block yet.
        lines = data[:data.rfind(b'\n') + 1].splitlines()
        blocks = [json.loads(line) for line in lines]
        if any('error' in block for block in blocks):
            raise ValueError('generated PCM producer reported a discontinuity')
        if blocks and blocks[-1].get('sequence', 0) >= last['sequence']:
            break
        if time.monotonic() >= deadline:
            raise TimeoutError('generated PCM delivery did not reach the acknowledged prefix')
        time.sleep(.01)
    result = validate(blocks, first, last)
    with (directory / 'generated.raw').open('rb') as source:
        source.seek(first['frames'] * 4)
        pcm = source.read(result['frames'] * 4)
    if len(pcm) != result['frames'] * 4:
        raise ValueError('generated PCM delivery is truncated')
    samples = array.array('h', pcm)
    if sys.byteorder != 'little':
        samples.byteswap()
    result['peak'] = max(map(abs, samples), default=0)
    if not result['peak']:
        raise ValueError('silent generated PCM interval')
    if output.exists():
        raise ValueError('generated PCM output exists; preserve evidence')
    with wave.open(str(output), 'wb') as target:
        target.setparams((2, 2, first['rate'], 0, 'NONE', 'not compressed'))
        target.writeframes(pcm)
    return result
