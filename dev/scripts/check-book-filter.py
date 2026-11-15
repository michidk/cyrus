#!/usr/bin/env python3
"""Check complementary black/white book textures through production screenshots.

Prepare two cache copies with opposite checkerboard RGB values, retaining each
texture's dimensions and alpha. Render the same settled pose for each. Encoded
bilinear filtering preserves the complementary channel sum; sRGB filtering does
not. The rectangle must contain only textured, unlit book geometry.
"""
import argparse
import json
from pathlib import Path


def prepare(source, destination, inverse, horizontal=False):
    book = json.loads(Path(source).read_text())
    for texture in book["textures"]:
        rgba = texture["rgba"]
        width = texture["width"]
        for pixel in range(len(rgba) // 4):
            value = 255 * ((pixel % width + (0 if horizontal else pixel // width) + inverse) % 2)
            rgba[pixel * 4:pixel * 4 + 3] = [value] * 3
    Path(destination).write_text(json.dumps(book, separators=(",", ":")))


def compare(first, second, rectangle, four_bit=False):
    from PIL import Image
    a = Image.open(first).convert("RGB")
    b = Image.open(second).convert("RGB")
    left, top, right, bottom = rectangle
    if a.size != b.size or not (0 <= left < right <= a.width and
                                0 <= top < bottom <= a.height):
        raise ValueError("comparison requires equal rasters and an interior rectangle")
    a, b = a.crop(rectangle), b.crop(rectangle)
    sums = [x + y for x, y in zip(a.tobytes(), b.tobytes())]
    errors = [abs(value - 255) for value in sums]
    if not errors:
        raise ValueError("empty comparison rectangle")
    # Each framebuffer conversion may round by half a channel unit.
    result = {"channels": len(errors), "maximum_complement_error": max(errors),
              "mean_complement_error": sum(errors) / len(errors)}
    print(json.dumps(result))
    # Two integer interpolation stages can lose one unit apiece. Continuous
    # filtering instead has only the final framebuffer rounding. Keep the
    # acceptance models explicit rather than widening the continuous bound.
    rejected = any(value < 253 or value > 255 for value in sums) if four_bit else max(errors) > 1
    if rejected:
        raise ValueError("book filtering does not preserve encoded complementary channels")


def sample_grid(path, rectangle):
    from PIL import Image
    image = Image.open(path).convert("RGB")
    left, top, right, bottom = rectangle
    if not (0 <= left < right <= image.width and 0 <= top < bottom <= image.height):
        raise ValueError("sample-grid rectangle must lie inside the raster")
    data = image.crop(rectangle).tobytes()
    pixels = list(zip(data[::3], data[1::3], data[2::3]))
    # A horizontal black/white stripe samples floor(255*k/16), in either direction.
    levels = {255 * step // 16 for step in range(17)}
    wrong = sum(r != g or g != b or r not in levels for r, g, b in pixels)
    observed = {r for r, _, _ in pixels}
    result = {"pixels": len(pixels), "off_grid_pixels": wrong,
              "observed_levels": sorted(observed)}
    print(json.dumps(result))
    if wrong or not (observed - {0, 255}):
        raise ValueError("book stripe capture does not follow four-bit encoded filtering")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    fixture = sub.add_parser("prepare")
    fixture.add_argument("source")
    fixture.add_argument("destination")
    fixture.add_argument("--inverse", action="store_true")
    fixture.add_argument("--horizontal", action="store_true")
    check = sub.add_parser("compare")
    check.add_argument("first")
    check.add_argument("second")
    check.add_argument("--four-bit", action="store_true")
    check.add_argument("--rectangle", type=int, nargs=4, required=True,
                       metavar=("LEFT", "TOP", "RIGHT", "BOTTOM"))
    grid = sub.add_parser("sample-grid")
    grid.add_argument("image")
    grid.add_argument("--rectangle", type=int, nargs=4, required=True)
    args = parser.parse_args()
    if args.command == "prepare":
        prepare(args.source, args.destination, args.inverse, args.horizontal)
    elif args.command == "sample-grid":
        sample_grid(args.image, args.rectangle)
    else:
        compare(args.first, args.second, args.rectangle, args.four_bit)


if __name__ == "__main__":
    main()
