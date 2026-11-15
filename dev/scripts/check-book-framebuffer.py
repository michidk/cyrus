#!/usr/bin/env python3
"""Prepare and check a constant-colour production book framebuffer capture."""
import argparse
import json
from pathlib import Path


# Published software-Voodoo framebuffer thresholds, indexed by framebuffer Y/X.
THRESHOLDS = ((0, 8, 2, 10), (12, 4, 14, 6), (3, 11, 1, 9), (15, 7, 13, 5))


def display(colour, x, y):
    result = []
    for channel, value in enumerate(colour):
        if channel == 1:
            stored = (4 * value - value // 16 + value // 64 + THRESHOLDS[y % 4][x % 4]) // 16
            result.append((stored << 2) | (stored >> 4))
        else:
            stored = (2 * value - value // 16 + value // 128 + THRESHOLDS[y % 4][x % 4]) // 16
            result.append((stored << 3) | (stored >> 2))
    return tuple(result)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest='command', required=True)
    fixture = sub.add_parser('prepare')
    fixture.add_argument('source')
    fixture.add_argument('destination')
    fixture.add_argument('--colour', type=int, nargs=3, required=True)
    check = sub.add_parser('check')
    check.add_argument('image')
    check.add_argument('--colour', type=int, nargs=3, required=True)
    check.add_argument('--rectangle', type=int, nargs=4, required=True)
    args = parser.parse_args()
    if any(value < 0 or value > 255 for value in args.colour):
        parser.error('colour channels must be in 0..255')
    if args.command == 'prepare':
        book = json.loads(Path(args.source).read_text())
        for texture in book['textures']:
            texture['rgba'] = (args.colour + [255]) * (texture['width'] * texture['height'])
        Path(args.destination).write_text(json.dumps(book, separators=(',', ':')))
        return
    from PIL import Image
    image = Image.open(args.image).convert('RGB')
    left, top, right, bottom = args.rectangle
    if not (0 <= left < right <= image.width and 0 <= top < bottom <= image.height):
        parser.error('rectangle must lie inside the capture')
    wrong = sum(image.getpixel((x, y)) != display(args.colour, x, y)
                for y in range(top, bottom) for x in range(left, right))
    print(json.dumps({'pixels': (right-left)*(bottom-top), 'mismatched_pixels': wrong}))
    if wrong:
        raise SystemExit('book framebuffer does not match ordered RGB565 storage')


if __name__ == '__main__':
    main()
