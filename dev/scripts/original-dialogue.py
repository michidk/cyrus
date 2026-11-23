#!/usr/bin/env python3
"""Select installed original-game topics through an acknowledged frame session.

The caller supplies reader bindings and installed label/action pairs. A key
receipt is not topic completion: the menu must latch select, then commit the
requested item on release. No memory writes other than ordinary key state.
"""


def _change_keys(session, held, desired, inputs):
    for key, operation in ([(k, 'release') for k in sorted(held - desired)]
                           + [(k, 'press') for k in sorted(desired - held)]):
        if operation == 'press':
            held.add(key)
        frame = session.snapshot()['frame']
        acknowledged = session.input(key, operation)
        inputs.append(dict(frame=frame, key=key, action=operation, acknowledged=acknowledged))
        if acknowledged != frame:
            raise RuntimeError(f'original topic: {key}/{operation} expected frame {frame}, acknowledged {acknowledged}')
        if operation == 'release':
            held.remove(key)


def select_topic(session, label, action, max_updates=120, *, select_key='ControlLeft', observe_commit=None):
    """Navigate a stable live menu and require its release/commit handshake.

    session snapshots expose dialogue_menu: active, items (label/action),
    highlighted, committed (item), and select_pending. Navigation pulses down
    for one explicit update, then releases to reset the menu's repeat latch.
    Every transition, including cleanup releases, is acknowledged and retained.
    A stopped-boundary adapter may supply observe_commit(): it observes the
    actual release-commit event before a synchronous next menu overwrites the
    table, returning label/action/frame at that source frame. The next ordinary
    frame must still advance successfully before this receipt is accepted.
    """
    if not isinstance(label, str) or not label or type(action) is not int:
        raise ValueError('original topic: requires installed label and integer action')
    if type(max_updates) is not int or max_updates < 1:
        raise ValueError('original topic: requires a positive update budget')
    start = session.snapshot()
    world, origin = start['world'], start['frame']
    if world in (None, -1, (1 << 32) - 1):
        raise ValueError('original topic: requires a running world')
    menu = start['dialogue_menu']
    identity = lambda m: [(item['label'], item['action']) for item in m['items']]
    entries = identity(menu)
    if not menu['active'] or entries.count((label, action)) != 1:
        raise ValueError(f'original topic: {label}/{action} absent or ambiguous in active menu')
    target = entries.index((label, action))
    held, inputs, phase, event = set(), [], 'navigate', None

    change = lambda desired: _change_keys(session, held, desired, inputs)

    try:
        for elapsed in range(max_updates + 1):
            state = session.snapshot()
            if state['world'] != world:
                raise RuntimeError(f'original topic: {label}/{action} world changed at frame {state["frame"]}')
            menu = state['dialogue_menu']
            if phase == 'commit' and (event is not None or not menu['active']):
                committed = event if event is not None else menu['committed']
                if (committed['label'], committed['action']) != (label, action):
                    raise RuntimeError(f'original topic: {label}/{action} committed {committed}')
                return dict(label=label, action=action, start=start, end=state, inputs=inputs, commit_event=event)
            if not menu['active'] or identity(menu) != entries:
                raise RuntimeError(f'original topic: {label}/{action} menu changed before commitment')
            if elapsed == max_updates:
                raise RuntimeError(f'original topic: {label}/{action} not committed within {max_updates} updates ({phase})')
            if phase == 'navigate':
                if not 0 <= menu['highlighted'] < len(entries):
                    raise ValueError('original topic: highlighted index outside live menu')
                if menu['highlighted'] == target:
                    change({select_key})
                    phase = 'pressed'
                else:
                    change(set() if held else {'ArrowDown'})
            elif phase == 'pressed':
                if menu['highlighted'] != target:
                    raise RuntimeError('original topic: highlight changed while select held')
                if menu['select_pending']:
                    change(set())
                    if observe_commit is not None:
                        event = observe_commit()
                        if event.get('frame') != state['frame']:
                            raise RuntimeError('original topic: commit event has wrong source frame')
                    phase = 'commit'
            session.advance_to(origin + elapsed + 1)
    finally:
        boundary = getattr(session, 'boundary', None)
        if held and boundary is not None and not boundary.stopped:
            boundary.pause()
        change(set())
