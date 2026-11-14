#!/usr/bin/env python3
"""Capture a bounded held jump and compare exact ordinary boundaries to original receipts."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path

spec = importlib.util.spec_from_file_location("native_trace", Path(__file__).with_name("native-trace.py"))
trace = importlib.util.module_from_spec(spec)
spec.loader.exec_module(trace)


def original_window(original, updates):
    """Require complete original admission, including the explicit release edge."""
    if not 2 <= updates <= 32:
        raise ValueError("held jump needs 2..32 updates; keep each capture below the service deadline")
    observations = original["observations"]
    steps = original["steps"]
    if (original["key"] != "space" or len(observations) != len(steps) + 1
            or len(steps) < updates or len(original["input_edges"]) != 1):
        raise ValueError("incomplete original held-jump receipt")
    origin = observations[0]
    release = original["input_edges"][0]
    release_offset = release["frame"] - origin["frame"]
    if not 0 < release_offset < updates or release["input_state"] != {"space": False}:
        raise ValueError("original release is missing or outside the capture")
    rate = original["normal_frame_rate"]
    if type(rate) is not int or rate <= 0:
        raise ValueError("invalid original clock units")
    elapsed = origin["clock_scale"]
    for i, row in enumerate(observations):
        if (row["frame"] != origin["frame"] + i or row["clock_scale"] != elapsed
                or row["input_state"] != {"space": 0 < i <= release_offset}
                or row["player"].get("selected") is not True):
            raise ValueError("original boundary continuity or input acknowledgement failed")
        if i < len(steps):
            step = steps[i]
            if (step["frame"] != row["frame"] or type(step["frame_scale"]) is not int
                    or step["frame_scale"] <= 0
                    or step["input_state"] != {"space": i < release_offset}):
                raise ValueError("original step admission failed")
            elapsed += step["frame_scale"]
    for edge, offset, pressed in ((original["boundaries"][0], 0, True),
                                  (release, release_offset, False),
                                  (original["boundaries"][-1], len(steps), False)):
        expected_edge = dict(observations[offset], input_state={"space": pressed})
        if edge != expected_edge:
            raise ValueError("original input edge differs from its atomic observation")
    return observations[:updates + 1], steps[:updates], release_offset, rate


def compare(original, native, updates):
    expected, steps, release, rate = original_window(original, updates)
    boundaries = native["boundaries"]
    # Offset zero establishes the native baseline; offset one admits the press.
    if len(boundaries) != updates + 2:
        raise ValueError("incomplete native schedule; partial deadline evidence is not acceptance")
    previous = None
    origin_scale = boundaries[1]["elapsed_scale"]
    origin_update = boundaries[0]["update"]
    generation = boundaries[0]["generation"]
    for offset, boundary in enumerate(boundaries):
        action = ("Space", "press") if offset == 1 else (
            ("Space", "release") if offset == release + 1 else (None, None))
        row = trace.boundary_sample(boundary, "held_jump", action)
        if (boundary["after_updates"] != offset or boundary["update"] != origin_update + offset
                or boundary["generation"] != generation or boundary["normal_frame_rate"] != rate
                or boundary["game_seconds"] != boundary["elapsed_scale"] / (256 * rate)):
            raise ValueError("native boundary identity or clock units changed")
        if offset == 0 and not row["stable"]:
            raise ValueError("native baseline has no stable support")
        if previous:
            trace.validate_continuation(previous, row)
        previous = row
        counterpart = expected[max(0, offset - 1)]
        player = counterpart["player"]
        actor = boundary["player"]["actors"][0]
        if (actor["spatial"]["effective"]["position"] != player["position_q8"]
                or actor["spatial"]["effective"]["angles"][1] != player["yaw"]
                or row["group"] != player["group"]
                or any(row[name] != player[name] for name in
                       ("health", "falling", "queued", "sliding", "living", "rope_attached"))):
            raise ValueError(f"held-jump state diverged at native offset {offset}")
        if offset and boundary["elapsed_scale"] - origin_scale != (
                counterpart["clock_scale"] - expected[0]["clock_scale"]):
            raise ValueError(f"held-jump consumed scale diverged at native offset {offset}")
    for player in (expected[0]["player"], expected[-1]["player"]):
        if (player["group"] != 0 or player["health"] <= 0
                or any(player[name] for name in ("falling", "queued", "sliding", "living", "rope_attached"))
                or player["position_q8"] != expected[0]["player"]["position_q8"]):
            raise ValueError("capture must start supported and finish idle at the same position")
    return {"updates": updates, "release_after": release,
            "consumed_scale": sum(step["frame_scale"] for step in steps),
            "normal_frame_rate": rate, "matched_boundaries": len(expected),
            "max_position_error_q8": 0, "generation": generation}


def capture(addr, original, updates):
    _, _, release, _ = original_window(original, updates)
    trace.wait_live(addr, 180)
    events = [{"after_updates": i} for i in range(updates + 2)]
    events[1].update(key="Space", action="press")
    events[release + 1].update(key="Space", action="release")
    try:
        return trace.call(addr, {"type": "schedule_input", "events": events})
    finally:
        trace.call(addr, {"type": "input", "key": "Space", "action": "release"})


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--original", type=Path, required=True)
    source = parser.add_mutually_exclusive_group(required=True)
    source.add_argument("--addr")
    source.add_argument("--native", type=Path)
    parser.add_argument("--updates", type=int, default=32)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    raw = args.out.with_suffix(".raw.json")
    inputs = {args.original.resolve()}
    if args.native:
        inputs.add(args.native.resolve())
    if args.out.resolve() in inputs or raw.resolve() in inputs or raw == args.out:
        parser.error("output paths must differ from both source receipts and each other")
    # Never leave an earlier accepted summary after a rejected new attempt.
    args.out.unlink(missing_ok=True)
    original = json.loads(args.original.read_text())
    native = (json.loads(args.native.read_text()) if args.native
              else capture(args.addr, original, args.updates))
    raw.write_text(json.dumps(native, indent=2) + "\n")
    result = compare(original, native, args.updates)
    result["original_sha256"] = hashlib.sha256(args.original.read_bytes()).hexdigest()
    result["native_sha256"] = hashlib.sha256(raw.read_bytes()).hexdigest()
    args.out.write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result))


if __name__ == "__main__":
    main()
