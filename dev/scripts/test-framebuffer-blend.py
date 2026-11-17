#!/usr/bin/env python3
"""Check a matched, held-frame blend crop through public screenshots.

Usage: test-framebuffer-blend.py ORIGINAL BEFORE AFTER LEFT TOP RIGHT BOTTOM
Camera, texture frame, lighting and crop must match in all three screenshots.
"""
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
    finally:
        for image in images:
            image.close()
