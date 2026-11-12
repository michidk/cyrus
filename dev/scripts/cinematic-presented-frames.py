#!/usr/bin/env python3
"""Identify compositor pixels against an explicit installed movie export.

No published movie status is used as a presentation identity. The vertical
bilinear expansion matches the movie presentation contract. Every RGBA byte
must match; ambiguous images retain all matching frame numbers.
"""
import argparse
import hashlib
import json
import math
from pathlib import Path
from PIL import Image


def identify(surfaces, images, references):
    lookup = {}
    for number, image in references.items():
        digest = hashlib.sha256(image.convert('RGBA').tobytes()).hexdigest()
        lookup.setdefault(digest, []).append(number)
    result = []
    previous = -math.inf
    for surface in surfaces:
        clock = surface['metadata'].get('monotonicTimestamp')
        if not isinstance(clock, (int, float)) or not math.isfinite(clock) or clock < previous:
            raise ValueError('missing or unordered compositor swap clock')
        previous = clock
        image = images[surface['id']].convert('RGBA')
        if image.size != (640, 480):
            raise ValueError('compositor viewport differs from movie reference')
        digest = hashlib.sha256(image.tobytes()).hexdigest()
        result.append({'id': surface['id'], 'swap_seconds': clock,
                       'sha256': digest, 'frames': sorted(lookup.get(digest, []))})
    if len({n for row in result for n in row['frames']}) < 2:
        raise ValueError('no progressing installed movie images at compositor swaps')
    return result


def require_endpoint(presentations, minimum):
    if not any(number >= minimum for row in presentations for number in row['frames']):
        raise ValueError('requested installed frame has not reached a compositor swap')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('capture', type=Path)
    parser.add_argument('installed_window', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('--minimum-frame', type=int)
    args = parser.parse_args()
    window = json.loads((args.installed_window / 'window.json').read_text())
    references = {}
    for frame in window['frames']:
        data = (args.installed_window / frame['picture']).read_bytes()
        image = Image.frombytes('RGBA', (640, 240), data)
        references[frame['number']] = image.resize((640, 480), Image.Resampling.BILINEAR)
    surfaces = json.loads((args.capture / 'surfaces.json').read_text())
    images = {f['id']: Image.open(args.capture / f"surface-{f['id']}.png") for f in surfaces}
    result = identify(surfaces, images, references)
    if args.minimum_frame is not None:
        require_endpoint(result, args.minimum_frame)
    with args.output.open('x') as output:
        output.write(json.dumps({'frame_micros': window['metadata']['frame_micros'],
                                 'presentations': result}, indent=2) + '\n')
    print(json.dumps({'swaps': len(result), 'identified': sum(bool(r['frames']) for r in result)}))


if __name__ == '__main__':
    main()
