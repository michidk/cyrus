#!/usr/bin/env python3
"""Exact-frame comparison adapter over the original stopped-boundary reader.

The caller owns boot/staging/cleanup and provides Watchpoint plus project(raw).
No polling, interpolation or OS-key acknowledgement establishes acceptance here.
"""


class FrameSession:
    """Keep the game stopped between inputs and read every intervening boundary.

    counter(raw) returns the verified game frame. project(raw) returns normalized
    comparison observables. emit(raw) optionally retains every batched frame read.
    The caller must already have stopped at the initial boundary.
    """

    def __init__(self, boundary, counter, project, emit=lambda raw: None, timeout=10):
        self.boundary, self.counter, self.project = boundary, counter, project
        self.emit, self.timeout = emit, timeout
        self.current = boundary.raw_snapshot()
        self.origin = self._frame(self.current)
        self.emit(self.current)

    def _frame(self, raw):
        frame = self.counter(raw)
        if type(frame) is not int or frame < 0:
            raise ValueError('original frame counter unavailable')
        return frame

    def snapshot(self):
        return dict(self.project(self.current), frame=self._frame(self.current) - self.origin)

    def advance_to(self, frame):
        current = self._frame(self.current) - self.origin
        if type(frame) is not int or frame < current:
            raise ValueError(f'original frame {frame} already passed or invalid')
        while current < frame:
            self.boundary.next_boundary(self.timeout)
            raw = self.boundary.raw_snapshot()
            observed = self._frame(raw) - self.origin
            if observed != current + 1:
                raise ValueError(f'original frame gap: expected {current + 1}, observed {observed}')
            self.current = raw
            self.emit(raw)
            current = observed

    def wait_for_world(self, target_world, max_updates, checkpoints=()):
        """Observe an explicitly admitted world transition without sending input.

        Source-world frames must remain consecutive. Only the requested world
        may end the wait; its first boundary becomes frame zero for subsequent
        input, allowing the world loop to replace its counter/clock continuation.
        Ordinary advance_to retains its strict no-reset contract. Optional
        ordered (name, predicate) checkpoints must be observed in the source
        world before arrival. An already loaded target world skips the phase.
        Arrival acknowledges the transition, not target-world scene readiness.
        """
        absent = (None, -1, (1 << 32) - 1)
        if type(max_updates) is not int or max_updates < 0:
            raise ValueError('world transition requires a nonnegative update budget')
        if type(target_world) is not int or target_world in absent or target_world < 0:
            raise ValueError('world transition requires a valid target world')
        start = self.snapshot()
        source = start.get('world')
        if source in absent:
            raise ValueError('world transition requires a running source world')
        if source == target_world:
            return dict(start=start, end=start, updates=0)
        pending, seen = list(checkpoints), []
        def observe(elapsed):
            if pending and pending[0][1](self.snapshot()):
                name, _ = pending.pop(0)
                seen.append(dict(name=name, updates=elapsed))
        observe(0)
        for elapsed in range(1, max_updates + 1):
            previous = self._frame(self.current)
            self.boundary.next_boundary(self.timeout)
            raw = self.boundary.raw_snapshot()
            observed = self._frame(raw)
            world = self.project(raw).get('world')
            if world == target_world:
                if pending:
                    raise ValueError(f'world transition missing checkpoint {pending[0][0]} before world {target_world}')
                self.current, self.origin = raw, observed
                self.emit(raw)
                receipt = dict(start=start, end=self.snapshot(), updates=elapsed)
                if checkpoints:
                    receipt['checkpoints'] = seen
                return receipt
            if world != source:
                raise ValueError(f'world transition expected world {target_world}, observed {world}')
            if observed != previous + 1:
                raise ValueError(f'original frame gap: expected {previous + 1}, observed {observed}')
            self.current = raw
            self.emit(raw)
            observe(elapsed)
        raise TimeoutError(f'world {target_world} not reached within {max_updates} updates')

    def input(self, key, action):
        before = self._frame(self.current)
        raw = self.boundary.latch_key(key, action)
        if self._frame(raw) != before:
            raise ValueError('original frame changed during key acknowledgement')
        self.current = raw
        return before - self.origin


class ActorObserver:
    """Read retained actor identities independently of view-centered list membership.

    The local reader supplies read_actor(pointer) and the verified pointer and
    script_data identity fields. An observation is not update-list admission.
    World changes invalidate all bindings before any old allocation is read.
    """

    def __init__(self, identifiers):
        self.identifiers = frozenset(identifiers)
        self.world = None
        self.bindings = {}

    def observe(self, world, listed, read_actor):
        if world != self.world:
            self.bindings.clear()
            self.world = world
        members = {actor['pointer'] for actor in listed}
        for actor in listed:
            if actor['id'] in self.identifiers and not actor['selected']:
                identity = (actor['pointer'], actor['script_data'])
                if self.bindings.setdefault(actor['id'], identity) != identity:
                    raise ValueError(f"observed actor {actor['id']} identity changed in world {world}")
        observed = []
        for identifier, (pointer, script) in self.bindings.items():
            actor = read_actor(pointer)
            if (actor['id'], actor['pointer'], actor['script_data']) != (identifier, pointer, script):
                raise ValueError(f"observed actor {identifier} identity changed in world {world}")
            observed.append(dict(actor, in_update_list=pointer in members))
        return observed
