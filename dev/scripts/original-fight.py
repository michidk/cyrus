#!/usr/bin/env python3
"""Measure the original game's Richton duel on EXTPALAC, driven by the original's own frame counter.

Starts the original under DOSBox-X in the retro container (image `cyrus-retro:dev`, built from
dev/docker/retro) with a run copy of the install under $CYRUS_MAGIC/retro-run-<name>, never the shared
read-only game links. One streaming sampler inside the container reads every actor's group,
health, pending group and drawn-sword byte several times a second, so there is no per-iteration
docker exec; keys go through xdotool. Every wait counts the original's `mainloop_frame`, not wall
time: on a loaded host the original runs at a few frames per second and fixed wall-clock waits
expire before a draw or a swing finishes.

The state reader and the memory-placement helper hold executable addresses, so they stay in the
gitignored $CYRUS_MAGIC/tools/original-state.py and $CYRUS_MAGIC/helpers/orig_write_in.py (never
commit them). Output is one JSON object per sampled frame in --out, then a summary on stdout.

    CYRUS_MAGIC=~/workspaces/cyrus/magic python3 dev/scripts/original-fight.py \\
        --name soup4 --out run.jsonl --max-frames 1500 --cheat drevil

The summary reports taps, hits, damage per hit, each tap's outcome (hit, parried or countered by the
opponent's reaction group, or missed) split by whether the opponent was idle or busy, and the
frames and game seconds to the end of the run.
"""
import argparse
from harness import exclusive_lock
import json
import math
import os
import re
import shutil
import subprocess
import sys
import struct
from dataclasses import dataclass
from enum import Enum
import threading
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from diag_output import DurableLog, verify
from original_console import Console, acknowledged_hold

ROOT = Path(__file__).resolve().parents[2]
MAGIC = Path(os.environ.get("CYRUS_MAGIC", ROOT / "magic"))
OPPONENT_ID = 55  # RICHTONP
ARMED_IDLE_GROUP = 8
NOT_ENGAGED_GROUP = 0
# The installed EXTPALAC RICHTONP script begins combat only while HaveItem(79) is 1 and SOUP
# PlayerDistance() < 131072 (preservation SOUP.md "PlayerDistance and PlayerFacing"): the sum of the
# absolute X, Y and Z differences in Q8 runtime units, so height counts, not just the courtyard walk.
ENGAGE_RANGE_Q8 = 131072
STATE_EVERY_FRAMES = 20  # one state row per this many original frames during the fight
HIT_REACTION_GROUP = 117  # a nonplayer hit receiver's reaction (preservation SOUP.md)
SAMPLER = """\
import contextlib, importlib.util, io, json, sys, time
spec = importlib.util.spec_from_file_location("st", "/tmp/original-state.py")
st = importlib.util.module_from_spec(spec)
spec.loader.exec_module(st)
interval = float(sys.argv[1])
while True:
    out = io.StringIO()
    try:
        with contextlib.redirect_stdout(out):
            st.main()
        print(out.getvalue().strip().replace("\\n", " "), flush=True)
    except (Exception, SystemExit) as error:  # not started yet or the world is changing: keep sampling
        print(json.dumps({"error": str(error)}), flush=True)
    time.sleep(interval)
"""


class QueryPositionSource(Enum):
    """Effective-position owners from preservation's PlayerDistance selection."""

    LOCAL = 'local'
    ATTACHED = 'attached'
    LINKED = 'linked'


def position_q8(value):
    """Require a bound signed runtime coordinate triple, without coercion."""
    if (not isinstance(value, (list, tuple)) or len(value) != 3
            or any(type(v) is not int or not -(1 << 31) <= v < (1 << 31) for v in value)):
        raise ValueError('query position requires three signed Q8 integers')
    return tuple(value)


@dataclass(frozen=True)
class QueryPosition:
    """A selected query point and its observed owner, distinct from model vertices."""

    source: QueryPositionSource
    position_q8: tuple[int, int, int]

    @classmethod
    def read(cls, actor):
        spatial = actor.get('query_spatial')
        if not isinstance(spatial, dict) or type(spatial.get('slave')) is not bool:
            raise ValueError('query position requires bound attachment selection')
        if spatial['slave']:
            source, point = QueryPositionSource.ATTACHED, spatial.get('attached_q8')
        else:
            if type(spatial.get('link_valid')) is not bool:
                raise ValueError('query position requires bound link validity')
            if spatial['link_valid']:
                source, point = QueryPositionSource.LINKED, spatial.get('linked_q8')
            else:
                source, point = QueryPositionSource.LOCAL, actor.get('position_q8')
        return cls(source, position_q8(point))

    def export(self):
        return dict(source=self.source.value, position_q8=list(self.position_q8))


def combat_query_range_sample(state, query_motion):
    """Observe only zero-sphere/zero-radius range, not orientation, elevation or defence.

    The caller supplies current-motion sphere size and the installed retained
    attack slot's first-motion bounds separately. Other contact branches require
    model-point observations and are rejected. The
    distance follows preservation's node-marker mixed-precision arithmetic,
    including wrapping subtraction and nearest-even conversion. Missing query
    bindings never fall back to body origins. This is not a hit verdict.
    """
    combat = state.get('opponent_combat') or {}
    if (type(query_motion.get('current_col_sphere_size')) is not int
            or query_motion['current_col_sphere_size'] != 0
            or type(combat.get('contact_radius_q8')) is not int
            or combat['contact_radius_q8'] != 0):
        raise ValueError('query range requires bound zero-sphere zero-radius contact branch')
    bounds = [query_motion.get(name) for name in ('min_range', 'max_range')]
    if any(type(v) is not int or not -(1 << 31) <= v < (1 << 31) for v in bounds):
        raise ValueError('query range requires installed signed first-motion bounds')
    points = []
    for owner in ('player', 'opponent'):
        query = state.get(f'{owner}_query')
        if not isinstance(query, dict):
            raise ValueError(f'{owner} query position unavailable')
        try:
            QueryPositionSource(query.get('source'))
        except ValueError as error:
            raise ValueError(f'{owner} query position source unavailable') from error
        points.append(position_q8(query.get('position_q8')))
    single = lambda v: struct.unpack('<f', struct.pack('<f', v))[0]
    differences = [((b - a + (1 << 31)) % (1 << 32)) - (1 << 31)
                   for a, b in zip(*points)]
    squared = sum(int(single(v)) ** 2 for v in differences)
    root = math.sqrt(float(squared))
    stored = single(root)
    bits = struct.unpack('<I', struct.pack('<f', stored))[0]
    neighbor = struct.unpack('<f', struct.pack('<I', bits - 1 if stored > root else bits + 1))[0]
    midpoint = (stored + neighbor) / 2
    # Correct a double-rounded root at an f32 midpoint, as the native distance
    # service does. Compare with the squared sum after its f64 store.
    if root == midpoint:
        boundary = midpoint * midpoint
        if float(squared) < boundary:
            stored = min(stored, neighbor)
        elif float(squared) > boundary:
            stored = max(stored, neighbor)
    distance = round(stored)
    if distance > (1 << 31) - 1:
        raise ValueError('query range distance exceeds signed integer range')
    return dict(frame=state.get('frame'), distance_q8=distance, strict_range_pass=bounds[0] < distance < bounds[1],
                min_range_q8=bounds[0], max_range_q8=bounds[1],
                player_query=state['player_query'], opponent_query=state['opponent_query'])


def view(state):
    """The fields the policy needs from one reader sample, or None without a selected player.
    Independent observed_actors retain a bound actor outside the view-centered update
    list. rlisted distinguishes that observation from admission; absence from the
    update list alone does not establish removal or death."""
    if not state or "actors" not in state:
        return None
    player = next((a for a in state["actors"] if a["selected"]), None)
    if player is None:
        return None
    opponent = next((a for a in state["actors"] if a["id"] == OPPONENT_ID and not a["selected"]), None)
    listed = opponent is not None
    if opponent is None:
        opponent = next((a for a in state.get("observed_actors", [])
                         if a["id"] == OPPONENT_ID and not a["selected"]), None)
    return {
        "frame": state["counters"]["mainloop_frame"],
        "fps": state["counters"]["fps"],
        "pg": player["group"], "ph": player["health"], "pdrawn": player["drawn"],
        "rg": opponent["group"] if opponent else None,
        "rpending": opponent["pending"] if opponent else None,
        "rh": opponent["health"] if opponent else None,
        "world": state.get("world"),
        "lockout": state["counters"].get("player_lockout"),
        "rlisted": listed,
        "player_combat": player.get("combat_state"),
        "player_query": QueryPosition.read(player).export() if "query_spatial" in player else None,
        "opponent_query": (QueryPosition.read(opponent).export()
                           if opponent and "query_spatial" in opponent else None),
        "opponent_combat": opponent.get("combat_state") if opponent else None,
        "player_command_cursor": player.get("command_frame"),
        "opponent_command_cursor": opponent.get("command_frame") if opponent else None,
        "rhidden": opponent.get("hidden") if opponent else None,
        # A combat structure is allocated before admission. Combat entry writes
        # lifecycle selector 6; its pointer alone cannot acknowledge the native.
        "rcombat": (opponent["death_selector"] == 6
                    if opponent and "death_selector" in opponent else None),
        # Q8 positions and yaw, for distance and step-back analysis against the native snapshots.
        "ppos": player["position_q8"], "pyaw": player["yaw"],
        "rpos": opponent["position_q8"] if opponent else None,
        "ryaw": opponent["yaw"] if opponent else None,
    }


def player_distance_q8(sample):
    """SOUP PlayerDistance(): |dx| + |dy| + |dz| in Q8 units between the fighters, or None."""
    if sample["rpos"] is None:
        return None
    return sum(abs(a - b) for a, b in zip(sample["rpos"], sample["ppos"]))


def horizontal_distance(sample):
    """Horizontal Q8 distance between the fighters (map X and Z), or None while he is absent."""
    if sample["rpos"] is None:
        return None
    return round(math.hypot(sample["rpos"][0] - sample["ppos"][0], sample["rpos"][2] - sample["ppos"][2]))


def opponent_busy(sample):
    """The opponent is attacking, defending or reacting: any group other than idle, not engaged
    or absent."""
    return sample["rg"] not in (NOT_ENGAGED_GROUP, ARMED_IDLE_GROUP, None)


def ready_to_tap(sample, gate):
    """A tap goes only from the player's armed idle animation (group 8 is reached only with the
    sword drawn; the reader's drawn byte stays 0 there, so the group is the signal); the `busy`
    gate additionally waits for the opponent to be mid-action (an idle opponent counters)."""
    if sample["pg"] != ARMED_IDLE_GROUP:
        return False
    return opponent_busy(sample) if gate == "busy" else True


def classify_tap(before, samples):
    """Outcome of one tap from the samples that followed it: the opponent's health drop is a hit
    (with its damage), his reaction group 117 without a drop is a hit absorbed elsewhere, and any
    other change in his group is a parry/counter response; none at all is a miss."""
    drop = before["rh"] - min((s["rh"] for s in samples), default=before["rh"])
    if drop > 0:
        return {"outcome": "hit", "damage": drop}
    groups = [s["rg"] for s in samples]
    if HIT_REACTION_GROUP in groups:
        return {"outcome": "reaction", "damage": 0}
    if any(g != before["rg"] for g in groups):
        return {"outcome": "response", "damage": 0, "groups": sorted(set(groups) - {before["rg"]})}
    return {"outcome": "miss", "damage": 0}


def combat_geometry(sample):
    """Diagnostic body-origin geometry, not the engine's model-point contact query.

    Positions are Q8; yaw has 2048 units per turn. atan2 estimates the bearing
    for ordinary turn input. Actual contact still requires observed combat state.
    """
    dx, dy, dz = [b - a for a, b in zip(sample['ppos'], sample['rpos'])]
    heading = round(math.atan2(-dx, dz) * 1024 / math.pi) % 2048
    return dict(horizontal_q8=round(math.hypot(dx, dz)),
                distance_q8=round(math.sqrt(dx * dx + dy * dy + dz * dz)),
                height_delta_q8=dy,
                heading_error=(heading - sample['pyaw'] + 1024) % 2048 - 1024)


def align_combat_facing(session, max_updates):
    """Turn through the observed opponent bearing, releasing input at each boundary.

    A broad defence arc is not an attack-alignment threshold. Stop at an exact
    bearing or its first crossed center; report that actual pose, not a tolerance
    pass. Wait through a busy animation and recompute from the next armed idle.
    No actor position, health or heading is written by this driver.
    """
    if type(max_updates) is not int or max_updates < 0:
        raise ValueError('alignment requires a nonnegative update budget')
    previous = None
    world = session.snapshot()["world"]
    for update in range(max_updates + 1):
        state = session.snapshot()
        if state["world"] != world:
            raise ValueError("combat alignment changed world")
        if (state.get('rhidden') is not False or state.get('rcombat') is not True
                or state.get('rlisted') is not True or state.get('lockout') != 0):
            raise ValueError('alignment requires revealed admitted combat and player control')
        error = combat_geometry(state)['heading_error']
        if state['pg'] == ARMED_IDLE_GROUP:
            if error == 0 or (previous is not None and previous * error < 0):
                return state
            key = 'ArrowLeft' if error > 0 else 'ArrowRight'
            previous = error
        else:
            key, previous = None, None
        if update == max_updates:
            break
        try:
            if key:
                session.input(key, 'press')
            session.advance_to(state['frame'] + 1)
        finally:
            if key:
                session.input(key, 'release')
    raise TimeoutError('combat facing did not reach the observed center within update budget')


def wait_for_combat_recovery(session, max_updates):
    """Observe an inactive opponent action before a diagnostic attack input.

    This is a probe policy, not a claim of vulnerability. Group changes alone
    cannot identify recovery: attack intent may already be cancelled while its
    animation continues. Unbound action/defence observations reject the probe.
    """
    if type(max_updates) is not int or max_updates < 0:
        raise ValueError('recovery requires a nonnegative update budget')
    world = session.snapshot()['world']
    for update in range(max_updates + 1):
        state = session.snapshot()
        if state['world'] != world:
            raise ValueError('combat recovery changed world')
        if (state.get('rhidden') is not False or state.get('rcombat') is not True
                or state.get('rlisted') is not True or state.get('lockout') != 0):
            raise ValueError('recovery requires revealed admitted combat and player control')
        combat = state.get('opponent_combat')
        if not combat or 'action' not in combat or 'flags' not in combat:
            raise ValueError('recovery requires bound combat observations')
        if (state['pg'] == ARMED_IDLE_GROUP and combat['action'] == -1
                and not combat['flags'] & 2):
            return state
        if update < max_updates:
            session.advance_to(state['frame'] + 1)
    raise TimeoutError('opponent did not reach observed recovery within update budget')


def combat_events(samples):
    """Report observed intent, contact-mark, defence and health transitions per update.

    Contact-tested is the combat mark, not proof of a hit. Intent clearing and
    health changes at the same boundary do not by themselves establish causality.
    Missing reader bindings remain unobserved; gaps reject event attribution.
    """
    events = []
    for before, after in zip(samples, samples[1:]):
        if after['frame'] != before['frame'] + 1:
            raise ValueError('combat events require consecutive update samples')
        for owner, prefix in [('player', 'p'), ('opponent', 'r')]:
            old = (before.get(f'{owner}_combat') or {}).get('flags')
            new = (after.get(f'{owner}_combat') or {}).get('flags')
            if old is not None and new is not None:
                # Preservation's combat flags: move intent, defence intent and
                # contact-tested are independent of the current animation group.
                for mask, changed, event in [
                    (1, bool(old & 1) and not new & 1, 'intent_cleared'),
                    (2, not old & 2 and bool(new & 2), 'defence_started'),
                    (0x20, not old & 0x20 and bool(new & 0x20), 'contact_tested'),
                ]:
                    if changed:
                        row = {'frame': after['frame'], 'event': f'{owner}_{event}'}
                        if mask != 2:
                            row.update(cursor=after.get(f'{owner}_command_cursor'), group=after.get(f'{prefix}g'))
                        events.append(row)
            old_health, new_health = before.get(f'{prefix}h'), after.get(f'{prefix}h')
            if old_health is not None and new_health is not None and old_health != new_health:
                events.append({'frame': after['frame'], 'event': f'{owner}_health_changed',
                               'before': old_health, 'after': new_health})
    return events


class GateEvidence(Enum):
    PASSED = 'passed_by_state'
    REJECTED = 'rejected_by_state'
    UNOBSERVED = 'unobserved'
    NOT_REACHED = 'not_reached'


def defence_admission_sample(state, current_motion):
    """Project hit-defence admission in service order, without inventing contact.

    Bounds/cursor use signed runtime integers and a wrapping window end. Missing
    bindings stop the projection as unobserved, not rejected. These state gates
    do not replace instruction-bound contact/response evidence.
    """
    player, opponent = state.get('player_combat') or {}, state.get('opponent_combat') or {}
    signed = lambda v: type(v) is int and -(1 << 31) <= v < (1 << 31)
    wrap = lambda v: (v + (1 << 31)) % (1 << 32) - (1 << 31)
    def action():
        value = player.get('action')
        return value not in (-1, 9) if signed(value) else None
    def selector():
        value, required = opponent.get('defence'), current_motion.get('defense')
        return value == wrap(required - 1) if signed(value) and signed(required) else None
    def window():
        cursor, start = state.get('player_command_cursor'), current_motion.get('first_defend_frame')
        if not signed(cursor) or not signed(start): return None
        if cursor < start: return False
        duration = current_motion.get('defend_duration')
        return cursor < wrap(start + duration) if signed(duration) else None
    def counter():
        flags = opponent.get('flags')
        return bool(flags & 2) if type(flags) is int and 0 <= flags <= 255 else None
    gates, reached = {}, True
    for name, query in [('active_attacker', action), ('defence_selector', selector),
                        ('defence_window', window), ('counter_intent', counter)]:
        status = GateEvidence.NOT_REACHED
        if reached:
            value = query()
            status = (GateEvidence.UNOBSERVED if value is None else
                      GateEvidence.PASSED if value else GateEvidence.REJECTED)
            reached = value is True
        gates[name] = status.value
    gates['contact'] = (GateEvidence.UNOBSERVED if reached else GateEvidence.NOT_REACHED).value
    return gates


class DefenceEvidenceSite(Enum):
    """Reader-bound writes within the successful original hit-defence branch."""

    CONTACT_REVERSE = 'defence_contact_pass_reverse_write'
    SUCCESS_RESULT = 'defence_success_result_write'


class ContactPath(Enum):
    ZERO_RADIUS_RANGE = 'zero_radius_range'


def defence_response_evidence(branch, result):
    """Validate an instruction-bound witness, never infer defence from animation.

    The local observer must bind the reverse setter to the successful hit-defence
    caller and both actor identities, then observe that same call's success-result
    write. These sites prove earlier admission and contact gates by control flow;
    they do not supply individual numeric helper returns. Ordinary frame samples
    or a changed reverse byte without the bound caller are insufficient.
    Follows preservation's Hit-defence response and Range/arc motion contact.
    """
    try:
        if (DefenceEvidenceSite(branch.get('event')) is not DefenceEvidenceSite.CONTACT_REVERSE
                or DefenceEvidenceSite(result.get('event')) is not DefenceEvidenceSite.SUCCESS_RESULT):
            raise ValueError('wrong defence evidence site')
        path = ContactPath(branch.get('contact_path'))
    except (ValueError, TypeError) as error:
        raise ValueError('defence response requires bound branch and success-result sites') from error
    for event in (branch, result):
        if (event.get('attacker') != 'player' or event.get('defender') != 'opponent'
                or type(event.get('frame')) is not int or event['frame'] < 0):
            raise ValueError('defence response requires bound player/opponent identities and update')
    before, after = branch.get('state') or {}, result.get('state') or {}
    if (branch['frame'] != result['frame'] or type(before.get('world')) is not int
            or before['world'] != after.get('world')):
        raise ValueError('defence response changed update or world')
    if type(result.get('result')) is not int or result['result'] != 1:
        raise ValueError('defence response requires observed success-result one')
    for owner in ('player', 'opponent'):
        old, new = before.get(f'{owner}_combat') or {}, after.get(f'{owner}_combat') or {}
        if (type(old.get('flags')) is not int or not 0 <= old['flags'] <= 255
                or type(new.get('flags')) is not int or not 0 <= new['flags'] <= 255):
            raise ValueError('defence response requires bound combat flags')
        expected = old['flags'] & ~3
        if owner == 'opponent':
            expected ^= 8
            if (not old['flags'] & 2 or type(old.get('contact_radius_q8')) is not int
                    or old['contact_radius_q8'] != 0):
                raise ValueError('zero-radius defence branch lacks counter intent or radius binding')
        elif (type(old.get('action')) is not int or old['action'] in (-1, 9)
              or type(old.get('reverse')) is not int or old['reverse'] != 1
              or type(new.get('reverse')) is not int or new['reverse'] != 1):
            raise ValueError('defence branch requires active attacker and observed reverse write')
        if new['flags'] != expected:
            raise ValueError(f'{owner} defence flags disagree with intent clear/parity response')
    gates = ('active_attacker', 'defence_selector', 'defence_window', 'counter_intent',
             'orientation', 'strict_query_range', 'elevation', 'contact')
    return dict(frame=branch['frame'], world=before['world'], contact_path=path.value,
                gates={gate: 'passed_by_executed_branch' for gate in gates},
                result=1, response='completed')


def combat_range_sample(state, previous=None):
    """Body-origin geometry and each actor's signed XYZ motion in Q8 units.

    Pair distance alone cannot distinguish a retreat from opponent movement or
    hit displacement. These observations make no contact or step-back verdict.
    The first boundary has no displacement observation; missing bindings reject.
    """
    for name in ('ppos', 'rpos'):
        position = state.get(name)
        if (not isinstance(position, (list, tuple)) or len(position) != 3
                or any(type(value) is not int for value in position)):
            raise ValueError('range capture requires bound player and opponent positions')
    if type(state.get('pyaw')) is not int:
        raise ValueError('range capture requires bound player heading')
    if previous is not None:
        if state['frame'] != previous['frame'] + 1 or state['world'] != previous['world']:
            raise ValueError('range capture requires consecutive same-world updates')
    result = dict(frame=state['frame'], l1_q8=player_distance_q8(state), **combat_geometry(state))
    for owner, position in [('player', 'ppos'), ('opponent', 'rpos')]:
        result[f'{owner}_delta_q8'] = (None if previous is None else
            [value - old for value, old in zip(state[position], previous[position])])
    return result


def record_combat_tap(session, key, held_updates, released_updates, *, capture_range=False, query_motion=None):
    """Record one explicit input and every stopped update, never infer a hit from input.

    The session projects world, reveal/combat state and the ordinary fight view.
    Counts are original game updates supplied by the caller's input tape.
    Frame gaps and world changes reject the measurement; a failed read still
    releases the key. Health loss is reported over the whole observation window,
    not attributed to a single strike. Optional range capture requires bound
    poses and records each actor's displacement at every held/released boundary.
    query_motion supplies current_col_sphere_size and installed first-motion
    min_range/max_range for a separate query_ranges observation, never a hit verdict.
    """
    if type(held_updates) is not int or held_updates < 1 or type(released_updates) is not int or released_updates < 0:
        raise ValueError("combat tap requires positive held and nonnegative released updates")
    before = session.snapshot()
    if before.get("rhidden") is not False or before.get("rcombat") is not True:
        raise ValueError("opponent must be revealed and in combat before measuring a tap")
    if before.get("rlisted") is not True:
        raise ValueError("opponent must be in the ordinary update list before measuring a tap")
    if before.get("lockout") != 0:
        raise ValueError("player control must be restored before measuring a tap")
    if before["pg"] != ARMED_IDLE_GROUP:
        raise ValueError("player must be in armed idle before measuring a tap")
    samples = [before]
    trajectory = [combat_range_sample(before)] if capture_range else None
    query_ranges = ([combat_query_range_sample(before, query_motion)]
                    if query_motion is not None else None)
    try:
        session.input(key, "press")
        for offset in range(1, held_updates + released_updates + 1):
            if offset == held_updates + 1:
                session.input(key, "release")
            expected = before["frame"] + offset
            session.advance_to(expected)
            state = session.snapshot()
            if state["frame"] != expected:
                raise ValueError(f"combat frame gap: expected {expected}, observed {state['frame']}")
            if state["world"] != before["world"]:
                raise ValueError("combat tap changed world")
            if state.get("rlisted") is not True:
                raise ValueError("opponent left the ordinary update list during combat tap")
            if state.get("lockout") != 0:
                raise ValueError("player control lost during combat tap")
            if state.get("rhidden") is not False or state.get("rcombat") is not True:
                raise ValueError("opponent reveal/combat admission lost during combat tap")
            if capture_range:
                trajectory.append(combat_range_sample(state, samples[-1]))
            if query_ranges is not None:
                query_ranges.append(combat_query_range_sample(state, query_motion))
            samples.append(state)
    finally:
        session.input(key, "release")
    return dict(classify_tap(before, samples), samples=samples, events=combat_events(samples),
                key=key, release_frame=before["frame"] + held_updates, trajectory=trajectory,
                query_ranges=query_ranges,
                held_updates=held_updates, released_updates=released_updates)


def summarize(rows):
    """Aggregate a run: per-tap outcomes split by opponent idle/busy, hits and damage per hit."""
    taps = [r for r in rows if r.get("ev") == "tap"]
    result = {"taps": len(taps), "hits": 0, "damage": [], "by_state": {}}
    for tap in taps:
        state = "busy" if opponent_busy(tap["before"]) else "idle"
        bucket = result["by_state"].setdefault(state, {"taps": 0, "hit": 0, "reaction": 0, "response": 0, "miss": 0})
        bucket["taps"] += 1
        bucket[tap["outcome"]] += 1
        if tap["outcome"] == "hit":
            result["hits"] += 1
            result["damage"].append(tap["damage"])
    ends = [r for r in rows if r.get("ev") == "end"]
    if ends:
        result["end"] = {k: ends[-1].get(k) for k in ("rh", "ph", "frames", "reason")}
    return result


class Sampler:
    """Streams reader samples from one long-lived docker exec and keeps the latest."""

    def __init__(self, name, interval):
        self.process = subprocess.Popen(
            ["docker", "exec", "-i", name, "python3", "-u", "-c", SAMPLER, str(interval)],
            stdout=subprocess.PIPE, text=True, bufsize=1)
        self.latest = None
        self.lock = threading.Lock()
        self.thread = threading.Thread(target=self.pump, daemon=True)
        self.thread.start()

    def pump(self):
        for line in self.process.stdout:
            try:
                state = json.loads(line)
            except ValueError:
                continue
            with self.lock:
                self.latest = state

    def get(self):
        with self.lock:
            return view(self.latest), self.latest

    def stop(self):
        self.process.kill()


def wait_frames(sampler, frames, timeout, until=None):
    """Block on the original's frame counter, never on wall time alone.

    Without `until`, return once the counter has advanced `frames`. With `until`, return the first
    sample for which it holds; a positive `frames` is then a budget and raises TimeoutError once
    that many frames pass without it holding. `timeout` is only a wall-clock safety limit."""
    deadline = time.monotonic() + timeout
    start = None
    while time.monotonic() < deadline:
        sample, raw = sampler.get()
        counter = (raw or {}).get("counters", {}).get("mainloop_frame") if raw else None
        if counter is not None:
            start = counter if start is None else start
            if until:
                if sample and until(sample):
                    return sample
                if frames and counter - start >= frames:
                    raise TimeoutError(f"condition not met within {frames} frames")
            elif counter - start >= frames:
                return sample
        time.sleep(0.05)
    raise TimeoutError(f"no {frames} frames within {timeout} s (counter start {start})")


def walk_step(sample, tolerance=100):
    """One held-key decision for the walk-in: turn toward the opponent, else walk forward.
    Returns (key, hold_seconds) or None once he is inside striking range and faced.
    Original turning is about 416 heading units per second and forward walking about 200 map units
    per second (measured earlier); holds are wall time because a held key is what registers at a few
    frames per second."""
    if sample["rpos"] is None:
        return None
    dx, dz = sample["rpos"][0] - sample["ppos"][0], sample["rpos"][2] - sample["ppos"][2]
    bearing = round(math.degrees(math.atan2(dx, dz)) * 2048 / 360) % 2048
    # The original's yaw is -atan2(dx, dz) in 2048 units (Right decreases yaw).
    want = (-bearing) % 2048
    error = (want - sample["pyaw"] + 1024) % 2048 - 1024
    if abs(error) > tolerance:
        return ("Left" if error > 0 else "Right", min(1.5, max(0.2, abs(error) / 416)))
    if horizontal_distance(sample) > 12000:
        return ("Up", 1.0)
    return None


def walk_in(sampler, key, frames):
    """Walk to the opponent with held keys until he leaves group 0 or the frame budget ends."""
    end = None
    while True:
        sample, raw = sampler.get()
        if sample is None:
            time.sleep(0.1)
            continue
        end = end or sample["frame"] + frames
        if sample["rg"] not in (NOT_ENGAGED_GROUP, None):
            return sample
        if sample["frame"] >= end:
            raise TimeoutError(f"opponent did not engage while walking in (group {sample['rg']})")
        gap = abs(sample["rpos"][1] - sample["ppos"][1]) if sample["rpos"] else 0
        if gap >= ENGAGE_RANGE_Q8:
            raise AssertionError(
                f"the player is {gap // 256} map units above or below him: PlayerDistance() counts height, "
                f"so it can never fall under {ENGAGE_RANGE_Q8 // 256} map units on this level")
        step = walk_step(sample)
        if step is None:
            time.sleep(0.3)
            continue
        name, hold = step
        key("keydown", name)
        time.sleep(hold)
        key("keyup", name)
        time.sleep(0.2)


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--name", default="fight", help="run copy and container suffix")
    parser.add_argument("--out", required=True, help="JSON-lines output path")
    parser.add_argument("--mirror", help="second JSON-lines path (another directory) written row by row")
    parser.add_argument("--max-frames", type=int, default=1500, help="frames to fight before stopping")
    parser.add_argument("--gate", choices=("busy", "any"), default="busy", help="when a tap is allowed")
    parser.add_argument("--cheat", default="", help="console cheat to set to 1 before the fight")
    parser.add_argument("--approach", choices=("place", "walk"), default="place",
                        help="if he does not engage: write the player beside him, or walk in with held keys")
    parser.add_argument("--interval", type=float, default=0.2, help="sampler period in seconds")
    args = parser.parse_args()
    image = subprocess.check_output(
        [str(ROOT / "dev/scripts/ensure-retro-runtime.sh")], text=True).strip()
    run = MAGIC / f"retro-run-{args.name}"
    name = f"retro-{args.name}"
    reader = MAGIC / "tools/original-state.py"
    place = MAGIC / "helpers/orig_write_in.py"
    for needed in (reader, place):
        if not needed.exists():
            sys.exit(f"{needed} is missing; restore the local original-game tooling bundle")
    with exclusive_lock(Path(args.out).with_suffix(".lock")), exclusive_lock(run.with_suffix(".lock")):
        if not run.exists():
            shutil.copytree(MAGIC / "retro-run", run, symlinks=True)
        maps = run / "Redguard/maps"
        if maps.is_symlink() or not maps.resolve().is_relative_to(run.resolve()):
            sys.exit(f"{maps}: original needs run-local writable maps")
        ini = run / "Redguard/WORLD.INI"
        ini.write_text(re.sub(r"(?m)^start_world=\d+", "start_world=30", ini.read_text(errors="surrogateescape")),
                       errors="surrogateescape")
        sh = lambda *c, check=False, **k: subprocess.run(c, capture_output=True, text=True, check=check, **k)
        import importlib
        scenario = importlib.import_module('original-scenario')
        scenario.start_original_runtime(name, run, image, command=sh, magic=MAGIC)
        sh("docker", "cp", str(place), f"{name}:/tmp/orig_write_in.py", check=True)
        key = lambda *c: sh("docker", "exec", "-e", "DISPLAY=:1", name, "xdotool", *c)
        # Every row is fsynced, optionally mirrored to a second directory, and the run is sealed with a
        # manifest at the end; a pod replacement or a cleaned artifacts directory has already removed
        # finished runs before they were read.
        log = DurableLog(args.out, args.mirror)

        def emit(row):
            log.append(row)

        def phase(label):
            sample = sampler.get()[0] if "sampler" in locals() else None
            print(f"[{time.strftime('%H:%M:%S')}] {label} frame={sample and sample['frame']}", flush=True)

        sampler = Sampler(name, args.interval)
        # The player exists before Richton does; wait for a live world, then drive the console.
        wait_frames(sampler, 0, 360, until=lambda s: s["frame"] > 0 and s["ph"])
        wait_frames(sampler, 30, 600)
        shots = Path(args.out).parent / "console-shots"
        shots.mkdir(parents=True, exist_ok=True)

        def shot(tag):
            """Save a screenshot of the original's screen; the console reply is evidence, not a guess."""
            sh("docker", "exec", "-e", "DISPLAY=:1", name, "scrot", "-o", "/tmp/console.png")
            target = shots / f"{tag}.png"
            sh("docker", "cp", f"{name}:/tmp/console.png", str(target))
            return str(target)

        # Resolve F12 to its physical X keycode in the container. Named xdotool
        # F12 can synthesize Alt and select the game's nonmodal overlay instead.
        sh("docker", "cp", str(Path(__file__).with_name("original_console.py")),
           f"{name}:/tmp/original_console.py", check=True)

        def console_key(action, symbol):
            if symbol == "F12":
                sh("docker", "exec", "-e", "DISPLAY=:1", name, "python3",
                   "/tmp/original_console.py", action, symbol, check=True)
            else:
                key(action, symbol)

        def press_wall(name_, seconds):
            """Balanced input while the console owns its independent editor loop."""
            console_key("keydown", name_)
            try:
                time.sleep(seconds)
            finally:
                console_key("keyup", name_)

        def mode():
            return (sampler.get()[1] or {}).get("console_mode")

        console = Console(
            hold=lambda name_: acknowledged_hold(
                lambda k: console_key("keydown", k), lambda k: console_key("keyup", k),
                lambda: mode() in ("interactive", "overlay"), name_),
            press=press_wall, console_mode=mode, pause=time.sleep, shot=shot,
            editor_text=lambda: ((sampler.get()[1] or {}).get("console_editor") or {}).get("text"))
        for number, line in enumerate(("item add,79", "getmapmarker 32")):
            typed, answered = console.run(line, f"console-{number}")
            emit({"ev": "console", "line": line, "typed": typed, "answered": answered})
        # `getmapmarker 32` replaces the world with the finale staging scene: the opponent leaves the
        # actor list and returns only when the scripted approach ends (measured: ~2,100 frames later).
        # Placing the player before that moves nothing, so wait for the unload and then the return.
        phase("console done; waiting for the staging scene to unload the opponent")
        wait_frames(sampler, 1500, 900, until=lambda s: s["rg"] is None)
        phase("waiting for the scripted approach to bring the opponent back")
        first = wait_frames(sampler, 0, 1800, until=lambda s: s["rg"] is not None)
        emit({"ev": "opponent-present", **first})
        phase("setting the cheat")
        if args.cheat:
            key("key", "--delay", "200", "F12")
            wait_frames(sampler, 4, 120)
            key("type", "--delay", "100", f"{args.cheat} = 1")
            key("key", "--delay", "300", "KP_Enter")
            wait_frames(sampler, 6, 120)
            key("key", "--delay", "200", "F12")
            wait_frames(sampler, 4, 120)
        # A one-frame press is missed at a few frames per second: hold the key for about 1.5 s of wall
        # time (measured: held `s` reaches the armed idle group, a one-frame press never does).
        phase("drawing the sword")
        key("keydown", "s")
        time.sleep(1.5)
        key("keyup", "s")
        drawn = wait_frames(sampler, 0, 1500, until=lambda s: s["pg"] == ARMED_IDLE_GROUP)
        emit({"ev": "sword-drawn", **drawn})
        phase("sword drawn; waiting for the opponent to engage by himself")
        try:
            engaged = wait_frames(sampler, 900, 900, until=lambda s: s["rg"] != NOT_ENGAGED_GROUP)
        except TimeoutError:
            if args.approach == "walk":
                phase("not engaged; walking in with held keys")
                engaged = walk_in(sampler, key, 3000)
            else:
                phase("not engaged; placing the player beside him")
                sh("docker", "exec", name, "python3", "/tmp/orig_write_in.py", str(OPPONENT_ID), "-1")
                wait_frames(sampler, 8, 120)
                engaged = wait_frames(sampler, 1500, 1500, until=lambda s: s["rg"] != NOT_ENGAGED_GROUP)
        phase("engaged; fighting")
        fight_start = engaged["frame"]
        next_state_frame = fight_start
        while True:
            sample, raw = sampler.get()
            if sample is None:
                time.sleep(0.05)
                continue
            frames = sample["frame"] - fight_start
            if sample["frame"] >= next_state_frame:
                # A periodic state row so the original's range and step-back can be set beside the
                # native fight snapshots (same fields: groups, health, distance, positions).
                emit({"ev": "state", "frames": frames, "dist": horizontal_distance(sample),
                      "pdist": player_distance_q8(sample), **sample})
                next_state_frame = sample["frame"] + STATE_EVERY_FRAMES
            if sample["rh"] == 0 or sample["ph"] == 0 or frames >= args.max_frames:
                emit({"ev": "end", "rh": sample["rh"], "ph": sample["ph"], "frames": frames, "fps": sample["fps"],
                      "reason": "kill" if sample["rh"] == 0 else "player-dead" if sample["ph"] == 0 else "frame-budget"})
                break
            if not ready_to_tap(sample, args.gate):
                time.sleep(0.05)
                continue
            sh("docker", "exec", name, "python3", "/tmp/orig_write_in.py", str(OPPONENT_ID), "-1")  # stay in range, facing him
            key("keydown", "Control_L")
            time.sleep(0.6)  # held for a few frames: a one-frame press is missed at this frame rate
            key("keyup", "Control_L")
            after = []
            mark = sampler.get()[0]
            end_frame = (mark or sample)["frame"] + 14
            while True:
                now = sampler.get()[0]
                if now:
                    after.append(now)
                    if now["frame"] >= end_frame:
                        break
                time.sleep(0.05)
            emit({"ev": "tap", "before": sample, **classify_tap(sample, after)})
        sampler.stop()
        sh("docker", "rm", "-f", name)
        log.seal("original-fight run complete")
        with open(args.out) as source:
            rows = [json.loads(line) for line in source]
        print(json.dumps(summarize(rows), indent=1))
        print(f"sealed {args.out}: {verify(args.out)[1]}", flush=True)


if __name__ == "__main__":
    main()
