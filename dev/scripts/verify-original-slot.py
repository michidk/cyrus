#!/usr/bin/env python3
"""Exercise native original-slot admission through the public book and control port."""
import argparse
import importlib.util
import json
import socket
import time
from pathlib import Path


def load_driver():
    path = Path(__file__).with_name("replay-chain.py")
    spec = importlib.util.spec_from_file_location("replay_chain", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


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


def verify(control, args):
    driver = load_driver()
    control.ready(timeout=args.timeout)
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
    clock = state["clock"]["continuation"]
    args.output.mkdir(parents=True, exist_ok=True)
    driver.capture_session(control, args.output / "admitted.session")
    for _ in range(3):
        observed = control.request({"type": "state"})
        assert observed["scripts"]["ordinary_updates"] == 0, observed["scripts"]
        assert observed["clock"]["continuation"] == clock, "observation advanced the clock"
    before = control.request({"type": "scripts", "limit": 4096})
    control.request({"type": "replay_frame", "frame_scale": args.frame_scale})
    after = control.request({"type": "state"})
    assert after["scripts"]["ordinary_updates"] == 1, after["scripts"]
    assert after["scripts"]["failed"] == 0, after["scripts"]
    driver.capture_session(control, args.output / "first-update.session")
    report = {"slot": args.slot, "world": args.world, "frame_scale": args.frame_scale,
              "admitted": state, "before_scripts": before, "first_update": after,
              "after_scripts": control.request({"type": "scripts", "limit": 4096})}
    (args.output / "result.json").write_text(json.dumps(report, indent=2) + "\n")
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
        verify(load_driver().Control(stream), args)


if __name__ == "__main__":
    main()
