#!/usr/bin/env python3
"""Play one admitted fixture input block through an original exact-frame session.

The caller owns the stopped original boundary and checkpoint projection. This
adapter neither boots a debug world nor treats a native fixture as parity proof.
"""

from paired_outcomes import validate_checkpoint, evaluate_checkpoint


def validate_input_block(events, max_updates):
    """Reject malformed or unbounded input before a route changes the original."""
    if type(max_updates) is not int or max_updates < 0:
        raise ValueError('original route: max_updates must be nonnegative')
    previous = -1
    for event in events:
        frame = event.get('after_updates')
        if type(frame) is not int or not 0 <= frame <= max_updates or frame < previous:
            raise ValueError('original route: input offsets must be ordered and bounded')
        previous = frame
        if 'expected' in event:
            validate_checkpoint(event['expected'])
        if 'key' in event and event.get('action') not in ('press', 'release'):
            raise ValueError('original route: key needs press or release')
        if 'key' not in event and 'action' in event:
            raise ValueError('original route: action needs a key')


def play_input_block(session, events, max_updates):
    """Apply fixture after_updates offsets relative to the current stopped frame.

    Validate the whole block before input. Release held keys on completion or
    failure; an unacknowledged input or changed world stops this block immediately.
    """
    validate_input_block(events, max_updates)
    start = session.snapshot()
    origin, world = start['frame'], start['world']
    if type(origin) is not int or origin < 0 or world in (None, -1, (1 << 32) - 1):
        raise ValueError('original route: requires a running world boundary')
    held, receipts, checkpoints = set(), [], []
    try:
        for event in events:
            target = origin + event['after_updates']
            session.advance_to(target)
            state = session.snapshot()
            if state['world'] != world:
                raise RuntimeError(f'original route: world changed from {world} to {state["world"]} at {target}')
            if 'expected' in event:
                fields = evaluate_checkpoint(event['expected'], state, start)
                checkpoints.append(dict(frame=target, fields=fields))
                failed = [field for field in fields if field['status'] != 'match']
                if failed:
                    raise RuntimeError(f'original route: checkpoint at {target} failed: {failed}')
            if 'key' in event:
                key, action = event['key'], event['action']
                # A failed acknowledgement can follow an applied press.
                if action == 'press':
                    held.add(key)
                acknowledged = session.input(key, action)
                if acknowledged != target:
                    raise RuntimeError(f'original route: {key}/{action} expected frame {target}, acknowledged {acknowledged}')
                if action == 'release':
                    held.discard(key)
                receipts.append(dict(frame=target, key=key, action=action, acknowledged=acknowledged))
        return dict(start=start, end=session.snapshot(), inputs=receipts, checkpoints=checkpoints)
    finally:
        for key in sorted(held):
            session.input(key, 'release')


def load_page_ready(raw):
    """Require the constructed retail Load page, not just its pending context."""
    startup = raw.get('startup') or {}
    return (startup.get('menu_context') == 6 and startup.get('menu_page') == 2
            and startup.get('menu_busy') is False
            and bool(startup.get('menu_selection')))


def verify_resumed_checkpoint(raw, expected, *, restored=False):
    """Check decoded save observables at an explicitly identified boundary.

    A loader-completion sample (restored=True) requires untouched saved clocks
    and actors. The legacy ordinary-boundary check includes one clock step and
    only applies to actors unchanged by that step; it is not a restore boundary.
    Reader/decoder bindings are supplied by the caller; no install data is kept.
    """
    if raw.get('world') != expected['world']:
        raise ValueError(f'original reload: world expected {expected["world"]}, observed {raw.get("world")}')
    for actor in expected['actors']:
        matches = [a for a in raw.get('actors', []) if a.get('id') == actor['id'] and a.get('selected') == actor['selected']]
        if len(matches) != 1:
            raise ValueError(f'original reload: actor {actor["id"]} missing or duplicated')
        for field in ('selected', 'position_q8', 'yaw', 'health', 'group'):
            if matches[0].get(field) != actor[field]:
                raise ValueError(f'original reload: actor {actor["id"]} {field} expected {actor[field]}, observed {matches[0].get(field)}')
    saved = expected['clock_continuation']
    observed = (raw.get('counters') or {}).get('clock_continuation') or {}
    fields = saved if restored else ('nominal_scaled_total', 'fine_scaled_total', 'frame_scale', 'next_scale', 'warmup')
    for field in fields:
        value = saved[field] + (saved['next_scale'] if not restored and field.endswith('_scaled_total') else 0)
        if observed.get(field) != value:
            raise ValueError(f'original reload: clock {field} expected {value}, observed {observed.get(field)}')


def resume_checkpoint(boundary, confirm, expected, max_boundaries=2, timeout=45):
    """Confirm retail Load and stop at the first matching restored boundary.

    confirm(bool) controls the menu Return key. The caller owns the watchpoint
    and must keep it stopped on return. Only the menu-exit boundary may precede
    the restored boundary; never advance past a mismatched resumed save.
    """
    if type(max_boundaries) is not int or not 1 <= max_boundaries <= 2:
        raise ValueError('original reload: requires one or two bounded boundaries')
    if not load_page_ready(boundary.raw_snapshot()):
        raise ValueError('original reload: retail Load page is not ready')
    try:
        confirm(True)
        for _ in range(max_boundaries):
            boundary.next_boundary(timeout)
            raw = boundary.raw_snapshot()
            confirm(False)
            # Menu exit can precede restoration by one boundary. The saved
            # accumulated clock identifies that continuation without relying
            # on a monotonic frame counter across Load.
            clock = (raw.get('counters') or {}).get('clock_continuation') or {}
            saved = expected['clock_continuation']
            if clock.get('nominal_scaled_total') == saved['nominal_scaled_total'] + saved['next_scale']:
                verify_resumed_checkpoint(raw, expected)
                return raw
        raise ValueError('original reload: saved continuation not observed within boundary budget')
    finally:
        confirm(False)


def restore_checkpoint(boundary, confirm, expected, stages, ordinary_binding, *, timeout=45):
    """Stop at an evidenced restored-state boundary before ordinary updates.

    Each local (guest_offset, write_count) stage identifies an evidenced loader
    or pre-clock boundary. Retained globals can precede actor restoration, so
    saved clock values alone cannot identify completion. Follow the finite
    binding sequence, then require every saved actor and clock field.
    Rebind the ordinary watchpoint while still stopped; only the caller's next
    explicit advance executes the first resumed update.
    """
    stages = list(stages)
    if not stages or any(type(count) is not int or count < 1 for _, count in stages):
        raise ValueError('original restore: requires a nonempty positive write-count sequence')
    if not load_page_ready(boundary.raw_snapshot()):
        raise ValueError('original restore: retail Load page is not ready')
    boundary.select_boundary(stages[0][0])
    try:
        confirm(True)
        for index, (binding, count) in enumerate(stages):
            if index:
                boundary.select_boundary(binding)
            for _ in range(count):
                boundary.next_boundary(timeout)
                raw = boundary.raw_snapshot()
                confirm(False)
        verify_resumed_checkpoint(raw, expected, restored=True)
        return raw
    finally:
        confirm(False)
        if boundary.stopped:
            boundary.select_boundary(ordinary_binding)


def _verify_pending_item(raw, expected_world, expected_item, frame):
    view = (raw.get('counters') or {}).get('item_view') or {}
    if (raw.get('world') != expected_world or (expected_item is not None and view.get('item') != expected_item)
            or view.get('pending') is not True
            or (raw.get('counters') or {}).get('mainloop_frame') != frame):
        raise ValueError(f'original item view: expected pending item request world {expected_world}, item {expected_item}, frame {frame}; observed world {raw.get("world")}, view {view}, frame {(raw.get("counters") or {}).get("mainloop_frame")}')


def dismiss_scripted_item(boundary, expected_world, expected_item, request_binding,
                          clock_binding, ordinary_binding, *, opening_updates,
                          key='Escape', request_updates=0, timeout=45):
    """Dismiss a script-requested viewer with acknowledged modal-clock input.

    The caller binds the pending-item write and modal clock, and supplies the
    documented fly-in update count. Ordinary frames do not advance inside the
    viewer. Wait for the request, finish its opening updates, hold the close
    key across one modal update, then release and observe ordinary resumption.
    """
    if type(opening_updates) is not int or opening_updates < 1:
        raise ValueError('original item view: requires positive opening updates')
    if type(request_updates) is not int or request_updates < 0:
        raise ValueError('original item view: request update budget must be nonnegative')
    start = boundary.raw_snapshot()
    frame = (start.get('counters') or {}).get('mainloop_frame')
    if start.get('world') != expected_world or type(frame) is not int:
        raise ValueError('original item view: unexpected starting world or missing frame')
    held = False
    try:
        if not ((start.get('counters') or {}).get('item_view') or {}).get('pending'):
            boundary.select_boundary(request_binding)
            boundary.next_boundary(timeout)
        request = boundary.raw_snapshot()
        request_frame = (request.get('counters') or {}).get('mainloop_frame')
        if type(request_frame) is not int or not frame <= request_frame <= frame + request_updates:
            raise ValueError('original item view: request exceeded ordinary update budget')
        frame = request_frame
        _verify_pending_item(request, expected_world, None, frame)
        boundary.select_boundary(clock_binding)
        for _ in range(opening_updates):
            boundary.next_boundary(timeout)
            _verify_pending_item(boundary.raw_snapshot(), expected_world, expected_item, frame)
        held = True
        boundary.latch_key(key, 'press')
        boundary.next_boundary(timeout)
        boundary.latch_key(key, 'release')
        held = False
        boundary.select_boundary(ordinary_binding)
        boundary.next_boundary(timeout)
        end = boundary.raw_snapshot()
        if (end.get('world') != expected_world
                or (end.get('counters') or {}).get('mainloop_frame') != frame + 1
                or ((end.get('counters') or {}).get('item_view') or {}).get('pending') is not False):
            raise ValueError('original item view: ordinary resumption not acknowledged')
        return end
    finally:
        if held and not boundary.stopped:
            boundary.pause()
        if boundary.stopped:
            try:
                if held:
                    boundary.latch_key(key, 'release')
            finally:
                boundary.select_boundary(ordinary_binding)


def play_checkpointed_phase(session, name, desired_keys, complete, max_updates):
    """Resume one same-world phase, requiring an observed progression checkpoint.

    Policies inspect every stopped update, return the keys to hold, and derive
    completion from game state, never input receipts. A satisfied checkpoint is
    idempotent. Unexpected world changes and missed inputs abort immediately.
    """
    if not name or type(max_updates) is not int or max_updates < 0:
        raise ValueError('original route: phase needs a name and nonnegative update budget')
    start = session.snapshot()
    origin, world = start['frame'], start['world']
    if type(origin) is not int or origin < 0 or world in (None, -1, (1 << 32) - 1):
        raise ValueError('original route: requires a running world boundary')
    held, receipts = set(), []
    try:
        for elapsed in range(max_updates + 1):
            if elapsed:
                session.advance_to(origin + elapsed)
            state = session.snapshot()
            if state['world'] != world:
                raise RuntimeError(f'original route: {name} world changed from {world} at {origin} to {state["world"]} at {state["frame"]}')
            if complete(state):
                return dict(checkpoint=name, start=start, end=state, inputs=receipts)
            if elapsed == max_updates:
                raise RuntimeError(f'original route: {name} checkpoint not reached within {max_updates} updates')
            desired = set(desired_keys(state))
            for key, action in ([(k, 'release') for k in sorted(held - desired)]
                                + [(k, 'press') for k in sorted(desired - held)]):
                if action == 'press':
                    held.add(key)
                acknowledged = session.input(key, action)
                if acknowledged != state['frame']:
                    raise RuntimeError(f'original route: {key}/{action} expected frame {state["frame"]}, acknowledged {acknowledged}')
                if action == 'release':
                    held.discard(key)
                receipts.append(dict(frame=state['frame'], key=key, action=action, acknowledged=acknowledged))
    finally:
        for key in sorted(held):
            session.input(key, 'release')


def aimed_combat_keys(state, opponent_id, attack, attack_keys):
    """Steer an original retail route toward an opponent before attacking.

    This is a driver policy, not combat simulation. The caller supplies the
    installed first motion's Q8 ranges and angular arc, and the admitted attack
    tape's current keys. Turning and approach replace attack input; once in the
    authored window the turn key is released instead of spinning through hits.
    Headings use 2048 units per turn and negative X is a left turn from +Z.
    """
    import math
    player, = [a for a in state['actors'] if a['selected']]
    opponent, = [a for a in state['actors'] if a['id'] == opponent_id and not a['selected']]
    dx, dy, dz = [v - u for u, v in zip(player['position_q8'], opponent['position_q8'])]
    distance = round(math.sqrt(dx * dx + dy * dy + dz * dz))
    bearing = round(math.atan2(-dx, dz) * 1024 / math.pi) % 2048
    error = (bearing - player['yaw'] + 1024) % 2048 - 1024
    if distance <= attack['min_range']:
        return set()
    if abs(error) >= attack['arc']:
        return {'ArrowLeft' if error > 0 else 'ArrowRight'}
    if distance >= attack['max_range']:
        return {'ArrowUp'}
    return set(attack_keys)


def waypoint_reached(state, waypoint):
    """Observe a caller-supplied horizontal Q8 disk and vertical clearance."""
    player, = [a for a in state['actors'] if a['selected']]
    dx, dy, dz = [v - u for u, v in zip(player['position_q8'], waypoint['position_q8'])]
    return dx * dx + dz * dz <= waypoint['radius_q8'] ** 2 and abs(dy) <= waypoint['height_q8']


def waypoint_keys(state, waypoint, turn_window):
    """Aim before moving; the caller supplies a measured turn window in yaw units.

    This is a navigation policy, not player simulation. Heading uses 2048 units
    per turn, with negative X left of +Z. No heading or position is written.
    """
    import math
    player, = [a for a in state['actors'] if a['selected']]
    if player['lockout'] != 0:
        raise ValueError('original walk: player control is locked')
    dx, _, dz = [v - u for u, v in zip(player['position_q8'], waypoint['position_q8'])]
    bearing = round(math.atan2(-dx, dz) * 1024 / math.pi) % 2048
    error = (bearing - player['yaw'] + 1024) % 2048 - 1024
    if abs(error) > turn_window:
        return {'ArrowLeft' if error > 0 else 'ArrowRight'}
    return {'ArrowUp'} | ({'ShiftLeft'} if waypoint['modifier'] else set())


def play_walk_route(session, waypoints, *, resume=0, turn_window, checkpoint=lambda receipt: None):
    """Resume an ordered route, acknowledging each measured waypoint separately.

    Waypoints carry names, Q8 position/radius/height, gait modifier and update
    budget from the caller's route evidence. Never infer completed legs from a
    nearest point. The returned next index may be persisted with its checkpoint;
    a rejected leg never produces a completion receipt. The checkpoint callback
    publishes each completed leg before starting the next, even if that leg fails.
    """
    waypoints = list(waypoints)
    if type(resume) is not int or not 0 <= resume <= len(waypoints):
        raise ValueError('original walk: invalid resume index')
    if type(turn_window) is not int or not 0 < turn_window < 1024:
        raise ValueError('original walk: invalid turn window')
    for point in waypoints:
        if (not point.get('name') or len(point.get('position_q8', [])) != 3
                or any(type(v) is not int for v in point['position_q8'])
                or any(type(point.get(k)) is not int or point[k] < 0
                       for k in ('radius_q8', 'height_q8', 'max_updates'))
                or point['radius_q8'] == 0 or type(point.get('modifier')) is not bool):
            raise ValueError('original walk: invalid waypoint')
    receipts = []
    for index in range(resume, len(waypoints)):
        point = waypoints[index]
        receipt = play_checkpointed_phase(session, point['name'],
                    lambda state: waypoint_keys(state, point, turn_window),
                    lambda state: waypoint_reached(state, point)
                        and next(a for a in state['actors'] if a['selected'])['lockout'] == 0,
                    point['max_updates'])
        receipt = dict(receipt, next_index=index + 1)
        checkpoint(receipt)
        receipts.append(receipt)
    return receipts
