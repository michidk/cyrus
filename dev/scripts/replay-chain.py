#!/usr/bin/env python3
"""Run input-only chapter fixtures in one headless New Game session."""
import argparse
import fcntl
import json
import math
import re
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
    equal(state["scripts"]["failed"], 0, "scripts/failed")
    equal(scripts["world"], expected["world"], "world")
    equal(flags_by_name(scripts), expected["flags"], "flags")
    equal(scripts["globals"], expected["globals"], "globals")
    equal(state["script_hud"]["inventory"]["counts"], expected["inventory"], "inventory")
    if "health" in expected:
        equal(state["hud"]["health"], expected["health"], "health")
    position = scripts["player_position_q8"]
    target = expected["position_q8"]
    residual = math.dist(position, target["value"])
    assert residual <= target["tolerance"], (
        f"position_q8: distance {residual:.3f} exceeds {target['tolerance']}; got {position}")
    return residual


SESSION_NAME = re.compile(r"[a-z0-9][a-z0-9-]*")


def validate(fixture):
    """Accept timing, keyboard events and complete checkpoint expectations only."""
    assert {"version", "step_micros", "steps"} <= set(fixture) <= {"version", "step_micros", "steps", "entry_scene", "resume_session"}, "unexpected fixture fields"
    if "entry_scene" in fixture:
        assert fixture["entry_scene"] in ("BELLTOWR", "EXTPALAC", "TAVERN", "ISLAND"), "unsupported staged entry"
    if "resume_session" in fixture:
        assert "entry_scene" in fixture, "a resumed session needs a staged entry scene"
        assert SESSION_NAME.fullmatch(fixture["resume_session"]), "invalid resume session name"
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
            assert {"world", "flags", "globals", "inventory", "position_q8"} <= set(expected) <= {"world", "flags", "globals", "inventory", "position_q8", "health"}, "incomplete checkpoint"
            assert expected["flags"] and expected["inventory"], "checkpoint needs flags and inventory"
            position = expected["position_q8"]
            assert len(position["value"]) == 3 and 0 <= position["tolerance"] < float("inf"), "invalid position tolerance"
            checkpoints += 1
        elif "face" in step:
            assert set(step) == {"face"}, "facing cannot contain commands"
            face = step["face"]
            assert set(face) == {"world", "heading", "rope", "limit"}, "incomplete facing"
            assert type(face["heading"]) is int and 0 <= face["heading"] < 2048, "invalid heading"
            assert type(face["rope"]) is bool, "invalid rope mode"
            assert type(face["limit"]) is int and 0 < face["limit"] <= 3000, "invalid facing limit"
        elif "topic" in step:
            assert set(step) == {"topic"}, "topic cannot contain commands"
            assert set(step["topic"]) == {"world", "text", "limit"}, "incomplete topic"
            assert isinstance(step["topic"]["text"], str) and step["topic"]["text"], "empty topic"
            assert type(step["topic"]["limit"]) is int and 0 < step["topic"]["limit"] <= 10000, "invalid topic limit"
        elif "await_control" in step:
            assert set(step) == {"await_control"}, "control wait cannot contain commands"
            wait = step["await_control"]
            assert {"world", "max_seconds", "max_updates"} <= set(wait) <= {"world", "max_seconds", "max_updates", "rope_attached", "inventory"}, "incomplete control wait"
            if "rope_attached" in wait:
                assert type(wait["rope_attached"]) is bool, "invalid rope attachment expectation"
            if "inventory" in wait:
                assert isinstance(wait["inventory"], dict) and wait["inventory"], "empty inventory wait"
            assert math.isfinite(wait["max_seconds"]) and wait["max_seconds"] > 0, "invalid control clock bound"
            assert type(wait["max_updates"]) is int and wait["max_updates"] > 0, "invalid control update bound"
        elif "steer" in step:
            assert set(step) == {"steer"}, "steering cannot contain other commands"
            target = step["steer"]
            assert {"world", "position_q8", "tolerance", "limit"} <= set(target) <= {"world", "position_q8", "tolerance", "limit", "transition_to", "gait", "forward_updates", "settle_updates", "stop_updates", "approach_radius"}, "incomplete steering target"
            for key, maximum in [("forward_updates", 4), ("settle_updates", 50), ("stop_updates", 100)]:
                if key in target:
                    assert type(target[key]) is int and 1 <= target[key] <= maximum, "invalid steering cadence"
            if "approach_radius" in target:
                assert math.isfinite(target["approach_radius"]) and target["approach_radius"] > target["tolerance"], "invalid final approach radius"
                assert "stop_updates" in target, "final approach needs a stop interval"
            assert target.get("gait", "walk") in ("walk", "run"), "invalid steering gait"
            assert len(target["position_q8"]) == 3 and all(math.isfinite(v) for v in target["position_q8"]), "invalid steering position"
            assert 0 < target["tolerance"] < float("inf"), "invalid steering tolerance"
            assert type(target["limit"]) is int and 1 <= target["limit"] <= 2000, "invalid steering limit"
        elif "capture_session" in step:
            assert set(step) == {"capture_session"}, "session capture cannot contain commands"
            assert SESSION_NAME.fullmatch(step["capture_session"]), "invalid capture session name"
        elif "setup" in step:
            assert set(step) == {"setup"}, "setup cannot contain commands"
            setup = step["setup"]
            assert set(setup) <= {"console", "place_player", "cheat"} and setup, "unsupported setup"
            assert all(isinstance(name, str) and type(value) is int for name, value in setup.get("cheat", {}).items()), "invalid cheat setup"
            assert all(isinstance(line, str) and line for line in setup.get("console", [])), "invalid console setup"
            if "place_player" in setup:
                assert set(setup["place_player"]) == {"x", "y", "z", "heading"} and all(type(v) is int for v in setup["place_player"].values()), "invalid player placement"
        elif "await_ending" in step:
            assert set(step) == {"await_ending"}, "ending wait cannot contain commands"
            wait = step["await_ending"]
            assert set(wait) == {"world", "max_updates", "max_seconds"}, "incomplete ending wait"
            assert type(wait["max_updates"]) is int and wait["max_updates"] > 0, "invalid ending update bound"
            assert math.isfinite(wait["max_seconds"]) and 0 < wait["max_seconds"] <= 1800, "invalid ending wall-clock bound"
        elif "fight" in step:
            assert set(step) == {"fight"}, "fight cannot contain commands"
            fight = step["fight"]
            assert set(fight) == {"world", "opponent", "target_health", "max_seconds"}, "incomplete fight"
            assert isinstance(fight["opponent"], str) and type(fight["target_health"]) is int and fight["target_health"] >= 0, "invalid fight target"
            assert math.isfinite(fight["max_seconds"]) and 0 < fight["max_seconds"] <= 600, "invalid fight clock bound"
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


def await_control(control, wait):
    """Let an entry script finish without input or changing its lockout.

    Both bounds are measured in the game, independent of host rendering speed.
    Optional attachment and inventory expectations wait for script-owned state
    instead of assuming an animation or task completes after a fixed delay.
    A blocked scheduler still reports its own boundary timeout.
    """
    initial = control.ready(wait["world"], examine_open=None)
    start = initial["clock"]["game_seconds"]
    updates = initial["scripts"]["ordinary_updates"]
    state = initial
    def complete(state):
        if state["hud"]["lockout"] != 0:
            return False
        if "inventory" in wait:
            try:
                equal(state["script_hud"]["inventory"]["counts"], wait["inventory"], "inventory")
            except AssertionError:
                return False
        if "rope_attached" in wait:
            actors = control.request({"type": "scripts", "limit": 64})["actors"]
            selected = [actor for actor in actors if actor["source"] == "player"]
            assert len(selected) == 1, "control wait requires one selected player"
            if selected[0]["control"]["rope_attached"] != wait["rope_attached"]:
                return False
        return True

    while not complete(state):
        elapsed = state["clock"]["game_seconds"] - start
        consumed = state["scripts"]["ordinary_updates"] - updates
        assert 0 <= elapsed <= wait["max_seconds"], f"entry lockout clock bound: {elapsed} seconds"
        assert 0 <= consumed < wait["max_updates"], f"entry lockout update bound: {consumed} updates"
        control.request({"type": "schedule_input", "events": [
            {"after_updates": 0}, {"after_updates": 1}]})
        state = control.ready(wait["world"], examine_open=None)
    return state


def face_heading(control, face):
    """Turn with ground or rope controls, observing the heading after each input."""
    tolerance = 3  # one 5-unit turn step always lands inside this window
    for _ in range(face["limit"]):
        state = control.ready(face["world"])
        error = (face["heading"] - state["hud"]["heading"] + 1024) % 2048 - 1024
        if abs(error) <= tolerance:
            return
        if face["rope"]:
            keys = ["ShiftLeft", "ArrowRight" if error > 0 else "ArrowLeft"]
            held = min(40, max(1, int(abs(error) / 24)))
        else:
            keys = ["ArrowLeft" if error > 0 else "ArrowRight"]
            # Ground turning is 5 heading units per held update: scale the hold so a far start
            # heading converges inside the limit, and shrink it to single updates near the target.
            held = min(40, max(1, int(abs(error) / 12)))
        control.request({"type": "schedule_input", "events": [
            *[{"after_updates": 0, "key": key, "action": "press"} for key in keys],
            *[{"after_updates": held, "key": key, "action": "release"} for key in keys],
            {"after_updates": held + 1}]})
    raise AssertionError(f"facing limit exhausted: {face['heading']}")


def choose_topic(control, topic):
    """Select a displayed installed topic through balanced keyboard input."""
    for _ in range(topic["limit"]):
        control.ready(topic["world"])
        globals_ = control.request({"type": "scripts", "offset": 0, "limit": 1})["globals"]
        menu = globals_["menu"]
        if globals_["menu_state"] == 1:
            choices = menu["slots"][:menu["count"]]
            target = next((i for i, choice in enumerate(choices) if choice["text"] == topic["text"]), None)
            assert target is not None, f"topic {topic['text']!r} unavailable: {[c['text'] for c in choices]}"
            key = ("ControlLeft" if target == menu["current"] else
                   "ArrowDown" if target > menu["current"] else "ArrowUp")
            control.request({"type": "schedule_input", "events": [
                {"after_updates": 0, "key": key, "action": "press"},
                {"after_updates": 1, "key": key, "action": "release"},
                {"after_updates": 3}]})
            if key == "ControlLeft":
                print(f"selected dialogue topic {topic['text']!r}", flush=True)
                return
        else:
            control.request({"type": "schedule_input", "events": [
                {"after_updates": 0}, {"after_updates": 2}]})
    raise AssertionError(f"topic wait limit exhausted: {topic['text']!r}")


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


def play_modal(control, modal):
    """Close a scripted viewer with balanced keys across sampled app frames.

    A viewer can open before it samples its initial key release, so a tap in
    that first frame may be ignored. Retry bounded presses after release frames.
    Ordinary updates remain paused until the viewer has closed and keys release.
    """
    before = control.ready(modal["world"], examine_open=True)
    deadline = time.monotonic() + 600

    def frames():
        state = control.request({"type": "state"})
        start = state["frame"]
        while state["frame"] < start + 2:
            assert time.monotonic() < deadline, "modal input frame timeout"
            assert state["scripts"]["failed"] == 0, "script failure during modal input"
            time.sleep(0.05)
            state = control.request({"type": "state"})
        return state

    for _ in range(32):
        control.request({"type": "input", "key": modal["key"], "action": "press"})
        try:
            frames()
        finally:
            control.request({"type": "input", "key": modal["key"], "action": "release"})
        if not frames()["examine"]["open"]:
            control.ready(modal["world"], min_updates=before["scripts"]["ordinary_updates"] + 1)
            return
    raise AssertionError("modal input limit exhausted")


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
        if distance <= target["tolerance"] and target.get("stop_updates"):
            before = position
            control.request({"type": "schedule_input", "events": [
                {"after_updates": 0}, {"after_updates": target["stop_updates"]}]})
            state = control.ready(target["world"])
            scripts = control.request({"type": "scripts", "offset": 0, "limit": 1})
            position = scripts["player_position_q8"]
            coast = math.hypot(position[0] - before[0], position[2] - before[2])
            print(f"released coast over {target['stop_updates']} updates: {coast:.3f} Q8 ({target.get('gait', 'walk')})", flush=True)
            distance = math.hypot(target["position_q8"][0] - position[0], target["position_q8"][2] - position[2])
            if distance > target["tolerance"]:
                continue
        if distance <= target["tolerance"]:
            print(f"steering reached waypoint after {iteration} inputs, residual {distance:.3f} Q8", flush=True)
            return {"state": state, "scripts": scripts}
        if iteration == target["limit"]:
            break
        bearing = (-math.atan2(dx, dz) * 2048 / math.tau) % 2048
        error = (bearing - state["hud"]["heading"] + 1024) % 2048 - 1024
        # Short schedules resample heading after each turn and avoid strafing.
        approaching = distance <= target.get("approach_radius", 0)
        turning = abs(error) > 48
        keys = (["ArrowLeft" if error > 0 else "ArrowRight"] if turning else
                ["ArrowUp"] if target.get("gait", "walk") == "run" and not approaching else ["ShiftLeft", "ArrowUp"])
        if turning:
            held = min(4, max(2, int(abs(error) / 64)))
        elif approaching:
            held = 1
        else:
            held = target.get("forward_updates", min(4, max(1, int(distance / 4096))))
        settle = target.get("stop_updates", 0) if approaching else target.get("settle_updates", 0)
        events = ([{"after_updates": 0, "key": key, "action": "press"} for key in keys]
                  + [{"after_updates": held, "key": key, "action": "release"} for key in keys]
                  + ([{"after_updates": held + settle}] if settle else []))
        transitioned = play_inputs(control, {"input": events, **(
            {"transition_to": target["transition_to"]} if "transition_to" in target else {})})
        if transitioned:
            return
        if (iteration + 1) % 20 == 0:
            print(f"steering input {iteration + 1}/{target['limit']}, residual {distance:.3f} Q8", flush=True)
    raise AssertionError(f"steering limit exhausted: target {target['position_q8']}, last position {position}, residual {distance:.3f} Q8")


def settle_async_work(control, timeout=120):
    """Wait, without advancing ordinary updates, for asynchronous model and geometry work.

    Gameplay that starts while such work is pending depends on wall-clock completion time."""
    deadline = time.monotonic() + timeout
    while True:
        state = control.request({"type": "state"})
        if not (state.get("rendered_actor_pending") or state.get("effect_geometry_pending") or state.get("preload")):
            return state
        assert time.monotonic() < deadline, "asynchronous model or geometry work never settled"
        time.sleep(0.1)


def capture_session(control, path):
    """Save the live session document under `path` so a later launch can resume from it."""
    for attempt in range(100):
        control.request({"type": "session_capture"})
        for _ in range(300):
            result = control.request({"type": "session_capture_result"})
            if not result.get("pending", True):
                break
            time.sleep(0.1)
        else:
            raise AssertionError("session capture never completed")
        if "document" in result:
            path.parent.mkdir(parents=True, exist_ok=True)
            temporary = path.with_suffix(".tmp")
            temporary.write_text(result["document"])
            temporary.replace(path)
            return
        time.sleep(0.2)
    raise AssertionError("session capture was never accepted")


def resume_session(control, path):
    """Replace the fresh entry world with a session captured by an earlier launch."""
    assert path.exists(), f"missing resume session {path}; run the segment that captures it first"
    control.request({"type": "session_load", "path": str(path.resolve())})
    for _ in range(600):
        result = control.request({"type": "session_load_result"})
        if not result.get("pending", True):
            break
        time.sleep(0.1)
    else:
        raise AssertionError("session load never completed")
    assert result.get("committed") is True, f"session load did not commit: {result}"
    control.request({"type": "session_load_result", "acknowledge": True})
    settle_async_work(control)


def run_setup(control, setup):
    """Test-only setup shortcuts: console lines and a placement, never used for quest state."""
    for line in setup.get("console", []):
        control.request({"type": "console", "line": line})
    if "console" in setup:
        control.request({"type": "console", "open": False})
    if "place_player" in setup:
        control.request({"type": "place_player", **setup["place_player"]})
    for name, value in setup.get("cheat", {}).items():
        control.request({"type": "cheat", "name": name, "value": value})


def await_ending(control, wait):
    """Idle until the won-game ending discards the world and the book opens (no input is sent).

    The player must stay alive while the world runs. The ending movie and credits run outside
    ordinary updates, so their phase is bounded by wall-clock seconds."""
    for update in range(wait["max_updates"]):
        state = control.request({"type": "state"})
        if update % 100 == 0:
            print(f"ending wait: update {update}, state {state['state']}", flush=True)
        scripts = state.get("scripts") or {}
        assert scripts.get("failed", 0) == 0, f"script failures: {scripts}"
        if state["state"] != "InGame":
            break
        assert state["hud"]["health"] > 0, "player died before the ending"
        control.request({"type": "schedule_input", "events": [{"after_updates": 0}, {"after_updates": 5}]})
    else:
        raise AssertionError(f"ending never started within {wait['max_updates']} updates")
    deadline = time.monotonic() + wait["max_seconds"]
    while True:
        state = control.request({"type": "state"})
        if state["state"] == "Menu" and (state.get("scripts") or {}).get("world") is None:
            return
        assert time.monotonic() < deadline, f"ending did not reach the book: state {state['state']}"
        time.sleep(0.25)


def actor_index(control, name):
    """The listing index of the single actor called `name`."""
    found, offset = [], 0
    while True:
        page = control.request({"type": "scripts", "offset": offset, "limit": 64})
        found += [offset + i for i, actor in enumerate(page["actors"]) if actor["name"] == name]
        offset += len(page["actors"])
        if not page["actors"] or offset >= page["total"]:
            break
    assert len(found) == 1, f"fight opponent {name!r} matched {len(found)} times"
    return found[0]


def opponent_health(control, name, index):
    """Health attribute of the opponent, or None once the world no longer lists it at `index`."""
    actors = control.request({"type": "scripts", "offset": index, "limit": 1})["actors"]
    if actors and actors[0]["name"] == name:
        return actors[0]["attributes"][3]
    return None


def fight(control, spec):
    """Draw the sword and tap attack once per eight updates, reading both healths every update.

    Each opponent loss must be a positive multiple of ten (one or more whole hits), the player
    must survive, and the opponent must reach the target health inside the game-clock bound."""
    control.ready(spec["world"])
    state = settle_async_work(control)
    start = state["clock"]["game_seconds"]
    index = actor_index(control, spec["opponent"])
    previous = opponent_health(control, spec["opponent"], index)
    control.request({"type": "schedule_input", "events": [
        {"after_updates": 0, "key": "KeyS", "action": "press"},
        {"after_updates": 1, "key": "KeyS", "action": "release"}, {"after_updates": 4}]})
    hits = []

    def observe():
        nonlocal previous
        state = control.request({"type": "state"})
        assert state["scripts"]["failed"] == 0, f"script failures: {state['scripts']}"
        assert state["hud"]["health"] > 0, "player died during the fight"
        health = opponent_health(control, spec["opponent"], index)
        if health is None:
            assert spec["target_health"] == 0, f"fight opponent {spec['opponent']!r} vanished"
            health = 0  # a defeated opponent leaves the actor list
        if health != previous:
            loss = previous - health
            assert loss > 0 and loss % 10 == 0, f"opponent health {previous} -> {health}: not whole hits"
            hits.append(loss)
            previous = health
        return state, health

    cycles = 0
    while True:
        cycles += 1
        if cycles % 10 == 0:
            print(f"fight cycle {cycles}: opponent {previous}, hits {hits}, "
                  f"clock {control.request({'type': 'state'})['clock']['game_seconds']:.1f}", flush=True)
        control.request({"type": "schedule_input", "events": [
            {"after_updates": 0, "key": "ControlLeft", "action": "press"},
            {"after_updates": 2, "key": "ControlLeft", "action": "release"}, {"after_updates": 3}]})
        state, health = observe()
        for _ in range(5):
            control.request({"type": "schedule_input", "events": [{"after_updates": 0}, {"after_updates": 1}]})
            state, health = observe()
        if health <= spec["target_health"]:
            print(f"fight reached {health} after hits {hits}", flush=True)
            return
        assert state["clock"]["game_seconds"] - start <= spec["max_seconds"], (
            f"fight clock bound: opponent {health}, hits {hits}")


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
    assert fixtures, "no replay fixtures"
    entries = [f for f in fixtures if "entry_scene" in f]
    assert not entries or len(fixtures) == 1, "staged segments require an independent session"
    timing = fixtures[0]["step_micros"]
    assert isinstance(timing, int) and timing > 0, "step_micros must be a positive integer"
    assert all(f["step_micros"] == timing for f in fixtures), "chain timing must agree"
    args.config.mkdir(parents=True, exist_ok=True)
    # Runs share captured sessions through CYRUS_REPLAY_SESSIONS, else the config directory's parent.
    sessions = Path(os.environ.get("CYRUS_REPLAY_SESSIONS") or args.config.parent / "sessions")
    with (args.config / "replay.lock").open("w") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        with socket.socket() as probe:
            probe.bind(("127.0.0.1", 0))
            port = probe.getsockname()[1]
        env = dict(os.environ, CYRUS_CONFIG_DIR=str(args.config.resolve()))
        with (args.config / "process.log").open("w") as log:
            entry = (["--scene", fixtures[0]["entry_scene"], "--skip-startup"]
                     if entries else ["--new-game", "--skip-movies"])
            process = subprocess.Popen([
                str(args.binary.resolve()), "--headless", *entry,
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
                    for fixture in fixtures:
                        if "resume_session" in fixture:
                            resume_session(control, sessions / f"{fixture['resume_session']}.session")
                            control.ready(examine_open=None)
                    for path, fixture in zip(args.fixtures, fixtures):
                        assert fixture["version"] == 1, f"{path}: unsupported version"
                        transitioned = False
                        observation = None
                        for index, step in enumerate(fixture["steps"]):
                            if "checkpoint" not in step:
                                observation = None
                            if "input" in step and not transitioned:
                                transitioned = play_inputs(control, step)
                            if "face" in step:
                                face_heading(control, step["face"])
                            if "topic" in step:
                                choose_topic(control, step["topic"])
                            if "await_control" in step:
                                await_control(control, step["await_control"])
                            if "steer" in step:
                                observation = steer(control, step["steer"])
                            if "repeat" in step:
                                repeat_inputs(control, step["repeat"])
                            if "modal_input" in step:
                                play_modal(control, step["modal_input"])
                            if "capture_session" in step:
                                capture_session(control, sessions / f"{step['capture_session']}.session")
                            if "setup" in step:
                                run_setup(control, step["setup"])
                            if "fight" in step:
                                fight(control, step["fight"])
                            if "await_ending" in step:
                                await_ending(control, step["await_ending"])
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
