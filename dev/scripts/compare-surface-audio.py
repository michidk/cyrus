#!/usr/bin/env python3
"""Compare surface_audio output with an independently recorded original report."""
import argparse
import json
from pathlib import Path


def validate(report, original=False):
    if not isinstance(report, dict):
        raise ValueError("report: expected object")
    if report.get("version") != 1:
        raise ValueError("report.version: expected 1")
    if original:
        evidence = report.get("evidence", {})
        if not isinstance(evidence, dict) or evidence.get("kind") != "original-runtime" or not evidence.get("record"):
            raise ValueError("original.evidence: independent runtime record required")
    cases = report.get("cases")
    if not isinstance(cases, list) or not cases:
        raise ValueError("report.cases: nonempty list required")
    names = set()
    plays = 0
    starts = 0
    for case in cases:
        name = case["name"]
        if name in names:
            raise ValueError(f"case {name}: duplicate name")
        names.add(name)
        if not case["steps"]:
            raise ValueError(f"case {name}: no steps")
        for index, step in enumerate(case["steps"]):
            for event in step["events"]:
                if event["kind"] not in ("play", "stop", "forget"):
                    raise ValueError(f"case {name} step {index}: unknown event kind")
                if event["kind"] == "play":
                    plays += 1
                    for start in event["started"]:
                        if len(start["target_candidates"]) != 1:
                            raise ValueError(f"case {name} step {index}: ambiguous PCM target")
                        starts += 1
    if not plays:
        raise ValueError("report: no animation Play observations")
    if not starts:
        raise ValueError("report: no actual playback starts")


def compare(actual, original):
    validate(actual)
    validate(original, original=True)
    for field in ("world", "cases"):
        difference(actual[field], original[field], field)


def difference(actual, expected, path):
    if type(actual) is not type(expected):
        raise ValueError(f"{path}: type differs")
    if isinstance(actual, dict):
        if actual.keys() != expected.keys():
            raise ValueError(f"{path}: fields differ")
        for key in actual:
            difference(actual[key], expected[key], f"{path}.{key}")
    elif isinstance(actual, list):
        if len(actual) != len(expected):
            raise ValueError(f"{path}: observation count {len(actual)} != {len(expected)}")
        for index, (left, right) in enumerate(zip(actual, expected)):
            difference(left, right, f"{path}[{index}]")
    elif actual != expected:
        raise ValueError(f"{path}: {actual!r} != {expected!r}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("actual", type=Path)
    parser.add_argument("original", type=Path)
    args = parser.parse_args()
    try:
        compare(json.loads(args.actual.read_text()), json.loads(args.original.read_text()))
    except (ValueError, KeyError, TypeError, OSError) as error:
        parser.exit(1, f"surface audio comparison failed: {error}\n")
    print("surface audio observations match")


if __name__ == "__main__":
    main()
