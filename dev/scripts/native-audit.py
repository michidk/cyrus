#!/usr/bin/env python3
"""Map the native catalogue to the installed scripts and to cyrus's sources.

Reads exported `*.scripts` files (what `cyrus --export` and the scene cache write for each
world, by default `cyrus-config/cache`). Each export lists the whole native catalogue
(`definition.functions`) and the calls of every installed actor. The audit reports

* natives an installed script calls that no Rust source mentions (a forgotten handler: exit 1),
* natives an installed script calls that no test code names (informational: a mention is
  necessary, not sufficient, and an indirect test may cover them),
* the catalogue natives that no installed script calls: not required by any original script
  contract, so they are outside the SOUP completion scope (docs/DIFFERENCES.md) and fail with a
  contextual error when a script reaches them.

Run it after exporting every world:

    dev/scripts/native-audit.py [--scripts cyrus-config/cache] [--sources crates] [--list-unused]
"""
import argparse
import collections
import glob
import json
import os
import sys


def collect(directory):
    catalogue = {}
    used = collections.defaultdict(set)
    files = sorted(glob.glob(os.path.join(directory, "*.scripts")))
    for path in files:
        with open(path) as handle:
            try:
                data = json.load(handle)
            except ValueError:
                continue
        for function in data.get("definition", {}).get("functions", []):
            catalogue[function["name"]] = function.get("kind", "")
        world = os.path.basename(path)

        def walk(node):
            if isinstance(node, dict):
                function = node.get("function")
                if isinstance(function, str) and "kind" in node:
                    used[function].add(world)
                for value in node.values():
                    walk(value)
            elif isinstance(node, list):
                for value in node:
                    walk(value)

        for actor in data.get("actors", {}).values():
            walk(actor.get("program", {}))
    return catalogue, used, len(files)


def sources(root):
    code, tests = [], []
    for directory, _, files in os.walk(root):
        for name in files:
            if not name.endswith(".rs"):
                continue
            path = os.path.join(directory, name)
            with open(path, errors="ignore") as handle:
                text = handle.read().lower()
            code.append(text)
            if os.sep + "tests" + os.sep in path or name == "tests.rs":
                tests.append(text)
            elif "#[cfg(test)]" in text:
                tests.append(text[text.index("#[cfg(test)]"):])
    # Test names spell natives in snake case, so underscores are ignored when matching them.
    return "\n".join(code), "\n".join(tests).replace("_", "")


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--scripts", default="cyrus-config/cache")
    parser.add_argument("--sources", default="crates")
    parser.add_argument("--list-unused", action="store_true")
    args = parser.parse_args()
    catalogue, used, count = collect(args.scripts)
    if not used:
        print(f"no natives found in {count} script exports under {args.scripts}")
        return 1
    code, tests = sources(args.sources)
    unmentioned = sorted(name for name in used if name.lower() not in code)
    untested = sorted(name for name in used if name.lower() not in tests)
    unused = sorted(name for name in catalogue if name not in used)
    print(
        f"{len(catalogue)} catalogue natives; {len(used)} called by installed scripts "
        f"({count} script exports); {len(unused)} never called"
    )
    print(f"{len(unmentioned)} called natives with no mention in the sources")
    for name in unmentioned:
        print(f"  {name} (called in {len(used[name])} worlds)")
    print(f"{len(untested)} called natives that no test code names (informational)")
    if untested:
        print("  " + ", ".join(untested))
    if args.list_unused:
        print("never called by an installed script:")
        print("  " + ", ".join(unused))
    return 1 if unmentioned else 0


if __name__ == "__main__":
    sys.exit(main())
