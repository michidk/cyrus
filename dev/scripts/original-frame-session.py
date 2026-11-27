#!/usr/bin/env python3
"""Exact-frame comparison adapter over the original stopped-boundary reader.

The caller owns boot/staging/cleanup and provides Watchpoint plus project(raw).
No polling, interpolation or OS-key acknowledgement establishes acceptance here.
"""


import copy

from original_acquisition import OriginalAcquisition, RandomWriteHistory


def acquire_entry(boundary, counter, target_start, *, emit=lambda raw: None,
                  timeout=10, max_observations=64):
    """Stop at an exact ordinary-update start without a settling/polling run.

    Attach before the loader releases the zero counter. Repeated loader zero
    writes are retained; after the first ordinary start every counter must be
    consecutive. The caller supplies the verified boundary binding and target
    phase. A start counter is not a completed-update count. Emitted raw records
    preserve RNG and timing history without modifying either.
    """
    if type(target_start) is not int or target_start < 1:
        raise ValueError('entry acquisition requires a positive ordinary-start target')
    if type(max_observations) is not int or max_observations < 1:
        raise ValueError('entry acquisition requires a positive observation budget')
    raw = boundary.raw_snapshot()
    previous = counter(raw)
    emit(raw)
    if type(previous) is not int or previous != 0:
        raise ValueError(f'entry acquisition attached late: expected loader zero, observed {previous}')
    for _ in range(max_observations):
        boundary.next_boundary(timeout)
        raw = boundary.raw_snapshot()
        observed = counter(raw)
        emit(raw)
        if type(observed) is not int or not (
                observed == previous + 1 or previous == observed == 0):
            raise ValueError(f'entry acquisition frame gap: expected {previous + 1}, observed {observed}')
        if observed == target_start:
            return raw
        previous = observed
    raise TimeoutError(f'entry start {target_start} not reached within {max_observations} observations')


def acquire_shortcut_entry(boundary, counter, completed_updates, key, wait_book,
                           **acquisition):
    """Latch a retail shortcut during the last requested ordinary body.

    Preservation's engine/menu.md specifies end-of-update shortcut sampling
    and consumption near the next update's start. Requesting it after all
    requested bodies have completed admits an unwanted extra body. key(action)
    sends the supported make/break; wait_book() returns the stopped, acknowledged
    installed book page using caller-owned bindings. Its counter must prove
    exactly the requested bodies completed. Input is released on every exit.
    """
    acquire_entry(boundary, counter, completed_updates, **acquisition)
    try:
        key('press')
        raw = wait_book()
        observed = counter(raw)
        if type(observed) is not int or observed != completed_updates + 1:
            raise ValueError(f'retail shortcut acquisition expected start {completed_updates + 1}, '
                             f'observed {observed}; ordinary-body history differs')
        return raw
    finally:
        key('release')


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
        """Retain acknowledged key state at the stopped frame before advancing."""
        before = self._frame(self.current)
        raw = self.boundary.latch_key(key, action)
        if self._frame(raw) != before:
            raise ValueError('original frame changed during key acknowledgement')
        self.current = raw
        self.emit(raw)
        return before - self.origin


class AcquiredFrameSession(FrameSession):
    """Require original acquisition evidence before exposing combat controls.

    project(raw) must expose world and random_state at the restored entry.
    Full semantic admission remains the paired driver's responsibility.
    """

    def __init__(self, boundary, counter, project, acquisition, **options):
        if not isinstance(acquisition, OriginalAcquisition):
            raise ValueError('original combat entry requires bound acquisition history')
        self.acquisition = acquisition
        super().__init__(boundary, counter, project, **options)
        acquisition.require_entry(self.snapshot())
        acquisition.require_native_admission()

    def acquisition_evidence(self):
        return self.acquisition.evidence()

    def input(self, key, action):
        self.acquisition.require_native_admission()
        return super().input(key, action)

    def advance_to(self, frame):
        self.acquisition.require_native_admission()
        return super().advance_to(frame)

    def wait_for_world(self, target_world, max_updates, checkpoints=()):
        self.acquisition.require_native_admission()
        return super().wait_for_world(target_world, max_updates, checkpoints)


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


def project_native_source(kind, observed_state, *, owner=None):
    """Project an observed source independently of the loaded owner census.

    The executable binding supplies decoded attributes, Q8 position, orientation
    and script context with normalized cursors/local offsets, never allocation
    tokens or record bytes. Unobserved fields stay None and reject paired
    comparison. This does not resolve, read or infer any departed allocation.

    Preservation's separate-object script-state contract establishes the shared
    temporary's process-global lifetime. Shell lifetime is not inferred here.
    Registry actors still require an independently bound semantic owner identity.
    """
    if kind not in ('separate_temporary', 'registry_actor'):
        raise ValueError('native source storage is unsupported or unobserved')
    if not isinstance(observed_state, dict):
        raise ValueError('native source requires decoded observed state')
    for field in ('attributes', 'position_q8', 'orientation', 'script_context'):
        if field not in observed_state:
            raise ValueError(f'native source missing observation: {field}')
    result = dict(kind=kind, state=copy.deepcopy(observed_state))
    if kind == 'separate_temporary':
        if owner is not None:
            raise ValueError('shared temporary source is not a registry owner')
        result['lifetime'] = 'process_global'
    else:
        if not isinstance(owner, dict) or not owner:
            raise ValueError('registry source requires a bound semantic owner identity')
        result['owner'] = copy.deepcopy(owner)
    return result


class OwnerReferenceObserver:
    """Correlate a retained reference with an observed owner census across loads.

    Tokens are opaque allocation identities supplied by the executable binding,
    not actor indices. Each census entry supplies token and semantic identity.
    Missing census membership establishes neither deallocation nor continued
    lifetime. This observer never dereferences an absent owner or maps it by slot.
    """

    def __init__(self):
        self.source = None
        self.owner = None

    def observe(self, world, source, owners):
        if type(world) is not int or type(source) is not int or source < 0:
            raise ValueError('owner reference requires observed world and nonnegative source token')
        census = {}
        for owner in owners:
            if (not isinstance(owner, dict) or type(owner.get('token')) is not int
                    or owner['token'] <= 0 or not isinstance(owner.get('identity'), dict)
                    or not owner['identity']):
                raise ValueError('owner census requires token and semantic identity')
            if owner['token'] in census:
                raise ValueError('duplicate owner token in census')
            census[owner['token']] = dict(world=world, identity=copy.deepcopy(owner['identity']))
        current = census.get(source)
        previous = copy.deepcopy(self.owner)
        if source == 0:
            status = 'null'
        elif current is None:
            status = 'unlisted'
        elif self.source not in (None, 0, source):
            status = 'reassigned'
        elif previous is not None and current != previous:
            status = 'identity_changed'
        else:
            status = 'listed'
        if source != self.source:
            self.owner = None
        if current is not None:
            self.owner = copy.deepcopy(current)
        self.source = source
        return dict(status=status, source=source, previous=previous,
                    current=copy.deepcopy(current), allocation_lifetime='unobserved')
