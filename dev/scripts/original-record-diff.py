#!/usr/bin/env python3
"""Diff two dumps of one original-game actor record (and its attribute block).

A dump is JSON with hex strings `rec` (the actor record) and `attrs` (its attribute bytes), as
written by the ignored in-container dumper. The dumper holds executable addresses and stays in
`$CYRUS_MAGIC/helpers`; this tool only compares bytes. Use it to find which byte a console command or a
scripted step changes (for example the hidden state after `item add,79`).

    original-record-diff.py before.json after.json [--ignore 0xcb-0xd6,0x124-0x127]
"""
import argparse
import json
import sys


def parse_ranges(text):
    """`0xcb-0xd6,0x124` -> inclusive (start, end) pairs; offsets may be decimal or hex."""
    ranges = []
    for part in filter(None, (text or "").split(",")):
        low, _, high = part.partition("-")
        start = int(low, 0)
        ranges.append((start, int(high, 0) if high else start))
    return ranges


def changed_bytes(before_hex, after_hex, ignore=()):
    """Offsets whose byte differs, as (offset, before, after); a length difference is reported
    as a trailing run of changes against 0 so it cannot hide."""
    before, after = bytes.fromhex(before_hex), bytes.fromhex(after_hex)
    rows = []
    for offset in range(max(len(before), len(after))):
        if any(low <= offset <= high for low, high in ignore):
            continue
        old = before[offset] if offset < len(before) else None
        new = after[offset] if offset < len(after) else None
        if old != new:
            rows.append((offset, old, new))
    return rows


def diff_dumps(before, after, ignore=()):
    """Changed bytes for the record and the attribute block of two dumps."""
    if before is None or after is None:
        return {"error": "a dump is missing (the actor was not in the list)"}
    return {key: changed_bytes(before[key], after[key], ignore if key == "rec" else ())
            for key in ("rec", "attrs")}


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("before")
    parser.add_argument("after")
    parser.add_argument("--ignore", default="", help="record offsets to skip, e.g. position and yaw")
    args = parser.parse_args()
    result = diff_dumps(json.load(open(args.before)), json.load(open(args.after)), parse_ranges(args.ignore))
    if "error" in result:
        sys.exit(result["error"])
    for key, rows in result.items():
        print(f"{key}: {len(rows)} changed bytes")
        for offset, old, new in rows:
            print(f"  +{offset:#05x} {old if old is None else f'{old:#04x}'} -> {new if new is None else f'{new:#04x}'}")


if __name__ == "__main__":
    main()
