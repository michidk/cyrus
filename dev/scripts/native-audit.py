#!/usr/bin/env python3
"""Map the native catalogue to the installed scripts and to cyrus's sources.

Reads exported `*.scripts` files (what `cyrus --export` and the scene cache write for each
world, by default `game-config/cache`). Each export lists the whole native catalogue
(`definition.functions`) and the calls of every installed actor. The audit reports

* natives an installed script calls that no Rust source mentions (a forgotten handler: exit 1),
* natives an installed script calls that no test code names (informational: a mention is
  necessary, not sufficient, and an indirect test may cover them),
* the catalogue natives that no installed script calls: not required by any original script
  contract, so they are outside the SOUP completion scope (docs/DIFFERENCES.md) and fail with a
  contextual error when a script reaches them.

Run it after exporting every world:

    dev/scripts/native-audit.py [--scripts game-config/cache] [--sources crates] [--list-unused] [--matrix]

`--matrix` lists every called native with its kind, the number of worlds calling it and the source
files that name it exactly or in snake case (a pointer for the per-native audit, not proof of behaviour).
`--behavior-matrix` instead joins explicitly reviewed evidence from native-behavior.json.
Unreviewed names are unaudited, never inferred missing or complete from source mentions.
`--instruction-matrix` reports reviewed VM forms and static decoded statement counts.
These counts do not establish execution or fidelity; absent forms remain visible.
`--rejection-matrix` groups retained decoder failures, separating fallback statements
from distinct actors. It does not establish current rejection or malformed original data.
`--operand-matrix` exposes reviewed portable operand scopes without requiring exports;
it does not claim original byte encoding or host property behavior from VM support.
`--local-targets` records explicit target/object uses and same-local write candidates,
with static-only limits; ordinary local reads and writes are not object consumers.
`--property-matrix` joins descriptor reviews with static decoded access modes; an
update is one read-modify-write occurrence, not separate observed reads and writes.
"""
import argparse
import collections
import glob
import json
import os
from pathlib import Path
import re
import sys


def collect(directory, pattern):
    catalogue = {}
    used = collections.defaultdict(set)
    files = sorted(glob.glob(os.path.join(directory, pattern)))
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


def matrix(root, catalogue, used):
    """Per called native: kind, calling worlds and the non-test source files naming it exactly."""
    texts = {}
    for directory, _, files in os.walk(root):
        for name in files:
            if name.endswith(".rs"):
                path = os.path.join(directory, name)
                with open(path, errors="ignore") as handle:
                    text = handle.read()
                texts[path] = text.split("#[cfg(test)]")[0]
    rows = []
    for name in sorted(used):
        snake = re.sub(r"(?<!^)(?=[A-Z])", "_", name).lower()
        pattern = re.compile(r"\b(?:" + re.escape(name) + "|" + re.escape(snake) + r")\b", re.IGNORECASE)
        files = sorted(
            os.path.relpath(path, root)
            for path, text in texts.items()
            if os.sep + "tests" + os.sep not in path and pattern.search(text)
        )
        rows.append((name, catalogue.get(name, ""), len(used[name]), files))
    return rows


def behavior_rows(path, catalogue, used, section="natives", name_field="native"):
    """Join manually reviewed scopes, preserving unknown coverage outside the audited slice."""
    data = json.loads(Path(path).read_text())
    reviewed = {}
    for row in data.get(section, []):
        name = row[name_field]
        if name in reviewed or name not in catalogue:
            raise ValueError(f"duplicate or unknown catalogue native: {name}")
        if row["status"] not in {"done", "partial", "missing"}:
            raise ValueError(f"invalid behavior status: {name}")
        if not isinstance(row["owner"], int) or row["owner"] <= 0:
            raise ValueError(f"owning issue required: {name}")
        for field in ("scope", "caller", "evidence", "limits"):
            if not row[field]:
                raise ValueError(f"{name}: {field} required")
        if not isinstance(row["evidence"], list) or not all(
            isinstance(reference, str) and reference for reference in row["evidence"]
        ):
            raise ValueError(f"{name}: evidence must be a nonempty list of references")
        if row["status"] == "done" and not all(
            row.get("acceptance", {}).get(host) for host in ("original", "native", "browser")
        ):
            raise ValueError(f"{name}: done requires original/native/browser acceptance references")
        reviewed[name] = row
    return [(name, reviewed.get(name)) for name in sorted(used)]



def instruction_usage(directory, pattern):
    """Count decoded statement forms only, never nested values or execution."""
    used = collections.defaultdict(set)
    counts = collections.Counter()
    for path in sorted(glob.glob(os.path.join(directory, pattern))):
        data = json.loads(Path(path).read_text())
        for actor in data.get("actors", {}).values():
            for step in actor.get("program", {}).get("steps", {}).values():
                name = step.get("operation", {}).get("type")
                if isinstance(name, str):
                    used[name].add(os.path.basename(path))
                    counts[name] += 1
    return used, counts



def rejection_rows(directory, pattern):
    """Group retained decode failures; repeated fallback cursors are not distinct actors."""
    groups = {}
    for path in sorted(glob.glob(os.path.join(directory, pattern))):
        data = json.loads(Path(path).read_text())
        for actor_name, actor in data.get("actors", {}).items():
            for step in actor.get("program", {}).get("steps", {}).values():
                operation = step.get("operation", {})
                if operation.get("type") != "unsupported":
                    continue
                reason = operation.get("reason", "")
                match = re.fullmatch(
                    r"script instruction \d+: function (.+) requires (\d+) arguments, found (\d+)", reason)
                key = ("arity", *match.groups()) if match else ("other", reason, "-", "-")
                group = groups.setdefault(key, {"statements": 0, "actors": set(), "files": set()})
                group["statements"] += 1
                group["actors"].add((os.path.basename(path), actor_name))
                group["files"].add(os.path.basename(path))
    return groups


def rejection_matrix(directory, pattern):
    groups = rejection_rows(directory, pattern)
    print("category\tfunction_or_reason\texpected\tfound\tstatements\tactors\texport_files\tclassification")
    for key, group in sorted(groups.items()):
        values = [*key, str(group["statements"]), str(len(group["actors"])),
                  str(len(group["files"])), "unknown: retained decode rejection; current execution not established"]
        print("\t".join(value.replace("\t", " ").replace("\n", " ") for value in values))

def instruction_matrix(path, directory, pattern):
    """Join reviewed VM forms with static export occurrences, including absent forms."""
    used, counts = instruction_usage(directory, pattern)
    if not counts:
        raise ValueError("no decoded instruction statements found")
    known = {"assign", "increment", "if", "goto", "gosub", "return", "end", "endint",
             "call", "object_call", "anchor", "yield", "script_rv", "task_stop",
             "task_pause", "task_resume", "unsupported"}
    rows = behavior_rows(path, known, known | set(used), "instructions", "instruction")
    print("instruction\texport_files\tstatements\tstatus\towner\tscope\tcaller\ttests\tevidence\tlimits")
    for name, row in rows:
        values = [name, str(len(used[name])), str(counts[name])]
        if row is None:
            values += ["unaudited", "-", "-", "-", "-", "-", "No behavior review recorded."]
        else:
            values += [row["status"], f'#{row["owner"]}', row["scope"], row["caller"],
                       "; ".join(row.get("tests", [])) or "-", "; ".join(row["evidence"]),
                       row["limits"]]
        print("\t".join(value.replace("\t", " ").replace("\n", " ") for value in values))


def operand_matrix(path):
    """Expose reviewed portable operand scopes without implying byte-encoding acceptance."""
    known = {"value.integer", "value.string", "value.read", "value.update", "value.call",
             "value.object_call", "place.flag", "place.local", "place.property", "object.me",
             "object.player", "object.camera", "object.named", "object.local", "formula"}
    rows = behavior_rows(path, known, known, "operands", "operand")
    print("operand\tstatus\towner\tscope\tcaller\ttests\tevidence\tlimits")
    for name, row in rows:
        if row is None:
            values = [name, "unaudited", "-", "-", "-", "-", "-", "No behavior review recorded."]
        else:
            values = [name, row["status"], f'#{row["owner"]}', row["scope"], row["caller"],
                      "; ".join(row.get("tests", [])) or "-", "; ".join(row["evidence"]), row["limits"]]
        print("\t".join(value.replace("\t", " ").replace("\n", " ") for value in values))


def local_target_census(directory, pattern):
    """Find explicit local targets and same-slot writes, without claiming dataflow or execution."""
    files = sorted(glob.glob(os.path.join(directory, pattern)))
    consumers, candidates = [], []
    writes = collections.Counter()
    first_calls = collections.Counter()
    for path in files:
        data = json.loads(Path(path).read_text())
        for actor_name, actor in data.get("actors", {}).items():
            steps = actor.get("program", {}).get("steps", {})
            indices = set()

            def walk(node, location):
                if isinstance(node, dict):
                    for key, value in node.items():
                        if key in {"target", "object"} and isinstance(value, dict) and value.get("type") == "local":
                            indices.add(value["index"])
                            consumers.append({"file": os.path.basename(path), "actor": actor_name,
                                              "path": location + "/" + key, "index": value["index"]})
                        walk(value, location + "/" + key)
                elif isinstance(node, list):
                    for index, value in enumerate(node):
                        walk(value, location + "/" + str(index))

            walk(steps, "steps")
            for offset, step in steps.items():
                operation = step.get("operation", {})
                destination = operation.get("destination", {})
                if destination.get("type") != "local":
                    continue
                writes[operation["type"]] += 1
                if operation["type"] == "assign":
                    first = operation["formula"]["first"]
                    writes["first." + first["type"]] += 1
                    if first["type"] in {"call", "object_call"}:
                        first_calls[first["call"]["function"]] += 1
                if destination["index"] in indices:
                    candidates.append({"file": os.path.basename(path), "actor": actor_name,
                                       "step": offset, "index": destination["index"],
                                       "operation": operation["type"]})
    if not files:
        raise ValueError("no script exports found")
    return {"evidence": "STATIC_ONLY", "export_files": len(files), "consumers": consumers,
            "same_slot_write_candidates": candidates, "local_writes": dict(sorted(writes.items())),
            "assignment_first_calls": dict(sorted(first_calls.items())),
            "limits": "Decoded exports only; unsupported bodies may hide operands. Same-slot writes are candidates, not reaching definitions or actor-reference producers. Absence does not establish live support or original reference fidelity."}


def property_accesses(directory, pattern):
    """Count explicit decoded reads/writes/updates; descriptors remain independent of execution."""
    names, counts = set(), collections.defaultdict(collections.Counter)
    files = sorted(glob.glob(os.path.join(directory, pattern)))
    for path in files:
        data = json.loads(Path(path).read_text())
        references = data.get("definition", {}).get("references", [])
        names.update(name for name in references if name)

        def access(place, mode):
            if place.get("type") == "property":
                reference = place["reference"]
                if not isinstance(reference, int) or not 0 <= reference < len(references):
                    raise ValueError(f"{path}: property reference {reference} absent")
                counts[references[reference]][mode] += 1

        def walk(node):
            if isinstance(node, dict):
                kind = node.get("type")
                if kind in {"assign", "increment"}:
                    access(node.get("destination", {}), "update" if kind == "increment" else "write")
                elif kind in {"read", "update"}:
                    access(node.get("place", {}), kind)
                for value in node.values():
                    walk(value)
            elif isinstance(node, list):
                for value in node:
                    walk(value)

        for actor in data.get("actors", {}).values():
            walk(actor.get("program", {}))
    if not names:
        raise ValueError("no installed reference descriptors found")
    return names, counts


def property_matrix(path, directory, pattern):
    names, counts = property_accesses(directory, pattern)
    rows = behavior_rows(path, names, names, "properties", "property")
    print("property\treads\twrites\tupdates\tstatus\towner\tscope\tcaller\ttests\tevidence\tlimits")
    for name, row in rows:
        values = [name, *(str(counts[name][mode]) for mode in ("read", "write", "update"))]
        if row is None:
            values += ["unaudited", "-", "-", "-", "-", "-", "No behavior review recorded."]
        else:
            values += [row["status"], f'#{row["owner"]}', row["scope"], row["caller"],
                       "; ".join(row.get("tests", [])) or "-", "; ".join(row["evidence"]), row["limits"]]
        print("\t".join(value.replace("\t", " ").replace("\n", " ") for value in values))

def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--scripts", default="game-config/cache")
    parser.add_argument("--sources", default="crates")
    parser.add_argument("--pattern", default="*.scripts", help="export file name pattern")
    parser.add_argument("--list-unused", action="store_true")
    parser.add_argument(
        "--matrix", action="store_true", help="print one tab-separated row per called native"
    )
    parser.add_argument(
        "--behavior-matrix", action="store_true", help="print reviewed behavior, owner and limits"
    )
    parser.add_argument(
        "--behavior", default=str(Path(__file__).with_name("native-behavior.json")),
        help="reviewed behavior catalogue used by --behavior-matrix",
    )
    parser.add_argument("--instruction-matrix", action="store_true",
                        help="review VM forms with static statement counts, not execution proof")
    parser.add_argument("--rejection-matrix", action="store_true",
                        help="group historical decode failures without classifying them as missing behavior")
    parser.add_argument("--operand-matrix", action="store_true",
                        help="review portable values, destinations, targets and formula scopes")
    parser.add_argument("--local-targets", action="store_true",
                        help="census explicit local targets and same-slot write candidates, never execution")
    parser.add_argument("--property-matrix", action="store_true",
                        help="join reviewed descriptors with static read/write/update counts")
    args = parser.parse_args()
    if args.property_matrix:
        try:
            property_matrix(args.behavior, args.scripts, args.pattern)
        except (OSError, ValueError, KeyError, TypeError) as error:
            print(f"invalid property audit: {error}", file=sys.stderr)
            return 1
        return 0
    if args.local_targets:
        try:
            print(json.dumps(local_target_census(args.scripts, args.pattern), indent=2))
        except (OSError, ValueError, KeyError, TypeError) as error:
            print(f"invalid local-target census: {error}", file=sys.stderr)
            return 1
        return 0
    if args.operand_matrix:
        try:
            operand_matrix(args.behavior)
        except (OSError, ValueError, KeyError, TypeError) as error:
            print(f"invalid operand audit: {error}", file=sys.stderr)
            return 1
        return 0
    if args.rejection_matrix:
        rejection_matrix(args.scripts, args.pattern)
        return 0
    if args.instruction_matrix:
        try:
            instruction_matrix(args.behavior, args.scripts, args.pattern)
        except (OSError, ValueError, KeyError, TypeError) as error:
            print(f"invalid instruction audit: {error}", file=sys.stderr)
            return 1
        return 0
    catalogue, used, count = collect(args.scripts, args.pattern)
    if not used:
        print(f"no natives found in {count} script exports under {args.scripts}")
        return 1
    if args.behavior_matrix:
        try:
            rows = behavior_rows(args.behavior, catalogue, used)
        except (OSError, ValueError, KeyError, TypeError) as error:
            print(f"invalid behavior catalogue: {error}", file=sys.stderr)
            return 1
        print("native\tkind\texport_files\tstatus\towner\tscope\tcaller\ttests\tevidence\tlimits")
        for name, row in rows:
            values = [name, catalogue.get(name, ""), str(len(used[name]))]
            if row is None:
                values += ["unaudited", "-", "-", "-", "-", "-", "No behavior review recorded."]
            else:
                values += [row["status"], f'#{row["owner"]}', row["scope"], row["caller"],
                           "; ".join(row.get("tests", [])) or "-",
                           "; ".join(row["evidence"]), row["limits"]]
            print("\t".join(value.replace("\t", " ").replace("\n", " ") for value in values))
        return 0
    if args.matrix:
        for name, kind, worlds, files in matrix(args.sources, catalogue, used):
            print(f"{name}\t{kind}\t{worlds}\t{', '.join(files) or '-'}")
        return 0
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
