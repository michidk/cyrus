#!/usr/bin/env python3
"""Check a matched, held-frame blend crop through public screenshots.

Usage: test-framebuffer-blend.py ORIGINAL BEFORE AFTER LEFT TOP RIGHT BOTTOM
Camera, texture frame, lighting and crop must match in all three screenshots.
Optional: append --control LEFT TOP RIGHT BOTTOM for an unchanged background crop.
"""
import sys
if __name__ == '__main__' and len(sys.argv) == 1:
    print('skipped: requires matched original/native framebuffer captures')
    raise SystemExit(0)

import sys
from PIL import Image, ImageChops, ImageStat


def check(reference, before, after, bounds):
    if reference.size != before.size or reference.size != after.size:
        raise AssertionError('matched framebuffer screenshots have different dimensions')
    left, top, right, bottom = bounds
    if not (0 <= left < right <= reference.width and 0 <= top < bottom <= reference.height):
        raise AssertionError('blend crop is empty or outside the framebuffer')
    crops = [image.convert('RGB').crop(bounds) for image in (reference, before, after)]
    errors = [sum(ImageStat.Stat(ImageChops.difference(crops[0], image)).sum)
              for image in crops[1:]]
    if errors[1] >= errors[0]:
        raise AssertionError(f'blend error did not improve: {errors[0]} -> {errors[1]}')
    pixels = (right - left) * (bottom - top)
    print(f'matched blend RGB MAE: {errors[0] / (pixels * 3):.6f} -> '
          f'{errors[1] / (pixels * 3):.6f} across {pixels} pixels')


if __name__ == '__main__':
    images = [Image.open(path) for path in sys.argv[1:4]]
    try:
        check(*images, tuple(map(int, sys.argv[4:8])))
        if len(sys.argv) > 8:
            if len(sys.argv) != 13 or sys.argv[8] != '--control':
                raise AssertionError('expected --control LEFT TOP RIGHT BOTTOM')
            bounds = tuple(map(int, sys.argv[9:13]))
            left, top, right, bottom = bounds
            if not (0 <= left < right <= images[1].width and 0 <= top < bottom <= images[1].height):
                raise AssertionError('control crop is empty or outside the framebuffer')
            if images[1].convert('RGB').crop(bounds).tobytes() != images[2].convert('RGB').crop(bounds).tobytes():
                raise AssertionError('unblended control crop changed')
            print(f'unchanged blend control: {(right-left)*(bottom-top)} pixels')
    finally:
        for image in images:
            image.close()
