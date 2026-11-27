#!/usr/bin/env python3
"""Exercise native original-slot admission through the public book and control port."""
import argparse
import json
import socket
import time
from pathlib import Path

import replay_chain as driver


def slot_row(menu, slot, world):
    matches = [i for i, item in enumerate(menu["slots"])
               if item["slot"] == slot and item["source"] == {
                   "type": "original", "world_id": world}]
    assert len(matches) == 1, f"expected one original slot {slot} world {world}"
    rows = [i for i, entry in enumerate(menu["entries"]) if entry["action"] == 202]
    assert matches[0] < len(rows), "original slot is outside the first book page"
    return rows[matches[0]]


def wait_state(control, predicate, timeout):
    deadline = time.monotonic() + timeout
    while True:
        state = control.request({"type": "state"})
        message = (state.get("menu") or {}).get("unavailable") or ""
        assert not message.startswith(("original slot ", "original save ")), message
        if predicate(state):
            return state
        assert time.monotonic() < deadline, f"native slot admission timed out: {state}"
        time.sleep(0.1)


def inspect_scripts(control):
    """Retain every owner across the public inspector's bounded pages.

    An original world may place its selected player after hundreds of actors.
    Reject a concurrent replacement instead of composing observations from two
    different logical worlds.
    """
    result = control.request({'type': 'scripts', 'offset': 0, 'limit': 64})
    identity = ('generation', 'content_id', 'world', 'total')
    for offset in range(64, result['total'], 64):
        page = control.request({'type': 'scripts', 'offset': offset, 'limit': 64})
        assert all(page[key] == result[key] for key in identity), 'world changed during script inspection'
        result['actors'].extend(page['actors'])
    assert len(result['actors']) == result['total'], 'incomplete script owner census'
    return result


def load_slot(control, args):
    """Enter the book only after the selected player's ordinary entry completed.

    Scene/presentation readiness alone does not establish the unsaved controller
    owner retained by original-slot load. Keep this wait in the shared driver so
    admission probes cannot pause entry preparation by opening the book early.
    Replay-held launches must advance entry through acknowledged boundaries;
    Control.ready tolerates transient boundary unavailability during preparation.
    """
    entry = control.ready(timeout=args.timeout, advance=True)
    assert entry["scripts"].get("ordinary_updates", 0) > 0 or entry["scripts"].get("restored"), \
        "original-slot driver requires an established gameplay continuation"
    control.request({"type": "input", "key": "F3", "action": "tap"})
    state = wait_state(control, lambda s: (s.get("menu") or {}).get("ready")
                       and s["menu"]["page"] == 2 and s["menu"]["slots"], args.timeout)
    target = slot_row(state["menu"], args.slot, args.world)
    for _ in range(len(state["menu"]["entries"])):
        if state["menu"]["selected"] == target:
            break
        previous = state["menu"]["selected"]
        control.request({"type": "input", "key": "ArrowDown", "action": "tap"})
        state = wait_state(control, lambda s, selected=previous: s["menu"]["selected"] != selected, 30)
    assert state["menu"]["selected"] == target, "could not select the original slot"
    control.request({"type": "input", "key": "Enter", "action": "tap"})
    state = wait_state(control, lambda s: s["state"] == "InGame"
                       and s["scene_ready"] and (s.get("scripts") or {}).get("restored")
                       and s["scripts"]["world"] == args.world, args.timeout)
    assert state["scripts"]["ordinary_updates"] == 0, state["scripts"]
    assert state["scripts"]["failed"] == 0, state["scripts"]
    args.output.mkdir(parents=True, exist_ok=True)
    (args.output / "admission.json").write_text(json.dumps({"entry": entry, "admitted": state}, indent=2) + "\n")
    driver.capture_session(control, args.output / "admitted.session")
    return state


def verify(control, args):
    state = load_slot(control, args)
    clock = state["clock"]["continuation"]
    for _ in range(3):
        observed = control.request({"type": "state"})
        assert observed["scripts"]["ordinary_updates"] == 0, observed["scripts"]
        assert observed["clock"]["continuation"] == clock, "observation advanced the clock"
    before = inspect_scripts(control)
    control.request({"type": "replay_frame", "frame_scale": args.frame_scale})
    after = control.request({"type": "state"})
    report = {"slot": args.slot, "world": args.world, "frame_scale": args.frame_scale,
              "admitted": state, "before_scripts": before, "first_update": after,
              "after_scripts": inspect_scripts(control)}
    (args.output / "result.json").write_text(json.dumps(report, indent=2) + "\n")
    assert after["scripts"]["ordinary_updates"] == 1, after["scripts"]
    assert after["scripts"]["failed"] == 0, report["after_scripts"]
    driver.capture_session(control, args.output / "first-update.session")
    print(f"original slot {args.slot} world {args.world}: public admission and first update passed")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--port", type=int, required=True)
    parser.add_argument("--slot", type=int, required=True)
    parser.add_argument("--world", type=int, required=True)
    parser.add_argument("--frame-scale", type=int, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--timeout", type=float, default=1800)
    args = parser.parse_args()
    assert 0 <= args.frame_scale <= 2**32 - 1, "frame scale must be a measured Q8 value"
    with socket.create_connection(("127.0.0.1", args.port), timeout=args.timeout) as stream:
        verify(driver.Control(stream), args)


if __name__ == "__main__":
    main()
