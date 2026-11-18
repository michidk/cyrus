#!/usr/bin/env python3
"""Assert held-torch brightening on matched public GPU screenshot readbacks.

Requires Pillow. Capture the same lit wall before and after inventory UseItem,
wait for lighting completion, and preserve camera and presentation receipts.
The default crop is the TAVERN wall used by issue #314, in 640x480 captures.
"""
import argparse
import json
from pathlib import Path


def measure(before, after, crop):
    from PIL import Image
    with Image.open(before) as dark, Image.open(after) as lit:
        if dark.size != lit.size:
            raise ValueError('capture dimensions differ')
        x0, y0, x1, y1 = crop
        if not (0 <= x0 < x1 <= dark.width and 0 <= y0 < y1 <= dark.height):
            raise ValueError('wall crop outside capture')
        a = list(dark.convert('RGB').crop(crop).getdata())
        b = list(lit.convert('RGB').crop(crop).getdata())
    differences = [y - x for p, q in zip(a, b) for x, y in zip(p, q)]
    return {'crop': crop, 'pixels': len(a),
            'mean_brightening': sum(differences) / len(differences),
            'mean_absolute_delta': sum(map(abs, differences)) / len(differences)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('before', type=Path)
    parser.add_argument('after', type=Path)
    parser.add_argument('--crop', type=int, nargs=4, default=[430, 100, 520, 350])
    parser.add_argument('--minimum-brightening', type=float, default=1.0)
    args = parser.parse_args()
    result = measure(args.before, args.after, args.crop)
    print(json.dumps(result, indent=2))
    if result['mean_brightening'] < args.minimum_brightening:
        raise SystemExit('burning held torch did not brighten the matched lit surface')


if __name__ == '__main__':
    main()
