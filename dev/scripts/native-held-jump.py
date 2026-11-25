#!/usr/bin/env python3
"""Capture bounded held input and compare exact ordinary boundaries to original receipts."""
import argparse
import hashlib
import json
from pathlib import Path

import native_trace as trace


def original_window(original, updates, original_key="space"):
    """Require complete original admission, including the explicit release edge."""
    if not 2 <= updates <= 32:
        raise ValueError("held input needs 2..32 updates; keep each capture below the service deadline")
    observations = original["observations"]
    # Acquisition may observe earlier released boundaries while waiting for the
    # requested cadence. The acknowledged press, not the first poll, owns time zero.
    first_frame = original["boundaries"][0]["frame"]
    prefix = [row for row in observations if row["frame"] < first_frame]
    if any(row["input_state"] != {original_key: False} for row in prefix):
        raise ValueError("original pre-admission observations contain held input")
    observations = observations[len(prefix):]
    steps = original["steps"]
    if (original["key"] != original_key or len(observations) != len(steps) + 1
            or len(steps) < updates or len(original["input_edges"]) != 1):
        raise ValueError("incomplete original held-input receipt")
    origin = observations[0]
    release = original["input_edges"][0]
    release_offset = release["frame"] - origin["frame"]
    if not 0 < release_offset < updates or release["input_state"] != {original_key: False}:
        raise ValueError("original release is missing or outside the capture")
    rate = original["normal_frame_rate"]
    if type(rate) is not int or rate <= 0:
        raise ValueError("invalid original clock units")
    elapsed = origin["clock_scale"]
    for i, row in enumerate(observations):
        if (row["frame"] != origin["frame"] + i or row["clock_scale"] != elapsed
                or row["input_state"] != {original_key: 0 < i <= release_offset}
                or row["player"].get("selected") is not True):
            raise ValueError("original boundary continuity or input acknowledgement failed")
        if i < len(steps):
            step = steps[i]
            if (step["frame"] != row["frame"] or type(step["frame_scale"]) is not int
                    or step["frame_scale"] <= 0
                    or step["input_state"] != {original_key: i < release_offset}):
                raise ValueError("original step admission failed")
            elapsed += step["frame_scale"]
    for edge, offset, pressed in ((original["boundaries"][0], 0, True),
                                  (release, release_offset, False),
                                  (original["boundaries"][-1], len(steps), False)):
        expected_edge = dict(observations[offset], input_state={original_key: pressed})
        if edge != expected_edge:
            raise ValueError("original input edge differs from its atomic observation")
    return observations[:updates + 1], steps[:updates], release_offset, rate


def jump_lifecycle(observations):
    """Admit the bounded standing-jump fixture, including airborne lift and landing."""
    events = {"queued_at": [], "falling_at": [], "landed_at": []}
    phase = "supported"
    lifted = False
    origin_y = observations[0]["player"]["position_q8"][1]
    for offset, row in enumerate(observations):
        player = row["player"]
        queued, falling = player["queued"], player["falling"]
        if phase == "supported":
            if falling:
                raise ValueError("jump lifecycle: fall without queued admission")
            if queued:
                events["queued_at"].append(offset)
                phase, lifted = "queued", False
        elif phase == "queued":
            if falling and not queued:
                events["falling_at"].append(offset)
                phase = "falling"
            elif not queued:
                raise ValueError("jump lifecycle: queue cleared without observed fall")
        elif queued:
            raise ValueError("jump lifecycle: new queue before observed landing")
        elif not falling:
            if not lifted:
                raise ValueError("jump lifecycle: no observed airborne lift")
            events["landed_at"].append(offset)
            phase = "supported"
        if phase != "supported" and player["position_q8"][1] < origin_y:
            lifted = True
    if phase != "supported" or not events["landed_at"]:
        raise ValueError("jump lifecycle: incomplete or absent queue/fall/landing cycle")
    return events


def compare(original, native, updates, *, key="Space", original_key="space", allow_displacement=False,
            require_jump_lifecycle=False):
    expected, steps, release, rate = original_window(original, updates, original_key)
    lifecycle = jump_lifecycle(expected) if require_jump_lifecycle else None
    boundaries = native["boundaries"]
    # Offset zero establishes the native baseline; offset one admits the press.
    if len(boundaries) != updates + 2:
        raise ValueError("incomplete native schedule; partial deadline evidence is not acceptance")
    previous = None
    origin_scale = boundaries[1]["elapsed_scale"]
    origin_update = boundaries[0]["update"]
    generation = boundaries[0]["generation"]
    for offset, boundary in enumerate(boundaries):
        action = (key, "press") if offset == 1 else (
            (key, "release") if offset == release + 1 else (None, None))
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
                or (not allow_displacement
                    and player["position_q8"] != expected[0]["player"]["position_q8"])):
            raise ValueError("capture must start supported and finish idle at the same position")
    return {"updates": updates, "release_after": release,
            "consumed_scale": sum(step["frame_scale"] for step in steps),
            "normal_frame_rate": rate, "matched_boundaries": len(expected),
            "max_position_error_q8": 0, "generation": generation, "jump_lifecycle": lifecycle,
            "key": key, "original_key": original_key, "allow_displacement": allow_displacement,
            "displacement_q8": [end - start for start, end in zip(
                expected[0]["player"]["position_q8"], expected[-1]["player"]["position_q8"])]}


def capture(addr, original, updates, *, key="Space", original_key="space"):
    _, _, release, _ = original_window(original, updates, original_key)
    trace.wait_live(addr, 180)
    events = [{"after_updates": i} for i in range(updates + 2)]
    events[1].update(key=key, action="press")
    events[release + 1].update(key=key, action="release")
    try:
        return trace.call(addr, {"type": "schedule_input", "events": events})
    finally:
        trace.call(addr, {"type": "input", "key": key, "action": "release"})


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--original", type=Path, required=True)
    source = parser.add_mutually_exclusive_group(required=True)
    source.add_argument("--addr")
    source.add_argument("--native", type=Path)
    parser.add_argument("--key", default="Space", help="native key from the installed binding")
    parser.add_argument("--original-key", default="space", help="key identity in the original receipt")
    parser.add_argument("--allow-displacement", action="store_true",
                        help="allow an idle endpoint at a different position; retain exact state/clock comparison")
    parser.add_argument("--require-jump-lifecycle", action="store_true",
                        help="require observed standing-jump queue, lift, fall and landing cycles")
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
              else capture(args.addr, original, args.updates,
                           key=args.key, original_key=args.original_key))
    raw.write_text(json.dumps(native, indent=2) + "\n")
    result = compare(original, native, args.updates, key=args.key,
                     original_key=args.original_key, allow_displacement=args.allow_displacement,
                     require_jump_lifecycle=args.require_jump_lifecycle)
    result["original_sha256"] = hashlib.sha256(args.original.read_bytes()).hexdigest()
    result["native_sha256"] = hashlib.sha256(raw.read_bytes()).hexdigest()
    args.out.write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result))


if __name__ == "__main__":
    main()
