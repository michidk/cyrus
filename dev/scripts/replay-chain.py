#!/usr/bin/env python3
"""Run input-only chapter fixtures in one headless New Game session."""
import argparse
import fcntl
import json
import math
import os
from pathlib import Path
import socket
import subprocess
import time


def equal(actual, expected, path):
    if isinstance(expected, dict):
        for key, value in expected.items():
            assert key in actual, f"{path}/{key}: missing"
            equal(actual[key], value, f"{path}/{key}")
    else:
        assert actual == expected, f"{path}: expected {expected!r}, got {actual!r}"


def checkpoint(state, scripts, expected):
    """Compare public control observations; positions use original map Q8 units."""
    equal(scripts["world"], expected["world"], "world")
    equal(flags_by_name(scripts), expected["flags"], "flags")
    equal(scripts["globals"], expected["globals"], "globals")
    equal(state["script_hud"]["inventory"]["counts"], expected["inventory"], "inventory")
    position = scripts["player_position_q8"]
    target = expected["position_q8"]
    residual = math.dist(position, target["value"])
    assert residual <= target["tolerance"], (
        f"position_q8: distance {residual:.3f} exceeds {target['tolerance']}; got {position}")
    return residual


def validate(fixture):
    """Accept timing, keyboard events and complete checkpoint expectations only."""
    assert set(fixture) == {"version", "step_micros", "steps"}, "unexpected fixture fields"
    assert fixture["version"] == 1, "unsupported fixture version"
    assert type(fixture["step_micros"]) is int and fixture["step_micros"] > 0, "invalid step_micros"
    assert fixture["steps"], "empty fixture"
    assert validate_steps(fixture["steps"]), "fixture has no checkpoints"


def validate_steps(steps, depth=0):
    assert depth <= 1, "nested input repeats are unsupported"
    checkpoints = 0
    for step in steps:
        if "checkpoint" in step:
            assert set(step) == {"checkpoint"}, "checkpoint cannot contain commands"
            expected = step["checkpoint"]
            assert set(expected) == {"world", "flags", "globals", "inventory", "position_q8"}, "incomplete checkpoint"
            assert expected["flags"] and expected["inventory"], "checkpoint needs flags and inventory"
            position = expected["position_q8"]
            assert len(position["value"]) == 3 and 0 <= position["tolerance"] < float("inf"), "invalid position tolerance"
            checkpoints += 1
        elif "steer" in step:
            assert set(step) == {"steer"}, "steering cannot contain other commands"
            target = step["steer"]
            assert {"world", "position_q8", "tolerance", "limit"} <= set(target) <= {"world", "position_q8", "tolerance", "limit", "transition_to"}, "incomplete steering target"
            assert len(target["position_q8"]) == 3 and all(math.isfinite(v) for v in target["position_q8"]), "invalid steering position"
            assert 0 < target["tolerance"] < float("inf"), "invalid steering tolerance"
            assert type(target["limit"]) is int and 1 <= target["limit"] <= 2000, "invalid steering limit"
        elif "modal_input" in step:
            assert set(step) == {"modal_input"}, "modal input cannot contain commands"
            assert set(step["modal_input"]) == {"key", "world"}, "modal input needs key and world"
        elif "repeat" in step:
            assert set(step) == {"repeat"}, "repeat cannot contain commands"
            repeat = step["repeat"]
            assert set(repeat) == {"until", "limit", "steps"}, "repeat needs a condition and limit"
            assert 1 <= repeat["limit"] <= 100, "invalid repetition limit"
            assert repeat["until"] and set(repeat["until"]) <= {"world", "flags"}, "invalid repeat condition"
            assert repeat["steps"], "empty repeat"
            assert all("input" in child for child in repeat["steps"]), "repeat bodies need keyboard schedules"
            validate_steps(repeat["steps"], depth + 1)
        else:
            assert "input" in step and set(step) <= {"input", "transition_to", "when_health_below"}, "only keyboard inputs are supported"
            assert step["input"], "empty input schedule"
    return checkpoints


def flags_by_name(scripts):
    return {f["definition"]["name"]: {"value": f["value"], "latched": f["latched"]}
            for f in scripts["flags"]["flags"]}


def reached(control, expected):
    state = control.ready(examine_open=None)
    assert state["hud"]["health"] > 0, "player died during replay"
    if "world" in expected and state["scripts"]["world"] != expected["world"]:
        return False
    if "flags" in expected:
        scripts = control.request({"type": "scripts", "offset": 0, "limit": 1})
        try:
            equal(flags_by_name(scripts), expected["flags"], "flags")
        except AssertionError:
            return False
    return True


def play_inputs(control, step):
    if "when_health_below" in step:
        state = control.ready()
        if state["hud"]["health"] >= step["when_health_below"]:
            return False
    response = control.response({"type": "schedule_input", "events": step["input"]})
    if not response["ok"]:
        assert "transition_to" in step, response.get("error", response)
        assert "ordinary input boundary" in response.get("error", ""), response
        control.ready(step["transition_to"], examine_open=None)
        return True
    return False


def repeat_inputs(control, repeat):
    for iteration in range(repeat["limit"]):
        if reached(control, repeat["until"]):
            return
        transitioned = False
        for step in repeat["steps"]:
            assert "input" in step, "repeat bodies contain keyboard schedules only"
            if play_inputs(control, step):
                transitioned = True
                break
        print(f"input repetition {iteration + 1}/{repeat['limit']} complete", flush=True)
        if transitioned:
            assert reached(control, repeat["until"]), "unexpected transition"
            return
    assert reached(control, repeat["until"]), f"input repetition limit exhausted: {repeat['until']}"


def steer(control, target):
    """Reach an X/Z waypoint with bounded real key input, never state mutation.

    Q8 map coordinates and 2048-unit headings match the public control API.
    Heading zero faces +Z; positive headings turn toward -X. Vertical position
    remains a checkpoint concern because walking cannot choose the floor height.
    """
    for iteration in range(target["limit"] + 1):
        state = control.ready()
        if state["scripts"]["world"] != target["world"]:
            assert state["scripts"]["world"] == target.get("transition_to"), "unexpected world during steering"
            return
        assert state["hud"]["health"] > 0, "player died during steering"
        scripts = control.request({"type": "scripts", "offset": 0, "limit": 1})
        assert scripts["world"] == target["world"], "world changed during steering observation"
        position = scripts["player_position_q8"]
        dx = target["position_q8"][0] - position[0]
        dz = target["position_q8"][2] - position[2]
        distance = math.hypot(dx, dz)
        if distance <= target["tolerance"]:
            print(f"steering reached waypoint after {iteration} inputs, residual {distance:.3f} Q8", flush=True)
            return {"state": state, "scripts": scripts}
        if iteration == target["limit"]:
            break
        bearing = (-math.atan2(dx, dz) * 2048 / math.tau) % 2048
        error = (bearing - state["hud"]["heading"] + 1024) % 2048 - 1024
        # Short schedules resample heading after each turn and avoid strafing.
        turning = abs(error) > 48
        keys = ["ArrowLeft" if error > 0 else "ArrowRight"] if turning else ["ShiftLeft", "ArrowUp"]
        held = min(4, max(1, int(abs(error) / 64))) if turning else min(4, max(1, int(distance / 4096)))
        events = ([{"after_updates": 0, "key": key, "action": "press"} for key in keys]
                  + [{"after_updates": held, "key": key, "action": "release"} for key in keys])
        transitioned = play_inputs(control, {"input": events, **(
            {"transition_to": target["transition_to"]} if "transition_to" in target else {})})
        if transitioned:
            return
        if (iteration + 1) % 20 == 0:
            print(f"steering input {iteration + 1}/{target['limit']}, residual {distance:.3f} Q8", flush=True)
    raise AssertionError(f"steering limit exhausted: target {target['position_q8']}, last position {position}, residual {distance:.3f} Q8")


class Control:
    def __init__(self, stream):
        self.stream = stream
        self.file = stream.makefile("rw")
        self.serial = 0

    def response(self, command):
        self.serial += 1
        self.file.write(json.dumps({"id": self.serial, "cmd": command}) + "\n")
        self.file.flush()
        line = self.file.readline()
        assert line, "game closed control connection"
        response = json.loads(line)
        return response

    def request(self, command):
        response = self.response(command)
        assert response["ok"], response.get("error", response)
        return response["result"]

    def ready(self, world=None, timeout=600, examine_open=False, min_updates=1):
        deadline = time.monotonic() + timeout
        while True:
            state = self.request({"type": "state"})
            scripts = state.get("scripts") or {}
            assert scripts.get("failed", 0) == 0, f"script failures: {scripts}"
            if (state["state"] == "InGame" and state["scene_ready"] and scripts.get("world") is not None and scripts.get("ordinary_updates", 0) >= min_updates
                    and (world is None or scripts.get("world") == world)
                    and (examine_open is None or state["examine"]["open"] == examine_open)):
                return state
            assert time.monotonic() < deadline, f"world {world} timeout: {scripts}"
            time.sleep(0.05)


def run(args):
    fixtures = []
    for path in args.fixtures:
        try:
            fixture = json.loads(path.read_text())
            validate(fixture)
        except (AssertionError, KeyError, TypeError, ValueError) as error:
            raise ValueError(f"{path}: {error}") from error
        fixtures.append(fixture)
    assert fixtures, "no chapter fixtures"
    timing = fixtures[0]["step_micros"]
    assert isinstance(timing, int) and timing > 0, "step_micros must be a positive integer"
    assert all(f["step_micros"] == timing for f in fixtures), "chain timing must agree"
    args.config.mkdir(parents=True, exist_ok=True)
    with (args.config / "replay.lock").open("w") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        with socket.socket() as probe:
            probe.bind(("127.0.0.1", 0))
            port = probe.getsockname()[1]
        env = dict(os.environ, CYRUS_CONFIG_DIR=str(args.config.resolve()))
        with (args.config / "process.log").open("w") as log:
            process = subprocess.Popen([
                str(args.binary.resolve()), "--headless", "--new-game", "--skip-movies",
                "--replay-step-micros", str(timing),
                "--volume", "0", "--control", f"127.0.0.1:{port}",
                "--install", str(args.install.resolve()),
            ], env=env, stdout=log, stderr=log)
            try:
                deadline = time.monotonic() + 60
                while True:
                    assert process.poll() is None, f"game exited; see {args.config}/process.log"
                    try:
                        stream = socket.create_connection(("127.0.0.1", port), timeout=40)
                        break
                    except ConnectionRefusedError:
                        assert time.monotonic() < deadline, "control connection timeout"
                        time.sleep(0.1)
                with stream:
                    control = Control(stream)
                    control.ready()
                    for path, fixture in zip(args.fixtures, fixtures):
                        assert fixture["version"] == 1, f"{path}: unsupported version"
                        transitioned = False
                        observation = None
                        for index, step in enumerate(fixture["steps"]):
                            if "checkpoint" not in step:
                                observation = None
                            if "input" in step and not transitioned:
                                transitioned = play_inputs(control, step)
                            if "steer" in step:
                                observation = steer(control, step["steer"])
                            if "repeat" in step:
                                repeat_inputs(control, step["repeat"])
                            if "modal_input" in step:
                                modal = step["modal_input"]
                                before = control.ready(modal["world"], examine_open=True)
                                control.request({"type": "input", "key": modal["key"], "action": "tap"})
                                control.ready(modal["world"], min_updates=before["scripts"]["ordinary_updates"] + 1)
                            if "checkpoint" in step:
                                expected = step["checkpoint"]
                                if observation is None:
                                    state = control.ready(expected["world"])
                                    scripts = control.request({"type": "scripts", "offset": 0, "limit": 1})
                                    observation = {"state": state, "scripts": scripts}
                                state, scripts = observation["state"], observation["scripts"]
                                # Inventory and world must refer to the same prepared generation.
                                assert state["scripts"]["world"] == scripts["world"], "world changed during checkpoint"
                                (args.config / f"{path.stem}-{index}.json").write_text(json.dumps(observation, indent=2))
                                residual = checkpoint(state, scripts, expected)
                                print(f"{path.name}:{index}: checkpoint passed, position residual {residual:.3f} Q8", flush=True)
                                observation = None
                    if args.keep_running:
                        address = f"127.0.0.1:{port}"
                        (args.config / "control-address").write_text(address + "\n")
                        print(f"chain passed; control remains available at {address}", flush=True)
                        while process.poll() is None:
                            time.sleep(0.25)
                    else:
                        control.request({"type": "quit"})
            finally:
                process.terminate()
                try:
                    process.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--keep-running", action="store_true", help="retain the passing session for recording the next chapter")
    parser.add_argument("fixtures", type=Path, nargs="+")
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--install", type=Path, required=True)
    parser.add_argument("--config", type=Path, required=True)
    args = parser.parse_args()
    try:
        run(args)
    except (AssertionError, KeyError, TypeError, OSError, ValueError) as error:
        parser.exit(1, f"replay-chain: {error}\n")


if __name__ == "__main__":
    main()
