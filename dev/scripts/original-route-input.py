#!/usr/bin/env python3
"""Play one admitted fixture input block through an original exact-frame session.

The caller owns the stopped original boundary and checkpoint projection. This
adapter neither boots a debug world nor treats a native fixture as parity proof.
"""


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
    held, receipts = set(), []
    try:
        for event in events:
            target = origin + event['after_updates']
            session.advance_to(target)
            state = session.snapshot()
            if state['world'] != world:
                raise RuntimeError(f'original route: world changed from {world} to {state["world"]} at {target}')
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
        return dict(start=start, end=session.snapshot(), inputs=receipts)
    finally:
        for key in sorted(held):
            session.input(key, 'release')


def load_page_ready(raw):
    """Require the constructed retail Load page, not just its pending context."""
    startup = raw.get('startup') or {}
    return (startup.get('menu_context') == 6 and startup.get('menu_page') == 2
            and startup.get('menu_busy') is False
            and bool(startup.get('menu_selection')))


def verify_resumed_checkpoint(raw, expected):
    """Check decoded save observables at the first resumed update boundary.

    The stopped main-loop boundary follows clock advance, before actor updates:
    both accumulated clocks must equal the save plus its retained next step.
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
    for field in ('nominal_scaled_total', 'fine_scaled_total', 'frame_scale', 'next_scale', 'warmup'):
        value = saved[field] + (saved['next_scale'] if field.endswith('_scaled_total') else 0)
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
