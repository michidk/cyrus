#!/usr/bin/env python3
"""Source-bound original acquisition evidence, not a game-state restoration API."""
import hashlib
import json
from pathlib import Path

from copy import deepcopy


class RandomWriteHistory:
    """Retain consecutive observed engine RNG writes, never endpoint estimates.

    The wrapping transition is published in preservation's engine/SOUP.md.
    Bind the real state write while stopped; call once per write with the
    original acquisition phase and measured timer context. Initial state is an
    observation, not a seed instruction. Missing writes, resets and duplicates
    reject the tape. This observer never writes engine state.
    """

    def __init__(self, initial_state, emit=lambda row: None):
        if type(initial_state) is not int or not 0 <= initial_state < 2**32:
            raise ValueError('original RNG history requires an observed u32 state')
        self.state, self.draw, self.emit = initial_state, 0, emit
        self.boundaries = {}

    def observe(self, state, phase, context):
        if not isinstance(phase, str) or not phase or not isinstance(context, dict):
            raise ValueError('original RNG write requires a named phase and measured context')
        expected = (self.state * 1103515245 + 12345) % 2**32
        if type(state) is not int or state != expected:
            raise ValueError(f'original RNG history gap during {phase}: '
                             f'expected {expected}, observed {state}')
        row = dict(draw=self.draw + 1, previous=self.state, state=state,
                   phase=phase, context=deepcopy(context))
        self.emit(row)
        self.state, self.draw = state, self.draw + 1
        return row

    def boundary(self, name, state, context):
        """Bind a stopped semantic boundary to the continuously observed writes.

        A matching endpoint alone does not prove a call count. The count here
        comes only from observe(); a missed final write rejects this boundary.
        Names identify lifecycle phases independently of mutable frame counters.
        """
        if (not isinstance(name, str) or not name or not isinstance(context, dict)
                or type(state) is not int or not 0 <= state < 2**32):
            raise ValueError('RNG boundary requires a name, observed u32 state and context')
        if name in self.boundaries:
            raise ValueError(f'RNG boundary {name} already observed')
        if state != self.state:
            raise ValueError(f'RNG boundary {name}: unobserved RNG write; '
                             f'expected {self.state}, observed {state}')
        row = dict(name=name, state=state, draw=self.draw, context=deepcopy(context))
        self.boundaries[name] = row
        return deepcopy(row)

    def require_boundary(self, name, state):
        """Compare an independently measured endpoint at the same named boundary.

        Other observed phases can explain a mismatch, never authorize it. This
        does not infer the other engine's draw count from its endpoint.
        """
        if name not in self.boundaries:
            raise ValueError(f'RNG boundary {name} was not observed')
        expected = self.boundaries[name]
        if type(state) is not int or state != expected['state']:
            matches = [key for key, row in self.boundaries.items()
                       if type(state) is int and row['state'] == state]
            detail = f'; matches observed phase(s) {matches}' if matches else ''
            raise ValueError(f'RNG boundary {name}: expected {expected["state"]} '
                             f'after {expected["draw"]} observed writes, observed {state}{detail}')
        return deepcopy(expected)


def next_bios_input(row):
    """Return observed BIOS input for this publication's forthcoming movement.

    The local observer captures the old baseline and remainder at the engine's
    sample write, then the first remainder addition before window reduction.
    Blocking restores have already replaced the baseline. The engine reads the
    timer again for that addition, so subtracting the earlier sample can miss
    ticks, especially while stopped under a debugger. No guest data is changed.
    """
    measurement = row.get('bios_measurement')
    if not isinstance(measurement, dict):
        raise ValueError('forthcoming BIOS input requires a measurement observation')
    names = ('sample', 'baseline', 'remainder_before', 'remainder_after')
    if (measurement.get('phase') != 'before_publication'
            or any(type(measurement.get(name)) is not int
                   or not 0 <= measurement[name] < 2**32 for name in names)
            or measurement['remainder_before'] >= 18
            or measurement['sample'] != row.get('bios_ticks')
            or measurement['sample'] != row.get('clock_publication', {}).get('bios_sample')):
        raise ValueError('BIOS measurement does not bind this publication phase')
    elapsed = measurement['remainder_after'] - measurement['remainder_before']
    sampled_elapsed = (measurement['sample'] - measurement['baseline']) % 2**32
    if elapsed < sampled_elapsed or elapsed >= 2**31:
        raise ValueError('BIOS measurement addition is missing, reduced or clamped')
    if elapsed != sampled_elapsed:
        raise ValueError('timer advanced between BIOS reads; single-sample replay is unsupported')
    return elapsed


def validate_bios_origin_tape(samples, bios_ticks):
    """Reject an origin-only setup when measured absolute samples need later rebasing.

    Effective intervals remain replay inputs. Absolute sample gaps cannot replace
    them: a discontinuity needs a separate supported clock control or recording.
    """
    if not isinstance(samples, (list, tuple)) or not isinstance(bios_ticks, (list, tuple)) or len(samples) != len(bios_ticks) + 1:
        raise ValueError('BIOS origin tape requires entry plus one absolute sample per interval')
    if any(type(value) is not int or not 0 <= value < 2**32 for value in (*samples, *bios_ticks)):
        raise ValueError('BIOS origin tape requires unsigned samples and effective intervals')
    for frame, interval in enumerate(bios_ticks, 1):
        predicted = (samples[frame - 1] + interval) % 2**32
        if samples[frame] != predicted:
            raise ValueError(f'BIOS origin-only replay discontinuity at body {frame}: '
                             f'observed sample {samples[frame]}, prior plus effective interval {predicted}; '
                             'absolute sample gaps must not replace effective BIOS inputs')
    return samples[0]


def validate_recording_inputs(events, terminal_update):
    """Require the complete acknowledged input tape, including terminal releases."""
    if type(terminal_update) is not int or terminal_update < 0 or not isinstance(events, list):
        raise ValueError('recording inputs require a nonnegative terminal update and event list')
    previous, held = 0, set()
    for event in events:
        if not isinstance(event, dict):
            raise ValueError('recording input must be an event object')
        frame, key, action = event.get('frame'), event.get('key'), event.get('action')
        if type(frame) is not int or not previous <= frame <= terminal_update:
            raise ValueError('recording horizon omits or reorders an input acknowledgement')
        if not isinstance(key, str) or not key or action not in ('press', 'release'):
            raise ValueError('recording input requires a named key and press/release action')
        if action == 'press':
            held.add(key)
        else:
            held.discard(key)
        previous = frame
    if held:
        raise ValueError(f'recording inputs lack terminal releases: {sorted(held)}')


def debugger_execution_timing(events):
    """Validate buffered resume/stop pairs; report host durations, never game ticks.

    A batch begins with an already-stopped tracee. Initial stopped time is not
    inferred; only pauses bounded by two events within the batch are summed.
    Stops are timestamped when wait returns, so resume-to-stop spans include
    stop-detection latency rather than measuring tracee execution alone.
    """
    if not isinstance(events, list):
        raise ValueError('debugger execution events must be a list')
    expected, previous, running, stopped = 'resume', None, 0, 0
    for event in events:
        if not isinstance(event, dict) or event.get('kind') != expected:
            raise ValueError('debugger execution events must alternate resume and stop')
        stamp = event.get('monotonic_ns')
        if type(stamp) is not int or stamp < 0 or (previous is not None and stamp < previous):
            raise ValueError('debugger execution timestamps must be monotonic unsigned nanoseconds')
        if previous is not None:
            if expected == 'stop':
                running += stamp - previous
            else:
                stopped += stamp - previous
        previous = stamp
        expected = 'stop' if expected == 'resume' else 'resume'
    if expected != 'resume':
        raise ValueError('debugger recording ended while tracee was running')
    return dict(resume_to_observed_stop_ns=running, internal_observed_stop_ns=stopped)


def continuous_bios_inputs(entry_sample, measurements):
    """Build origin-alignable inputs only from both independently observed reads.

    Measurement baselines may be restored between calculations. Such a tape is
    still diagnostic evidence, but cannot pass a continuous-origin contract.
    """
    if not measurements:
        raise ValueError('continuous BIOS tape requires observed measurements')
    intervals = [next_bios_input(row) for row in measurements]
    samples = [entry_sample] + [row['bios_ticks'] for row in measurements]
    validate_bios_origin_tape(samples, intervals)
    return dict(bios_ticks=intervals, cached_bios_samples=samples)


def file_digest(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


class OriginalAcquisition:
    """Validate an original tape and bind it to an existing verified native artifact.

    The shared next_bios_input validator checks the tape's actual measurement
    witnesses, not diagnostic sample gaps.
    Replay inputs are expectations for natural native execution; no RNG setter
    or manufactured loading draw is supplied. Files and artifact identity are
    checked again before use. Installed bindings and raw tapes stay local.
    """

    def __init__(self, path, executable, save, artifact):
        self.path, self.executable, self.save = map(Path, (path, executable, save))
        self.artifact = artifact
        self.native_admission = None
        self.identity = deepcopy(artifact.validate())
        self.tape_digest = file_digest(self.path)
        try:
            tape = json.loads(self.path.read_text())
            if type(tape['version']) is not int or tape['version'] != 1:
                raise ValueError('unsupported original acquisition version')
            self.original = deepcopy(tape['original'])
            entry = tape['entry']
            if (entry['route'] != 'retail_introduction'
                    or not isinstance(entry['scene'], str) or not entry['scene']
                    or type(entry['world']) is not int or not 0 <= entry['world'] < 2**32 - 1
                    or type(entry['marker']) is not int or entry['marker'] < 0):
                raise ValueError('original acquisition requires the observed retail Introduction entry')
            if type(tape['initial_random_state']) is not int or tape['initial_random_state'] != 1:
                raise ValueError('original acquisition must observe startup before any RNG draw')
            history = RandomWriteHistory(tape['initial_random_state'])

            def section(value, counter):
                for row in value['writes']:
                    if (type(row['draw']) is not int or row['draw'] != history.draw + 1
                            or type(row['previous']) is not int or row['previous'] != history.state
                            or type(row['context']['counter']) is not int
                            or row['context']['counter'] != counter):
                        raise ValueError(f'original acquisition RNG write differs at counter {counter}')
                    history.observe(row['state'], row['phase'], row['context'])
                if type(value['random_state']) is not int or value['random_state'] != history.state:
                    raise ValueError(f'original acquisition RNG endpoint differs at counter {counter}')

            section(tape['loading'], 0)
            self.loading_state = history.state
            artwork = tape['loading_artwork']
            if (type(artwork['frames']) is not int or artwork['frames'] < 1
                    or type(artwork['selected_frame']) is not int
                    or not 0 <= artwork['selected_frame'] < artwork['frames']):
                raise ValueError('original loading artwork observation is invalid')
            if artwork['frames'] == 1:
                if artwork['draw'] is not None:
                    raise ValueError('single-frame loading artwork must consume no draw')
            elif (type(artwork['draw']) is not int
                  or not 1 <= artwork['draw'] <= len(tape['loading']['writes'])):
                raise ValueError('loading artwork draw is missing from observed loading history')
            updates = tape['updates']
            if not isinstance(updates, list) or not updates:
                raise ValueError('original acquisition requires observed pre-load bodies')
            scales, bios, states = [], [], [history.state]
            for update, value in enumerate(updates, 1):
                scale = value['frame_scale']
                if type(scale) is not int or not 0 <= scale < 2**32:
                    raise ValueError(f'original acquisition body {update} scale is invalid')
                observation = value['bios_observation']
                if (type(observation['update']) is not int or observation['update'] != update
                        or type(observation['consumed_frame_scale']) is not int
                        or observation['consumed_frame_scale'] != scale
                        or any(type(row['context']['movement']) is not int
                               or row['context']['movement'] != scale for row in value['writes'])):
                    raise ValueError(f'original acquisition body {update} timing phase differs')
                try:
                    interval = next_bios_input(observation)
                except ValueError as error:
                    raise ValueError(f'original acquisition body {update} BIOS: {error}') from error
                if type(interval) is not int or not 0 <= interval < 2**32:
                    raise ValueError(f'original acquisition body {update} BIOS input is invalid')
                section(value, update)
                scales.append(scale)
                bios.append(interval)
                states.append(history.state)
            book = tape['book']
            if (type(book['completed_updates']) is not int or book['completed_updates'] != len(updates)
                    or type(book['counter']) is not int or book['counter'] != len(updates) + 1
                    or type(book['random_state']) is not int or book['random_state'] != history.state):
                raise ValueError('original acquisition book boundary admits different ordinary history')
            section(tape['reload'], book['counter'])
            self.restored = dict(world=tape['reload']['world'], random_state=history.state)
            if type(self.restored['world']) is not int or not 0 <= self.restored['world'] < 2**32 - 1:
                raise ValueError('original acquisition restored world is invalid')
            self.inputs = dict(entry=deepcopy(entry), frame_scales=scales, bios_ticks=bios, random_states=states,
                               loading_artwork=deepcopy(artwork))
            self.tape = deepcopy(tape)
            self.evidence()
        except (KeyError, TypeError, AttributeError) as error:
            raise ValueError(f'original acquisition history is missing or malformed: {error}') from error

    def evidence(self):
        if file_digest(self.path) != self.tape_digest:
            raise ValueError('original acquisition tape changed before use')
        for path, name in ((self.executable, 'executable_sha256'), (self.save, 'save_sha256')):
            if file_digest(path) != self.original[name]:
                raise ValueError(f'original acquisition {name} differs from observation')
        if self.artifact.validate() != self.identity:
            raise ValueError('native artifact identity changed after original acquisition binding')
        return dict(version=1, tape_sha256=self.tape_digest, original=deepcopy(self.original),
                    native_artifact=deepcopy(self.identity), history=deepcopy(self.tape),
                    native_admission=deepcopy(self.native_admission))

    def replay_inputs(self):
        self.evidence()
        return dict(deepcopy(self.inputs), native_artifact=deepcopy(self.identity))

    def require_entry(self, state):
        self.evidence()
        for name, expected in self.restored.items():
            if type(state.get(name)) is not int or state[name] != expected:
                raise ValueError(f'original acquisition restored {name}: expected {expected}, '
                                 f'observed {state.get(name)}')

    def admit_native(self, session):
        """Require the existing verified launcher's real pre-load replay observations.

        Consume NativeFrameSession acquisition/execution evidence after its
        explicit first bodies, before using original combat controls. The
        verified launcher checks the live process here; copied evidence survives
        serial game runs. Full restored semantic admission remains separate.
        """
        self.native_admission = None
        self.evidence()
        self.artifact.check_session(session)
        try:
            acquisition = deepcopy(session.acquisition_evidence())
            execution = deepcopy(session.execution_evidence())
            updates = len(self.inputs['frame_scales'])
            for field, expected in self.inputs['entry'].items():
                observed = acquisition['entry'][field]
                if type(observed) is not type(expected) or observed != expected:
                    raise ValueError(f'native acquisition entry {field}: '
                                     f'expected {expected}, observed {observed}')
            if (type(acquisition['expected_initial_update']) is not int
                    or acquisition['expected_initial_update'] != 0
                    or type(acquisition['initial_update']) is not int
                    or acquisition['initial_update'] != 0):
                raise ValueError('native acquisition must begin at absolute update zero')
            for field in ('frame_scales', 'bios_ticks'):
                tape = execution[field]
                if (not isinstance(tape, list) or any(type(value) is not int for value in tape)
                        or tape != self.inputs[field]):
                    raise ValueError(f'native acquisition {field} differs from original history')
            observations, clocks = acquisition['observations'], execution['clocks']
            if len(observations) != updates + 1 or len(clocks) != updates + 1:
                raise ValueError('native acquisition history is incomplete or includes extra bodies')
            elapsed = 0
            for update, (observation, clock) in enumerate(zip(observations, clocks)):
                for field, expected in dict(ordinary_updates=update,
                        world=self.inputs['entry']['world'],
                        random_state=self.inputs['random_states'][update]).items():
                    if type(observation[field]) is not int or observation[field] != expected:
                        raise ValueError(f'native acquisition body {update} {field}: '
                                         f'expected {expected}, observed {observation[field]}')
                continuation = observation['clock']['continuation']
                if not isinstance(continuation, dict) or clock['continuation'] != continuation:
                    raise ValueError(f'native acquisition body {update} clock observations disagree')
                if update:
                    scale = self.inputs['frame_scales'][update - 1]
                    consumed = continuation['step']['frame_scale']
                    if type(consumed) is not int or consumed != scale:
                        raise ValueError(f'native acquisition body {update} consumed scale differs')
                    elapsed += scale
                if (type(clock['frame']) is not int or clock['frame'] != update
                        or type(clock['elapsed_scale']) is not int or clock['elapsed_scale'] != elapsed):
                    raise ValueError(f'native acquisition body {update} consumed-clock history differs')
            self.native_admission = dict(acquisition=acquisition, execution=execution)
        except (KeyError, TypeError, AttributeError) as error:
            raise ValueError(f'native acquisition evidence is missing or malformed: {error}') from error
        return deepcopy(self.native_admission)

    def require_native_admission(self):
        self.evidence()
        if self.native_admission is None:
            raise ValueError('original combat requires admitted native acquisition history')
