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
