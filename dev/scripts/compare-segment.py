#!/usr/bin/env python3
"""Run an explicit segment input tape in the original first, then cyrus.

Each executable-specific backend provides open_session(segment, output_directory), a context
manager yielding snapshot(), advance_to(relative_frame), and input(key, action). snapshot()
returns frame plus world/player/actors/flags/prompts/timing/continuations, in the same units and coordinate space.
advance_to must leave the game at the exact requested frame; skipped boundaries reject the run.
Script continuations include normalized cursors and all six task-slot states; a
limited import retaining native baseline tasks cannot establish a matched start.
Original executable bindings belong in an ignored backend, never this script.
"""
import argparse
import copy
from contextlib import ExitStack, contextmanager
import hashlib
import re
from harness import exclusive_lock
from paired_outcomes import validate_checkpoint, evaluate_checkpoint
from original_acquisition import RandomWriteHistory, validate_bios_origin_tape
import importlib.util
import json
import subprocess
from pathlib import Path

OBSERVABLES = ('world', 'player', 'actors', 'flags', 'prompts', 'timing', 'continuations')
PROJECTED_OBSERVABLES = ('globals', 'inventory', 'random_state', 'native_source')


class NativePhaseError(RuntimeError):
    """A rejected stopped boundary with evidence available even during construction."""

    def __init__(self, message, evidence):
        super().__init__(message)
        self.evidence = copy.deepcopy(evidence)


class NativeArtifact:
    """Launch only the executable identified by a source-bound acquisition receipt.

    Local backends remain trusted adapters. In verified mode they must launch with
    this object and expose that live Popen as session.process before observation.
    """

    def __init__(self, executable, receipt, head, tree):
        self.executable = Path(executable).resolve()
        self.receipt_path, self.head, self.tree = Path(receipt), head, tree
        self.processes = []
        self.identity = None
        self._leased_executable = None

    @staticmethod
    def digest(path):
        with path.open('rb') as stream:
            return hashlib.file_digest(stream, 'sha256').hexdigest()

    def validate(self):
        receipt = json.loads(self.receipt_path.read_text())
        if not isinstance(receipt, dict):
            raise ValueError('native artifact receipt must be an object')
        for field in ('commit', 'checkout_commit', 'checkout_tree'):
            if not isinstance(receipt.get(field), str) or not re.fullmatch(r'[0-9a-f]{40}', receipt[field]):
                raise ValueError(f'native artifact receipt lacks {field}')
        for field in ('archive_sha256', 'executable_sha256'):
            if not isinstance(receipt.get(field), str) or not re.fullmatch(r'[0-9a-f]{64}', receipt[field]):
                raise ValueError(f'native artifact receipt lacks {field}')
        for field in ('run', 'artifact', 'checkout_job'):
            if type(receipt.get(field)) is not int or receipt[field] <= 0:
                raise ValueError(f'native artifact receipt lacks {field}')
        if receipt['commit'] != self.head:
            raise ValueError('native artifact workflow head differs from requested source')
        if receipt['checkout_tree'] != self.tree:
            raise ValueError('native artifact checkout tree differs from requested source')
        if self._leased_executable is None:
            executable_digest = self.digest(self.executable)
        else:
            self._leased_executable.seek(0)
            executable_digest = hashlib.file_digest(self._leased_executable, 'sha256').hexdigest()
        if executable_digest != receipt['executable_sha256']:
            raise ValueError('native executable differs from artifact receipt')
        if self.identity is not None and receipt != self.identity:
            raise ValueError('native artifact receipt changed during comparison')
        self.identity = receipt.copy()
        return dict(receipt, executable=str(self.executable))

    @contextmanager
    def lease(self):
        """Pin verified Linux executable bytes across sequential acceptance runs.

        The caller owns this lifetime separately from per-run locks. An open
        inode survives pathname removal; launches use that descriptor rather
        than silently rebuilding or switching sources. Receipt changes and
        byte changes still reject. Closing the lease releases only our handle.
        """
        if self._leased_executable is not None:
            raise ValueError('native executable lease already active')
        self.validate()
        with self.executable.open('rb') as executable:
            if not Path('/proc/self/fd').is_dir():
                raise ValueError('native executable lease requires Linux descriptor execution')
            self._leased_executable = executable
            try:
                self.validate()
                yield self
            finally:
                self._leased_executable = None

    def launch(self, arguments, **options):
        self.validate()
        if options.get('shell') or 'executable' in options:
            raise ValueError('native artifact launcher cannot override the executable')
        executable = str(self.executable)
        if self._leased_executable is not None:
            descriptor = self._leased_executable.fileno()
            executable = f'/proc/self/fd/{descriptor}'
            options['pass_fds'] = tuple(set(options.get('pass_fds', ())) | {descriptor})
        process = subprocess.Popen([executable, *arguments], **options)
        self.processes.append(process)
        return process

    def check_session(self, session):
        process = getattr(session, 'process', None)
        if not any(process is launched for launched in self.processes):
            raise ValueError('native backend must expose the process from the verified launcher')
        if process.poll() is not None:
            raise ValueError('verified native process exited before comparison')
        self.validate()


class NativeFrameSession:
    """Drive a staged native control connection while ReplayHold keeps it stopped.

    project(state, scripts) normalizes the measured native state. Input is latched
    while ReplayHold is active, before the next observation-only schedule. All actors
    must fit in actor_limit; silently truncating the comparison is an error.
    Recorded scales are checked against the observed consumed clock step, with
    full continuations retained in clock_observations. Input replies must match the
    requested key/action without advancing the stopped frame or clock; successful
    replies are retained in input_observations. Optional bios_ticks carries
    measured per-update BIOS intervals; omitting it uses diagnostic simulated BIOS
    timing and cannot establish complete original/native clock equivalence.
    expected_initial_update rejects uncontrolled bootstrap before relative-frame
    rebasing. host_nanos forwards measured presentation intervals, including zero.
    acquisition_evidence retains absolute update/RNG/clock boundaries.
    """

    def __init__(self, control, project, actor_limit=4096, frame_scales=None, bios_ticks=None,
                 expected_initial_update=None, host_nanos=None, cached_bios_samples=None, combat_pacing=None, clock_timelines=None):
        self.control, self.project, self.actor_limit = control, project, actor_limit
        self.frame_scales = None if frame_scales is None else tuple(frame_scales)
        if self.frame_scales is not None and any(type(scale) is not int or not 0 <= scale <= 2**32 - 1 for scale in self.frame_scales):
            raise ValueError('recorded clock tape requires nonnegative integer Q8 frame scales')
        self.bios_ticks = None if bios_ticks is None else tuple(bios_ticks)
        if self.bios_ticks is not None and (
                self.frame_scales is None or len(self.bios_ticks) != len(self.frame_scales)
                or any(type(tick) is not int or not 0 <= tick <= 2**32 - 1 for tick in self.bios_ticks)):
            raise ValueError('recorded BIOS intervals require one unsigned interval per clock scale')
        if expected_initial_update is not None and (
                type(expected_initial_update) is not int or expected_initial_update < 0):
            raise ValueError('expected initial native update must be a nonnegative integer')
        self.expected_initial_update = expected_initial_update
        self.host_nanos = None if host_nanos is None else tuple(host_nanos)
        if self.host_nanos is not None and (
                self.frame_scales is None or len(self.host_nanos) != len(self.frame_scales)
                or any(type(nanos) is not int or not 0 <= nanos <= 2**64 - 1 for nanos in self.host_nanos)):
            raise ValueError('recorded host time requires one unsigned nanosecond interval per clock scale')
        self.cached_bios_samples = None if cached_bios_samples is None else tuple(cached_bios_samples)
        if self.cached_bios_samples is not None and (
                self.frame_scales is None or not self.frame_scales
                or len(self.cached_bios_samples) != len(self.frame_scales) + 1
                or any(type(sample) is not int or not 0 <= sample < 2**32
                       for sample in self.cached_bios_samples)):
            raise ValueError('cached BIOS phase requires an unsigned entry sample and one per body')
        self.combat_pacing = None if combat_pacing is None else copy.deepcopy(combat_pacing)
        if self.combat_pacing is not None:
            if self.cached_bios_samples is None or not isinstance(self.combat_pacing, list) or len(self.combat_pacing) != len(self.cached_bios_samples):
                raise ValueError('combat pacing requires one boundary list per absolute BIOS sample')
            for boundary in self.combat_pacing:
                if not isinstance(boundary, list):
                    raise ValueError('combat pacing boundary must be a list')
                names = set()
                for expected in boundary:
                    if not isinstance(expected, dict) or set(expected) != {'actor', 'allocation_index', 'selection', 'cached_bios_sample'}:
                        raise ValueError('combat pacing expectation requires actor, allocation_index, cached_bios_sample and selection')
                    name = expected['actor']
                    if not isinstance(name, str) or not name or name in names:
                        raise ValueError('combat pacing actor must be unique and named')
                    names.add(name)
                    if type(expected['cached_bios_sample']) is not int or not 0 <= expected['cached_bios_sample'] < 2**32:
                        raise ValueError('combat pacing requires an independently observed unsigned cached sample')
                    if type(expected['allocation_index']) is not int or not -2**31 <= expected['allocation_index'] < 2**31 or type(expected['selection']) is not bool:
                        raise ValueError('combat pacing requires signed allocation index and boolean selection phase')
        self.clock_timelines = copy.deepcopy(clock_timelines)
        if self.clock_timelines is not None:
            if (not isinstance(self.clock_timelines, list) or self.bios_ticks is None
                    or self.cached_bios_samples is None or len(self.clock_timelines) != len(self.bios_ticks)):
                raise ValueError('clock timelines require one measured timeline per effective interval and absolute boundary')
            for timeline_index, timeline in enumerate(self.clock_timelines):
                if not isinstance(timeline, dict) or set(timeline) != {'stored_sample', 'world', 'decisions', 'before_decisions', 'after_decisions'}:
                    raise ValueError('clock timeline requires stored_sample, world, decisions and ordered refresh lists')
                if type(timeline['world']) is not int or timeline['world'] < 0:
                    raise ValueError('clock timeline requires a nonnegative world')
                for samples in ([timeline['stored_sample']], timeline['before_decisions'], timeline['after_decisions']):
                    if not isinstance(samples, list) or any(type(v) is not int or not 0 <= v < 2**32 for v in samples):
                        raise ValueError('clock timeline requires unsigned measured samples')
                if not isinstance(timeline['decisions'], list) or not timeline['decisions']:
                    raise ValueError('clock timeline requires independently observed ordered decisions')
                final_sample = (timeline['after_decisions'] or timeline['before_decisions'] or [timeline['stored_sample']])[-1]
                if final_sample != self.cached_bios_samples[timeline_index + 1]:
                    raise ValueError('clock timeline final refresh differs from measured body boundary')
                occurrences = {}
                for decision in timeline['decisions']:
                    if (not isinstance(decision, dict) or set(decision) != {'index', 'name', 'occurrence'}
                            or type(decision['index']) is not int or decision['index'] < 0
                            or not isinstance(decision['name'], str) or not decision['name']
                            or type(decision['occurrence']) is not int):
                        raise ValueError('clock timeline decision identity is incomplete')
                    identity = (decision['index'], decision['name'])
                    occurrences[identity] = occurrences.get(identity, 0) + 1
                    if decision['occurrence'] != occurrences[identity]:
                        raise ValueError('clock timeline decision occurrence is missing or reordered')
        self.timeline_observations = []
        self.combat_observations = []
        self.current = self._read()
        self.origin = self.current[0]['scripts']['ordinary_updates']
        self.timeline_generation = self.current[1].get('generation')
        if self.clock_timelines is not None and (type(self.timeline_generation) is not int or self.timeline_generation < 0):
            raise ValueError('clock timeline requires actual native script generation')
        if expected_initial_update is not None and self.origin != expected_initial_update:
            raise ValueError(f'native acquisition expected initial update {expected_initial_update}, '
                             f'observed {self.origin}; bootstrap timing was not controlled')
        self._check_bios_phase(0)
        self.acquisition_observations = [self._acquisition_observation()]
        self.clock_observations = []
        self.input_observations = []
        if self.frame_scales is not None:
            self.clock_observations.append(self._clock_observation(0))

    def _read(self):
        state = self.control.request({'type': 'state'})
        scripts = self.control.request({'type': 'scripts', 'limit': self.actor_limit})
        after = self.control.request({'type': 'state'})
        if state['scripts']['ordinary_updates'] != after['scripts']['ordinary_updates']:
            raise RuntimeError('native simulation advanced outside an explicit frame step')
        if state.get('clock') != after.get('clock'):
            raise RuntimeError('native clock changed outside an explicit frame step')
        if state.get('entry') != after.get('entry'):
            raise RuntimeError('native entry provenance changed during observation-only read')
        if after['scripts']['failed']:
            raise RuntimeError(f'native script failures: {after["scripts"]}')
        if len(scripts['actors']) != scripts['total']:
            raise RuntimeError('native comparison actor read was truncated')
        return after, scripts

    def _phase_error(self, message, frame, expected):
        state, scripts = self.current
        return NativePhaseError(message, dict(
            frame=frame, ordinary_update=state['scripts']['ordinary_updates'],
            expected_bios_sample=expected, clock=copy.deepcopy(state.get('clock')),
            combat_pacing=[dict(actor=actor.get('name'), observation=copy.deepcopy(actor['combat_pacing']))
                           for actor in scripts['actors'] if actor.get('combat_pacing') is not None]))

    def _check_bios_phase(self, frame):
        if self.cached_bios_samples is None:
            return
        if not 0 <= frame < len(self.cached_bios_samples):
            raise self._phase_error(f'native cached BIOS phase has no boundary for frame {frame}', frame, None)
        actual = self.current[0].get('clock', {}).get('bios_sample')
        expected = self.cached_bios_samples[frame]
        if type(actual) is not int or not 0 <= actual < 2**32:
            raise self._phase_error(f'native cached BIOS sample unavailable at frame {frame}', frame, expected)
        if actual != expected:
            raise self._phase_error(f'native cached BIOS phase at frame {frame}: observed {actual}, expected {expected}', frame, expected)
        try:
            self._check_combat_phase(frame)
        except RuntimeError as error:
            raise self._phase_error(str(error), frame, expected) from error

    def _check_combat_phase(self, frame):
        if self.combat_pacing is None:
            return
        state, scripts = self.current
        for expected in self.combat_pacing[frame]:
            name = expected['actor']
            actors = [actor for actor in scripts['actors'] if actor.get('name') == name]
            if len(actors) != 1:
                raise RuntimeError(f'native combat pacing frame {frame} actor {name}: absent or ambiguous')
            observation = actors[0].get('combat_pacing')
            context = f'native combat pacing frame {frame} actor {name}'
            row = dict(frame=frame, actor=name, observation=copy.deepcopy(observation))
            if row not in self.combat_observations:
                self.combat_observations.append(row)
            if not isinstance(observation, dict):
                raise RuntimeError(f'{context}: observation missing')
            sample = expected['cached_bios_sample']
            required = dict(ordinary_update=state['scripts']['ordinary_updates'],
                            cached_bios_sample=sample, allocation_index=expected['allocation_index'])
            for field, value in required.items():
                if type(observation.get(field)) is not int or observation[field] != value:
                    raise RuntimeError(f'{context}: stale or misaligned {field}')
            selection = observation.get('selection')
            if 'selection' not in observation or (selection is not None and not isinstance(selection, dict)) or (selection is not None) != expected['selection']:
                raise RuntimeError(f'{context}: selection phase differs or missing')
            # Signed wrapping sum and checked remainder follow preservation a95064e.
            total = (sample + expected['allocation_index'] + 2**31) % 2**32 - 2**31
            if type(observation.get('pacing_sum')) is not int or observation['pacing_sum'] != total:
                raise RuntimeError(f'{context}: signed pacing sum differs')
            divisors = [('defence_counter', observation.get('defence_counter'), 'counter_remainder')]
            if selection is not None:
                if type(selection.get('pacing_sum')) is not int or selection['pacing_sum'] != total:
                    raise RuntimeError(f'{context}: selection pacing sum differs')
                divisors.append(('defence_pacing', selection.get('defence_pacing'), 'pacing_remainder'))
            for field, divisor, remainder_field in divisors:
                if type(divisor) is not int or not -2**31 <= divisor < 2**31:
                    raise RuntimeError(f'{context}: {field} unavailable')
                remainder = None
                if divisor != 0 and not (total == -2**31 and divisor == -1):
                    remainder = abs(total) % abs(divisor)
                    if total < 0:
                        remainder = -remainder
                actual = observation.get(remainder_field)
                if remainder_field not in observation or (remainder is None and actual is not None) or (remainder is not None and (type(actual) is not int or actual != remainder)):
                    raise RuntimeError(f'{context}: checked {remainder_field} differs')

    def _clock_observation(self, elapsed_scale):
        continuation = self.current[0].get('clock', {}).get('continuation')
        if not isinstance(continuation, dict):
            raise RuntimeError('recorded native clock continuation unavailable')
        return dict(frame=self.current[0]['scripts']['ordinary_updates'] - self.origin,
                    elapsed_scale=elapsed_scale, continuation=copy.deepcopy(continuation),
                    cached_bios_sample=self.current[0].get('clock', {}).get('bios_sample'))

    def snapshot(self):
        state, scripts = self.current
        frame = state['scripts']['ordinary_updates'] - self.origin
        self._check_bios_phase(frame)
        return dict(self.project(state, scripts), frame=frame)

    def _acquisition_observation(self):
        state = self.current[0]
        return copy.deepcopy({
            'ordinary_updates': state['scripts']['ordinary_updates'],
            'world': state['scripts'].get('world'),
            'random_state': state.get('random_state'),
            'clock': state.get('clock'), 'startup': state.get('startup'),
            'entry': state.get('entry'),
        })

    def acquisition_evidence(self):
        """Retain absolute native boundaries; relative frame zero does not erase bootstrap work."""
        return copy.deepcopy({
            'expected_initial_update': self.expected_initial_update,
            'initial_update': self.origin, 'observations': self.acquisition_observations,
            'entry': self.acquisition_observations[0]['entry'],
        })

    def execution_evidence(self):
        """Copy requested tapes and measured boundaries, including an interrupted run."""
        return copy.deepcopy(dict(
            frame_scales=None if self.frame_scales is None else list(self.frame_scales),
            bios_ticks=None if self.bios_ticks is None else list(self.bios_ticks),
            cached_bios_samples=None if self.cached_bios_samples is None else list(self.cached_bios_samples),
            host_nanos=None if self.host_nanos is None else list(self.host_nanos),
            clock_timelines=copy.deepcopy(self.clock_timelines),
            timeline_observations=self.timeline_observations,
            combat_pacing=copy.deepcopy(self.combat_pacing),
            combat_observations=copy.deepcopy(self.combat_observations),
            clocks=self.clock_observations, inputs=self.input_observations))

    def input(self, key, action):
        if action not in ('press', 'release'):
            raise ValueError(f'unsupported native key action {action}')
        before = self.snapshot()['frame']
        clock = copy.deepcopy(self.current[0].get('clock'))
        reply = self.control.request(dict(type='input', key=key, action=action))
        if not isinstance(reply, dict) or reply.get('key') != key or reply.get('action') != action:
            raise RuntimeError(f'native input acknowledgement differs from {key}/{action}: {reply!r}')
        self.current = self._read()
        if self.snapshot()['frame'] != before:
            raise RuntimeError('native frame changed during key acknowledgement')
        if self.current[0].get('clock') != clock:
            raise RuntimeError('native clock changed during key acknowledgement')
        self.input_observations.append(dict(frame=before, key=key, action=action,
                                            acknowledgement=copy.deepcopy(reply)))
        return self.snapshot()['frame']

    def advance_to(self, frame):
        current = self.snapshot()['frame']
        if type(frame) is not int or frame < current:
            raise ValueError(f'native frame {frame} already passed or invalid')
        if self.frame_scales is not None and frame > len(self.frame_scales):
            raise ValueError('recorded clock tape exhausted')
        while current < frame:
            command = ({'type': 'replay_frame', 'frame_scale': self.frame_scales[current]}
                       if self.frame_scales is not None else
                       {'type': 'schedule_input', 'events': [{'after_updates': 0}]})
            if self.bios_ticks is not None:
                command['elapsed_bios_ticks'] = self.bios_ticks[current]
            if self.host_nanos is not None:
                command['elapsed_host_nanos'] = self.host_nanos[current]
            if self.clock_timelines is not None:
                self._check_bios_phase(current)
                if self.current[1].get('generation') != self.timeline_generation:
                    raise RuntimeError('native clock timeline generation changed')
                command['clock_timeline'] = dict(copy.deepcopy(self.clock_timelines[current]),
                    generation=self.timeline_generation,
                    expected_update=self.current[0]['scripts']['ordinary_updates'],
                    expected_bios_sample=self.current[0]['clock']['bios_sample'])
                evidence = dict(frame=current, command=copy.deepcopy(command))
                self.timeline_observations.append(evidence)
                evidence['acknowledgement'] = copy.deepcopy(self.control.request(command))
            else:
                self.control.request(command)
            self.current = self._read()
            if self.clock_timelines is not None:
                evidence['observed'] = dict(generation=self.current[1].get('generation'),
                    ordinary_update=self.current[0]['scripts']['ordinary_updates'],
                    clock=copy.deepcopy(self.current[0].get('clock')))
                if self.current[1].get('generation') != self.timeline_generation:
                    raise RuntimeError('native clock timeline generation changed during frame')
                acknowledgement = evidence['acknowledgement']
                boundaries = acknowledgement.get('boundaries') if isinstance(acknowledgement, dict) else None
                if not isinstance(boundaries, list) or len(boundaries) != 1 or not isinstance(boundaries[0], dict):
                    raise RuntimeError('native timeline acknowledgement requires one actual input boundary')
                boundary = boundaries[0]
                required = dict(generation=self.timeline_generation,
                    update=command['clock_timeline']['expected_update'] + 1, after_updates=0)
                for field, value in required.items():
                    if type(boundary.get(field)) is not int or boundary[field] != value:
                        raise RuntimeError(f'native timeline acknowledgement has stale or misaligned {field}')
                if boundary.get('key') is not None or boundary.get('action') is not None:
                    raise RuntimeError('native timeline acknowledgement contains unexpected input')
            self.acquisition_observations.append(self._acquisition_observation())
            observed = self.snapshot()['frame']
            if observed != current + 1:
                raise RuntimeError(f'native frame gap: expected {current + 1}, observed {observed}')
            if self.frame_scales is not None:
                observation = self._clock_observation(self.clock_observations[-1]['elapsed_scale'])
                consumed = observation['continuation'].get('step', {}).get('frame_scale')
                if type(consumed) is not int or not 0 <= consumed <= 2**32 - 1:
                    raise RuntimeError('native consumed clock scale unavailable')
                observation['elapsed_scale'] += consumed
                observation['requested_scale'] = self.frame_scales[current]
                self.clock_observations.append(observation)
                if consumed != self.frame_scales[current]:
                    raise RuntimeError(f'native consumed clock scale {consumed}, requested {self.frame_scales[current]}')
            self._check_bios_phase(observed)
            current = observed


def input_tape(segment):
    """Compile input-only replay steps; never silently discard steering or dialogue decisions."""
    events, checkpoints, assertions, cursor = [], [0], [], 0
    for index, step in enumerate(segment['steps']):
        if set(step) in ({'input'}, {'input', 'updates'}):
            batch = step['input']
            if not batch:
                raise ValueError(f'step {index}: empty input batch')
            for event in batch:
                offset = event.get('after_updates')
                if type(offset) is not int or offset < 0:
                    raise ValueError(f'step {index}: invalid frame offset')
                if 'key' in event or 'action' in event:
                    if not isinstance(event.get('key'), str) or event.get('action') not in ('press', 'release'):
                        raise ValueError(f'step {index}: input needs key and press/release action')
                    events.append(dict(frame=cursor + offset, key=event['key'], action=event['action']))
            final_offset = max(event['after_updates'] for event in batch)
            duration = step.get('updates', final_offset + 1)
            if type(duration) is not int or duration < 0 or final_offset > duration:
                raise ValueError(f'step {index}: invalid updates duration or input beyond final boundary')
            cursor += duration
            checkpoints.append(cursor)
        elif set(step) == {'checkpoint'}:
            expected = step['checkpoint']
            try:
                validate_checkpoint(expected)
            except ValueError as error:
                raise ValueError(f'step {index}: {error}') from error
            if expected:
                assertions.append(dict(frame=cursor, step=index, expected=copy.deepcopy(expected)))
            checkpoints.append(cursor)
        else:
            raise ValueError(f'step {index}: supply expanded input steps; unsupported {list(step)}')
    if not events:
        raise ValueError('segment has no key input')
    return sorted(events, key=lambda event: event['frame']), sorted(set(checkpoints)), assertions


def delta(original, native, path=''):
    """Keep both measured values; report missing fields and numeric signed deltas explicitly."""
    if isinstance(original, dict) and isinstance(native, dict):
        rows = []
        for key in sorted(original.keys() | native.keys()):
            name = f'{path}.{key}' if path else key
            if key not in original or key not in native:
                rows.append(dict(field=name, status='unavailable', original=original.get(key), native=native.get(key)))
            else:
                rows.extend(delta(original[key], native[key], name))
        return rows
    if original is None or native is None:
        return [dict(field=path, status='unavailable', original=original, native=native)]
    if isinstance(original, list) and isinstance(native, list) and len(original) == len(native):
        return [row for index, (a, b) in enumerate(zip(original, native))
                for row in delta(a, b, f'{path}[{index}]')]
    row = dict(field=path, status='match' if type(original) is type(native) and original == native else 'different',
               original=original, native=native)
    if type(original) in (int, float) and type(native) in (int, float):
        row['delta'] = native - original
    return [row]


def snapshot(session, frame):
    return validate_snapshot(copy.deepcopy(session.snapshot()), frame)


def validate_snapshot(state, frame):
    if not isinstance(state, dict):
        raise ValueError('snapshot must be an object')
    if type(state.get('frame')) is not int or state['frame'] != frame:
        raise ValueError(f'frame boundary missed: requested {frame}, observed {state.get("frame")}')
    for field in OBSERVABLES:
        if field not in state:
            raise ValueError(f'frame {frame}: missing observable {field}')
    return state


def record(session, events, checkpoints, initial, input_attempts):
    """Yield each measured checkpoint before sending inputs at that boundary.

    The caller retains completed observations immediately, including when a later
    boundary fails, and can stop native execution at the first actual mismatch.
    """
    yield initial
    boundaries = sorted(set(checkpoints) | {event['frame'] for event in events})
    for frame in boundaries:
        if frame:
            session.advance_to(frame)
            state = snapshot(session, frame)
            if frame in checkpoints:
                yield state
        for event in events:
            if event['frame'] == frame:
                try:
                    applied = session.input(event['key'], event['action'])
                except (ValueError, RuntimeError, TimeoutError, OSError) as error:
                    input_attempts.append(dict(event, status='failed', reason=str(error)))
                    raise
                accepted = type(applied) is int and applied == frame
                input_attempts.append(dict(event, acknowledged_frame=copy.deepcopy(applied),
                                           status='accepted' if accepted else 'rejected'))
                if not accepted:
                    raise ValueError(f'input frame missed: requested {frame}, acknowledged {applied}')


def entry_snapshot(session, report, phase, side):
    """Retain the observation before validation, including malformed entries."""
    try:
        acquisition = getattr(session, 'acquisition_evidence', None)
        if callable(acquisition):
            evidence = report.setdefault('acquisition', {}).setdefault(phase, {})
            try:
                evidence[side] = copy.deepcopy(acquisition())
            except (ValueError, RuntimeError, TimeoutError, OSError) as error:
                evidence[side] = dict(error=str(error))
                raise ValueError(f'acquisition evidence: {error}') from error
            if not isinstance(evidence[side], dict):
                raise ValueError('acquisition evidence must be an object')
        observed = copy.deepcopy(session.snapshot())
        report.setdefault(phase, {})[side] = observed
        return validate_snapshot(observed, 0)
    except (ValueError, RuntimeError, TimeoutError, OSError) as error:
        raise ValueError(f'{side} {phase}: {error}') from error


def semantic_delta(expected, measured):
    keys = (*OBSERVABLES, *PROJECTED_OBSERVABLES)
    return delta({key: expected[key] for key in keys if key in expected},
                 {key: measured[key] for key in keys if key in measured})


def reject_admission(report, phase, side, fields, reason):
    """Retain every failed field alongside the acquisition evidence for this phase.

    Delta rows use original/native for the two sides at preflight, and for
    admitted/measured values when a single backend is reopened.
    """
    failed = [row for row in fields if row['status'] != 'match']
    report.setdefault('admission_rejections', []).append(dict(phase=phase, side=side, fields=failed))
    first = failed[0]
    report['reason'] = (f"{reason}: {first['field']} {first['status']} "
                        f"(expected {first['original']!r}, measured {first['native']!r})")


def admit_replay_entry(session, report, side):
    measured = entry_snapshot(session, report, 'replay_entry', side)
    fields = semantic_delta(report['entry'][side], measured)
    report.setdefault('replay_entry_deltas', {})[side] = fields
    if any(row['status'] != 'match' for row in fields):
        reject_admission(report, 'replay_entry', side, fields,
                         f'{side} replay entry differs from admitted entry')
        raise ValueError(report['reason'])
    return measured


def check_outcomes(report, assertions, side, measured):
    """Check this side before the recorder resumes and sends later inputs."""
    for assertion in assertions:
        if assertion['frame'] != measured['frame']:
            continue
        fields = evaluate_checkpoint(assertion['expected'], measured, report['replay_entry'][side])
        report['outcomes'].append(dict(side=side, frame=measured['frame'],
                                       step=assertion['step'], fields=fields))
        failed = [row for row in fields if row['status'] != 'match']
        if failed:
            report['status'] = ('blocked' if any(row['status'] == 'unavailable' for row in failed)
                                else 'different')
            report['reason'] = (f"{side} frame {measured['frame']} step {assertion['step']}: "
                                f"checkpoint {failed[0]['field']} {failed[0]['status']}")
            return False
    return True



class RestorationPhases:
    """Require observed restoration and first-body RNG phases before combat input.

    The local witness is an ordered write/boundary stream from the same admitted
    original acquisition. Never derive expected states or draw counts from native
    endpoints. A separate no-input first-body run leaves the combat tape intact.
    """

    def __init__(self, witness):
        try:
            self.history = RandomWriteHistory(witness['initial_state'])
            for event in witness['events']:
                if event['type'] == 'write':
                    row = self.history.observe(event['state'], event['phase'], event['context'])
                    if any(type(event[key]) is not int or event[key] != row[key]
                           for key in ('draw', 'previous')):
                        raise ValueError('RNG write count or predecessor differs from observed history')
                elif event['type'] == 'boundary':
                    row = self.history.boundary(event['name'], event['state'], event['context'])
                    if type(event['draw']) is not int or event['draw'] != row['draw']:
                        raise ValueError('RNG boundary count differs from observed history')
                else:
                    raise ValueError('RNG history contains an unsupported event')
            names = list(self.history.boundaries)
            if ('restored-publication' not in names or 'first-completed-body' not in names
                    or names.index('restored-publication') >= names.index('first-completed-body')):
                raise ValueError('RNG history requires ordered restoration and first-body boundaries')
        except (KeyError, TypeError, AttributeError) as error:
            raise ValueError(f'RNG restoration history is incomplete: {error}') from error

    def check(self, session, report, side, phase):
        measured = copy.deepcopy(session.snapshot())
        row = dict(measured=measured, status='rejected')
        report.setdefault('restoration_phases', {}).setdefault(side, {})[phase] = row
        try:
            frame = 0 if phase == 'restored-publication' else 1
            if type(measured.get('frame')) is not int or measured['frame'] != frame:
                raise ValueError(f'expected acknowledged frame {frame}, observed {measured.get("frame")}')
            row['expected'] = self.history.require_boundary(phase, measured.get('random_state'))
            row['status'] = 'match'
        except ValueError as error:
            row['reason'] = str(error)
            raise ValueError(f'{side} {phase}: {error}') from error


def run_pair(segment, original, native, output, native_artifact=None, restoration_history=None):
    report = dict(version=1, status='blocked', events=[], checkpoints=[], outcomes=[],
                  input_attempts=dict(original=[], native=[]))
    resources = ExitStack()
    try:
        phases = None if restoration_history is None else RestorationPhases(restoration_history)
        if phases is not None:
            report['restoration_history'] = copy.deepcopy(restoration_history)
        if native_artifact is not None:
            resources.enter_context(native_artifact.lease())
            report['native_artifact'] = native_artifact.validate()
        events, checkpoints, assertions = input_tape(segment)
        report['events'] = events
        (output / 'original').mkdir(parents=True, exist_ok=True)
        (output / 'native').mkdir(parents=True, exist_ok=True)
        options = {} if native_artifact is None else dict(executable=native_artifact)
        for side, factory in (('original', original), ('native', native)):
            directory = output / 'entry' / side
            directory.mkdir(parents=True, exist_ok=True)
            with factory(segment, directory, **(options if side == 'native' else {})) as session:
                try:
                    if side == 'native' and native_artifact is not None:
                        native_artifact.check_session(session)
                    entry_snapshot(session, report, 'entry', side)
                    if phases is not None:
                        phases.check(session, report, side, 'restored-publication')
                finally:
                    evidence = getattr(session, 'execution_evidence', None)
                    if side == 'native' and callable(evidence):
                        report['entry_native_execution'] = copy.deepcopy(evidence())
        fields = semantic_delta(report['entry']['original'], report['entry']['native'])
        report['start_deltas'] = fields
        if any(row['status'] != 'match' for row in fields):
            reject_admission(report, 'entry', 'original/native', fields,
                             'start states differ or are incomplete')
            return report
        if phases is not None:
            for side, factory in (('original', original), ('native', native)):
                directory = output / 'first-body' / side
                directory.mkdir(parents=True, exist_ok=True)
                with factory(segment, directory, **(options if side == 'native' else {})) as session:
                    try:
                        if side == 'native' and native_artifact is not None:
                            native_artifact.check_session(session)
                        admit_replay_entry(session, report, side)
                        phases.check(session, report, side, 'restored-publication')
                        session.advance_to(1)
                        phases.check(session, report, side, 'first-completed-body')
                    finally:
                        evidence = getattr(session, 'execution_evidence', None)
                        if callable(evidence):
                            report.setdefault('restoration_execution', {})[side] = copy.deepcopy(evidence())
        report['original'] = []
        with original(segment, output / 'original') as session:
            initial = admit_replay_entry(session, report, 'original')
            for observation in record(session, events, checkpoints, initial, report['input_attempts']['original']):
                report['original'].append(observation)
                if not check_outcomes(report, assertions, 'original', observation):
                    return report
        report['native'] = []
        with native(segment, output / 'native', **options) as session:
            try:
                if native_artifact is not None:
                    native_artifact.check_session(session)
                initial = admit_replay_entry(session, report, 'native')
                for expected, measured in zip(report['original'], record(session, events, checkpoints, initial,
                                                                        report['input_attempts']['native'])):
                    report['native'].append(measured)
                    if not check_outcomes(report, assertions, 'native', measured):
                        return report
                    fields = semantic_delta(expected, measured)
                    mismatch = any(row['status'] != 'match' for row in fields)
                    if mismatch and measured['frame'] == 0:
                        report.update(reason='start states differ or are incomplete', start_deltas=fields)
                        return report
                    report['checkpoints'].append(dict(frame=measured['frame'], fields=fields))
                    if mismatch:
                        report['status'] = 'different'
                        return report
            finally:
                evidence = getattr(session, 'execution_evidence', None)
                if callable(evidence):
                    report['native_execution'] = copy.deepcopy(evidence())
        report['status'] = 'match'
    except (ValueError, RuntimeError, TimeoutError, OSError) as error:
        report['reason'] = str(error)
        if isinstance(error, NativePhaseError):
            report['phase_rejection'] = copy.deepcopy(error.evidence)
    finally:
        resources.close()
    return report


def backend(path):
    spec = importlib.util.spec_from_file_location('comparison_backend', path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module.open_session


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('segment')
    parser.add_argument('--original-backend', required=True)
    parser.add_argument('--native-backend', required=True)
    parser.add_argument('--native-executable', type=Path)
    parser.add_argument('--native-receipt', type=Path)
    parser.add_argument('--native-head', help='expected full workflow head SHA')
    parser.add_argument('--native-source-tree', help='expected actual checkout tree SHA')
    parser.add_argument('--restoration-history', type=Path,
                        help='observed original RNG write/boundary stream for no-input phase gates')
    parser.add_argument('--out', required=True, help='host directory under magic/')
    args = parser.parse_args()
    binding = [args.native_executable, args.native_receipt, args.native_head, args.native_source_tree]
    if any(binding) and not all(binding):
        parser.error('verified native replay requires executable, receipt, head and source tree together')
    artifact = NativeArtifact(*binding) if all(binding) else None
    if artifact is not None:
        try:
            artifact.validate()
        except (ValueError, OSError) as error:
            parser.error(str(error))
    output = Path(args.out)
    output.mkdir(parents=True, exist_ok=True)
    with exclusive_lock(output / 'comparison.lock'):
        subprocess.run([str(Path(__file__).with_name("ensure-retro-runtime.sh"))],
                       check=True, stdout=subprocess.DEVNULL)
        report = run_pair(json.loads(Path(args.segment).read_text()), backend(args.original_backend),
                          backend(args.native_backend), output, native_artifact=artifact,
                          restoration_history=(json.loads(args.restoration_history.read_text())
                                               if args.restoration_history else None))
        report['segment'] = str(args.segment)
        (output / 'comparison.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(dict(status=report['status'], report=str(output / 'comparison.json'))))
    return 0 if report['status'] == 'match' else 1


if __name__ == '__main__':
    raise SystemExit(main())
