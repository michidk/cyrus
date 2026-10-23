#!/usr/bin/env python3
"""Check that every native the installed scripts call is mentioned in cyrus's sources.

Reads exported `*.scripts` files (what `cyrus --export` and the scene cache write for each
world, by default `cyrus-config/cache`), collects the distinct native names with their call
kinds, and looks each name up, case-insensitively, in the Rust sources outside test directories.
A name with no mention is a native an installed script reaches that no handler could serve:
the audit exits 1 and lists it. A mention is necessary, not sufficient: it does not prove the
handler works, only that none was forgotten. Run it after exporting every world:

    dev/scripts/native-audit.py [--scripts cyrus-config/cache] [--sources crates]
"""
import argparse
import collections
import glob
import json
import os
import sys


def natives(directory):
    names = collections.defaultdict(set)
    files = sorted(glob.glob(os.path.join(directory, "*.scripts")))
    for path in files:
        with open(path) as handle:
            try:
                data = json.load(handle)
            except ValueError:
                continue

        def walk(node, world):
            if isinstance(node, dict):
                function = node.get("function")
                if isinstance(function, str) and "kind" in node:
                    names[(function, node["kind"])].add(world)
                for value in node.values():
                    walk(value, world)
            elif isinstance(node, list):
                for value in node:
                    walk(value, world)

        for actor in data.get("actors", {}).values():
            walk(actor.get("program", {}), os.path.basename(path))
    return names, len(files)


def sources(root):
    text = []
    for directory, _, files in os.walk(root):
        if os.sep + "tests" in directory + os.sep:
            continue
        for name in files:
            if name.endswith(".rs"):
                with open(os.path.join(directory, name), errors="ignore") as handle:
                    text.append(handle.read().lower())
    return "\n".join(text)


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--scripts", default="cyrus-config/cache")
    parser.add_argument("--sources", default="crates")
    args = parser.parse_args()
    used, count = natives(args.scripts)
    if not used:
        print(f"no natives found in {count} script exports under {args.scripts}")
        return 1
    text = sources(args.sources)
    missing = sorted(
        (function, kind, len(worlds))
        for (function, kind), worlds in used.items()
        if function.lower() not in text
    )
    print(f"{len(used)} distinct natives in {count} script exports, {len(missing)} unmentioned")
    for function, kind, worlds in missing:
        print(f"  {function} ({kind}), called in {worlds} worlds")
    return 1 if missing else 0


if __name__ == "__main__":
    sys.exit(main())
