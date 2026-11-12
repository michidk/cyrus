#!/usr/bin/env python3
"""Validate and export completed native or SDL mixer-output blocks, independent of file delivery.

A cursor identifies a completed producer callback. Its timestamp and sample
prefix are frozen at generation; they are never inferred from recorder length.
Raw blocks retain their native stereo format and rate. Float WAV export is
explicitly a signed-16 preview conversion, without resampling.
"""
import array
import hashlib
import math
import json
from pathlib import Path
import sys
import time
import wave


def cursor(value):
    if (not isinstance(value, dict) or any(type(value.get(key)) is not int or value[key] <= 0
            for key in ('frames', 'sequence', 'monotonic_ns', 'rate'))):
        raise ValueError('require a completed mixer-generation cursor')
    if value.get("format", "s16le") not in ("s16le", "f32le"):
        raise ValueError("unsupported generated PCM format")
    return value


def validate(blocks, first, last):
    first, last = cursor(first), cursor(last)
    if first.get('format', 's16le') != last.get('format', 's16le') or first['rate'] != last['rate'] or first['sequence'] >= last['sequence']:
        raise ValueError('mixer generation reset or no completed interval')
    end = sequence = ns = 0
    endpoints = {}
    for block in blocks:
        if (not isinstance(block, dict) or any(type(block.get(key)) is not int
                for key in ('sequence', 'first', 'last', 'monotonic_ns', 'rate'))
                or block['sequence'] != sequence + 1 or block['first'] != end
                or block['last'] <= end or block['monotonic_ns'] <= ns
                or block['rate'] != first['rate']
                or block.get('format', 's16le') != first.get('format', 's16le')):
            raise ValueError('generated PCM discontinuity, format change or delivery failure')
        if (first['sequence'] < block['sequence'] <= last['sequence']
                and abs((block['monotonic_ns']-ns)/1e9 - (block['last']-end)/first['rate']) > .25):
            raise ValueError('mixer-generation pacing discontinuity')
        sequence, end, ns = block['sequence'], block['last'], block['monotonic_ns']
        endpoints[sequence] = dict(frames=end, sequence=sequence, monotonic_ns=ns, rate=block['rate'])
        if 'format' in first:
            endpoints[sequence]['format'] = block.get('format', 's16le')
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
                first=first, last=last, format=first.get('format', 's16le'))


def pacing_report(blocks, first, last):
    """Describe producer timing after a rejection; this never grants acceptance."""
    first, last = cursor(first), cursor(last)
    rows = [row for row in blocks if type(row.get('sequence')) is int
            and first['sequence'] < row['sequence'] <= last['sequence']]
    previous_ns, previous_frames = first['monotonic_ns'], first['frames']
    gaps = []
    for row in rows:
        wall = (row['monotonic_ns'] - previous_ns) / 1e9
        samples = (row['last'] - previous_frames) / first['rate']
        gaps.append(dict(sequence=row['sequence'], generation_seconds=wall,
                         sample_seconds=samples, difference_seconds=wall-samples))
        previous_ns, previous_frames = row['monotonic_ns'], row['last']
    return dict(acceptance=False, attribution='completed_output_callback',
                sample_seconds=(last['frames']-first['frames'])/first['rate'],
                generation_seconds=(last['monotonic_ns']-first['monotonic_ns'])/1e9,
                callback_count=len(rows),
                discontinuities=[gap for gap in gaps if abs(gap['difference_seconds']) > .25],
                largest_difference=max(gaps, key=lambda gap: abs(gap['difference_seconds']), default=None))


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
    frame_bytes = 8 if result['format'] == 'f32le' else 4
    with (directory / 'generated.raw').open('rb') as source:
        source.seek(first['frames'] * frame_bytes)
        pcm = source.read(result['frames'] * frame_bytes)
    if len(pcm) != result['frames'] * frame_bytes:
        raise ValueError('generated PCM delivery is truncated')
    result['generated_raw_sha256'] = hashlib.sha256(pcm).hexdigest()
    samples = array.array('f' if result['format'] == 'f32le' else 'h', pcm)
    if sys.byteorder != 'little':
        samples.byteswap()
    if any(not math.isfinite(value) for value in samples):
        raise ValueError('nonfinite generated PCM sample')
    if result['format'] == 'f32le':
        samples = array.array('h', (round(max(-1, min(1, value)) * 32767) for value in samples))
        pcm = samples.tobytes()
        if sys.byteorder != 'little':
            encoded = array.array('h', samples); encoded.byteswap(); pcm = encoded.tobytes()
        result['wav_conversion'] = 'f32le_to_s16le_preview'
    result['peak'] = max(map(abs, samples), default=0)
    if not result['peak']:
        raise ValueError('silent generated PCM interval')
    if output.exists():
        raise ValueError('generated PCM output exists; preserve evidence')
    with wave.open(str(output), 'wb') as target:
        target.setparams((2, 2, first['rate'], 0, 'NONE', 'not compressed'))
        target.writeframes(pcm)
    return result
