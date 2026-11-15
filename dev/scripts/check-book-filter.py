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


def prepare(source, destination, inverse):
    book = json.loads(Path(source).read_text())
    for texture in book["textures"]:
        rgba = texture["rgba"]
        width = texture["width"]
        for pixel in range(len(rgba) // 4):
            value = 255 * ((pixel % width + pixel // width + inverse) % 2)
            rgba[pixel * 4:pixel * 4 + 3] = [value] * 3
    Path(destination).write_text(json.dumps(book, separators=(",", ":")))


def compare(first, second, rectangle):
    from PIL import Image
    a = Image.open(first).convert("RGB")
    b = Image.open(second).convert("RGB")
    left, top, right, bottom = rectangle
    if a.size != b.size or not (0 <= left < right <= a.width and
                                0 <= top < bottom <= a.height):
        raise ValueError("comparison requires equal rasters and an interior rectangle")
    a, b = a.crop(rectangle), b.crop(rectangle)
    errors = [abs(x + y - 255) for x, y in zip(a.tobytes(), b.tobytes())]
    if not errors:
        raise ValueError("empty comparison rectangle")
    # Each framebuffer conversion may round by half a channel unit.
    result = {"channels": len(errors), "maximum_complement_error": max(errors),
              "mean_complement_error": sum(errors) / len(errors)}
    print(json.dumps(result))
    if max(errors) > 1:
        raise ValueError("book filtering does not preserve encoded complementary channels")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    fixture = sub.add_parser("prepare")
    fixture.add_argument("source")
    fixture.add_argument("destination")
    fixture.add_argument("--inverse", action="store_true")
    check = sub.add_parser("compare")
    check.add_argument("first")
    check.add_argument("second")
    check.add_argument("--rectangle", type=int, nargs=4, required=True,
                       metavar=("LEFT", "TOP", "RIGHT", "BOTTOM"))
    args = parser.parse_args()
    if args.command == "prepare":
        prepare(args.source, args.destination, args.inverse)
    else:
        compare(args.first, args.second, args.rectangle)


if __name__ == "__main__":
    main()
