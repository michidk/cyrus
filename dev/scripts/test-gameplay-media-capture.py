#!/usr/bin/env python3
"""Admission controls for non-stalling gameplay media windows."""
import importlib.util
from pathlib import Path
import tempfile
import os
import subprocess
import sys
import json
import unittest

spec = importlib.util.spec_from_file_location('media', Path(__file__).with_name('gameplay-media-capture.py'))
media = importlib.util.module_from_spec(spec)
spec.loader.exec_module(media)


def observation(clock=1, frame=12, execution='running'):
    return dict(ready=True, scene='CAVERNS', game_seconds=clock, frame=frame,
                execution=execution, player={'position_q8': [1, 2, 3]},
                elapsed_scale=int(clock * 3072) if isinstance(clock, (int, float)) and __import__('math').isfinite(clock) else 0,
                normal_frame_rate=12)


class Windows(unittest.TestCase):
    def test_missing_or_inconsistent_integer_scale_is_rejected_at_acquisition(self):
        for row in (dict(observation(), elapsed_scale=None),
                    dict(observation(), elapsed_scale=True),
                    dict(observation(), elapsed_scale=3073),
                    dict(observation(), elapsed_scale_end=3071),
                    dict(observation(), game_seconds=True)):
            with self.assertRaises(ValueError):
                media.require_observation(row)

    def test_read_span_and_event_relative_bounds_survive_window_serialization(self):
        before = dict(observation(1, 12), elapsed_scale_end=3200)
        after = dict(observation(1.25, 14), elapsed_scale_end=4000)
        row = media.window(before, after, .2, .4, 'frame.png')
        origin = dict(observation(.5, 8), event='placement_applied', phase='before_ordinary_update')
        anchored = media.anchor_window(row, origin)
        self.assertEqual(anchored['clock_scale'], [3072, 4000])
        self.assertEqual(anchored['event_relative_scale'], [1536, 2464])
        self.assertEqual(anchored['normal_frame_rate'], 12)
        self.assertFalse(anchored['atomic'])

    def test_owned_frame_rejects_legacy_poll_and_wrong_artifact(self):
        path = Path('frame.png')
        boundary = dict(observation(), generation=1, event='render_frame',
                        phase='render_extraction', render_frame=1, pcm_delivery_frames=5)
        result = dict(captured=True, path=str(path), boundary=boundary)
        frozen = media.owned_frame(result, path, 500)
        self.assertEqual(frozen['elapsed_scale'], 3072)
        self.assertEqual(frozen['pcm_delivery_frames'], 5)
        for bad in (dict(path=str(path)), dict(result, path='other.png'),
                    dict(result, boundary=dict(boundary, phase='polled')),
                    dict(result, boundary=dict(boundary, pcm_delivery_frames=501))):
            with self.assertRaises(ValueError):
                media.owned_frame(bad, path, 500)

    def test_surface_owned_sequence_is_admitted_and_duplicate_delivery_rejected(self):
        path = Path('frame.png')
        boundary = dict(observation(), generation=1, event='render_frame',
                        phase='sdl_surface_present', render_frame=7, pcm_delivery_frames=5)
        media.owned_frame(dict(captured=True, path=str(path), boundary=boundary), path, 5)
        first = media.window(boundary, boundary, 0, 1, str(path))
        first['clock_owner'] = 'sdl_surface_present'
        second = dict(first, wall_seconds=[1, 2])
        with self.assertRaisesRegex(ValueError, 'sequence repeated'):
            media.summarize([first, second], 0)

    def test_published_pending_step_is_excluded_from_placement_origin(self):
        boundary = dict(observation(1, 12), event='placement_applied',
                        phase='before_ordinary_update', clock_accounting='includes_pending_update',
                        pending_frame_scale=219, pending_accumulator_scale=219, startup_remaining=0)
        ack = dict(acknowledged=True, boundary=boundary)
        after_first_movement = observation(1, 12)
        origin = media.trigger_origin(ack, after_first_movement)
        self.assertEqual(origin['elapsed_scale'], 3072 - 219)
        self.assertEqual(origin['raw_elapsed_scale'], 3072)
        self.assertEqual((after_first_movement['elapsed_scale'] - origin['elapsed_scale']), 219)
        self.assertEqual(ack['boundary']['elapsed_scale'], 3072, 'retain producer evidence')
        # Native completed-update origins already exclude the forthcoming step.
        native = dict(boundary, elapsed_scale=2853, game_seconds=2853 / 3072,
                      clock_accounting='completed_updates')
        self.assertEqual(media.trigger_origin(dict(acknowledged=True, boundary=native),
                                             after_first_movement)['elapsed_scale'], 2853)
        for bad in (dict(boundary, pending_accumulator_scale=512),
                    dict(boundary, pending_frame_scale=True),
                    dict(boundary, startup_remaining=1),
                    dict(boundary, clock_accounting='unknown')):
            with self.assertRaises(ValueError):
                media.trigger_origin(dict(acknowledged=True, boundary=bad), after_first_movement)

    def test_delayed_poll_cannot_shift_acknowledged_trigger_origin(self):
        boundary = dict(observation(1, 12), elapsed_scale=3072, normal_frame_rate=12,
                        event='placement_applied', phase='before_ordinary_update')
        ack = dict(acknowledged=True, boundary=boundary)
        self.assertEqual(media.trigger_origin(ack, observation(2, 18)), boundary)
        self.assertEqual(media.elapsed_from_origin(boundary, observation(2.5, 20)), 1.5)

    def test_boolean_ack_or_uncertain_event_clock_is_rejected(self):
        boundary = dict(observation(1, 12), elapsed_scale=3072, normal_frame_rate=12,
                        event='placement_applied', phase='before_ordinary_update')
        for bad in (None, dict(boundary, elapsed_scale=3073),
                    dict(boundary, normal_frame_rate=0), dict(boundary, phase='polled'),
                    dict(boundary, elapsed_scale=True), dict(boundary, event='unknown')):
            with self.subTest(boundary=bad), self.assertRaises(ValueError):
                media.trigger_origin(dict(acknowledged=True, boundary=bad), observation(2, 18))

    def test_trigger_reset_or_future_event_is_rejected(self):
        boundary = dict(observation(1, 12), elapsed_scale=3072, normal_frame_rate=12,
                        event='placement_applied', phase='before_ordinary_update')
        for poll in (observation(.5, 18), observation(2, 11),
                     dict(observation(2, 18), scene='ISLAND')):
            with self.assertRaises(ValueError):
                media.trigger_origin(dict(acknowledged=True, boundary=boundary), poll)

    def test_pixels_have_clock_bounds_and_never_claim_atomic_pose(self):
        row = media.window(observation(), observation(1.25, 14), .2, .4, 'frame.png')
        self.assertEqual(row['game_seconds'], [1, 1.25])
        self.assertFalse(row['atomic'])
        self.assertEqual(row['before']['player']['position_q8'], [1, 2, 3])

    def test_paused_debugger_is_not_natural_capture(self):
        for key in ('before', 'after'):
            values = dict(before=observation(), after=observation(1.25, 14))
            values[key]['execution'] = 'stopped'
            with self.assertRaisesRegex(ValueError, 'running'):
                media.window(**values, started=.2, ended=.4, pixels='frame.png')

    def test_scene_reset_and_reversed_clocks_are_rejected(self):
        for after in (dict(observation(1.25, 14), scene='ISLAND'),
                      observation(.5, 14), observation(1.25, 11)):
            with self.assertRaises(ValueError):
                media.window(observation(), after, .2, .4, 'frame.png')

    def test_missing_actor_and_nonfinite_clock_are_rejected(self):
        for bad in (dict(observation(), player=None), observation(float('nan')),
                    dict(observation(), ready=False)):
            with self.assertRaises(ValueError):
                media.window(bad, observation(1.25, 14), .2, .4, 'frame.png')

    def test_pcm_delivery_cursor_and_incomplete_final_frame(self):
        import struct, wave
        with tempfile.TemporaryDirectory() as temp:
            raw, wav = Path(temp) / 'audio.raw', Path(temp) / 'audio.wav'
            payload = struct.pack('<hh', 1234, -1234) * 3
            raw.write_bytes(payload + b'x')
            self.assertEqual(media.pcm_frames(raw), 3)
            with self.assertRaisesRegex(ValueError, 'incomplete stereo'):
                media.finalize_pcm(raw, wav)
            raw.write_bytes(payload)
            media.finalize_pcm(raw, wav)
            with wave.open(str(wav)) as recording:
                self.assertEqual(recording.getparams()[:3], (2, 2, 48000))
                self.assertEqual(recording.readframes(3), payload)

    def test_non_raster_output_cannot_be_admitted_as_pixels(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / 'frame.png'
            path.write_text('logical pose only')
            with self.assertRaisesRegex(ValueError, 'PNG raster'):
                media.raster_metadata(path)

    def test_progress_uses_game_clock_and_preserves_gaps(self):
        rows = [media.window(observation(1, 12), observation(1.25, 14), .2, .4, '1.png'),
                media.window(observation(1.5, 17), observation(2, 20), .6, .9, '2.png')]
        result = media.summarize(rows, 1)
        self.assertEqual(result['game_seconds'], 1)
        self.assertFalse(result['exact_update_coverage'])
        with self.assertRaisesRegex(ValueError, 'progress'):
            media.summarize(rows, 2)


class MatchedIntervals(unittest.TestCase):
    def fixture(self, root, name, start, duration):
        directory = root / name
        directory.mkdir()
        def sample(scale, frame):
            return dict(observation(scale / 3072, frame), elapsed_scale=scale, normal_frame_rate=12)
        origin = dict(sample(start, 12), event='placement_applied', phase='before_ordinary_update')
        (directory / 'result.json').write_text(json.dumps(dict(success=True,
            trigger=dict(acknowledged=True, boundary=origin), trigger_observation=sample(start, 12),
            audio=dict(frames=48000))))
        row = media.window(sample(start, 12), sample(start + duration, 14), .1, .2, '1.png')
        (directory / 'windows.jsonl').write_text(json.dumps(row) + '\n')
        row.update(pcm_frames=[0, 48000], pixels=None)
        (directory / 'pcm-windows.jsonl').write_text(json.dumps(row) + '\n')
        return directory

    def test_exact_relative_intervals_allow_different_world_ages(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            left = self.fixture(root, 'left', 3072, 384)
            right = self.fixture(root, 'right', 6144, 384)
            command = [sys.executable, str(Path(__file__).with_name('gameplay-media-capture.py')),
                       '--compare-captures', str(left), str(right)]
            result = subprocess.run(command, capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertTrue(json.loads(result.stdout)['matched_clock_intervals'])
            self.assertTrue(media.matching_intervals(left, right)['matched_clock_intervals'])
            self.assertFalse(media.matching_intervals(left, right)['exact_pcm_event_mapping'])

    def test_passive_read_span_is_part_of_exact_interval_admission(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            left = self.fixture(root, 'left', 3072, 384)
            right = self.fixture(root, 'right', 6144, 384)
            path = right / 'windows.jsonl'
            row = json.loads(path.read_text())
            row['after']['elapsed_scale_end'] = row['after']['elapsed_scale'] + 1
            path.write_text(json.dumps(row))
            with self.assertRaisesRegex(ValueError, 'intervals differ'):
                media.matching_intervals(left, right)

    def test_pcm_clock_mismatch_cannot_hide_behind_matching_rasters(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            left = self.fixture(root, 'left', 3072, 384)
            right = self.fixture(root, 'right', 6144, 384)
            path = right / 'pcm-windows.jsonl'
            row = json.loads(path.read_text())
            row['after']['elapsed_scale'] += 128
            row['after']['game_seconds'] = row['after']['elapsed_scale'] / 3072
            path.write_text(json.dumps(row))
            with self.assertRaisesRegex(ValueError, 'intervals differ'):
                media.matching_intervals(left, right)
            row['pcm_frames'] = [1, 48000]
            path.write_text(json.dumps(row))
            with self.assertRaisesRegex(ValueError, 'gap'):
                media.matching_intervals(left, right)

    def test_one_eighth_and_one_sixth_second_cannot_match(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            left = self.fixture(root, 'left', 3072, 384)
            right = self.fixture(root, 'right', 6144, 512)
            with self.assertRaisesRegex(ValueError, 'intervals differ'):
                media.matching_intervals(left, right)
            path = right / 'windows.jsonl'
            row = json.loads(path.read_text())
            del row['after']['elapsed_scale']
            path.write_text(json.dumps(row))
            with self.assertRaisesRegex(ValueError, 'exact event-clock'):
                media.matching_intervals(left, right)


class PresentationPlans(unittest.TestCase):
    fixture = MatchedIntervals.fixture

    def presentations(self, directory, offsets, rate=12):
        result = json.loads((directory / 'result.json').read_text())
        origin = result['trigger']['boundary']['elapsed_scale']
        rows = []
        for sequence, offset in enumerate(offsets, 1):
            scale = origin + offset
            sample = dict(observation(scale / (256 * rate), 12 + sequence),
                elapsed_scale=scale, normal_frame_rate=rate, generation=1,
                event='render_frame', phase='sdl_surface_present', render_frame=sequence)
            row = media.window(sample, sample, sequence, sequence, f'{sequence}.png')
            row.update(clock_owner='sdl_surface_present', pcm_frames=[sequence * 10] * 2,
                       pcm_mapping='completed_mixer_callbacks')
            rows.append(row)
        (directory / 'windows.jsonl').write_text(''.join(json.dumps(row) + '\n' for row in rows))

    def test_public_plan_selects_actual_shared_points_despite_extra_receipts(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            left = self.fixture(root, 'left', 3072, 384)
            right = self.fixture(root, 'right', 6144, 384)
            self.presentations(left, [-192, 384, 768, 1152])
            self.presentations(right, [384, 512, 1152])
            with self.assertRaisesRegex(ValueError, 'intervals differ'):
                media.matching_intervals(left, right)
            result = subprocess.run([sys.executable, str(Path(__file__).with_name('gameplay-media-capture.py')),
                '--plan-presentations', str(left), str(right)], capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            plan = json.loads(result.stdout)
            self.assertTrue(plan['paired_window_available'])
            self.assertEqual([p['event_seconds'] for p in plan['pairs']], ['1/8', '3/8'])
            self.assertEqual([p['counterpart_elapsed_scale'] for p in plan['pairs']], [6528, 7296])
            self.assertEqual(plan['missing'][0]['source']['elapsed_scale'], 3840)
            self.assertFalse(plan['exact_pcm_event_mapping'])
            self.assertFalse(plan['pixel_comparison_performed'])

    def test_skipped_original_target_cannot_be_rounded_to_a_native_boundary(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            left = self.fixture(root, 'left', 21629, 384)
            right = self.fixture(root, 'right', 6144, 384)
            self.presentations(left, [3180])
            self.presentations(right, [3072, 3584])
            plan = media.plan_presentations(left, right)
            self.assertEqual(plan['pairs'], [])
            self.assertFalse(plan['paired_window_available'])
            self.assertEqual(plan['missing'][0]['counterpart_elapsed_scale'], 9324)
            self.assertEqual(plan['missing'][0]['event_seconds'], '265/256')

    def test_duplicate_or_polling_receipts_cannot_select_a_boundary(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            left = self.fixture(root, 'left', 3072, 384)
            right = self.fixture(root, 'right', 6144, 384)
            self.presentations(left, [384, 384])
            self.presentations(right, [384, 768])
            with self.assertRaisesRegex(ValueError, 'duplicate presentation clock'):
                media.plan_presentations(left, right)
            self.presentations(left, [384, 768])
            path = left / 'windows.jsonl'
            rows = [json.loads(line) for line in path.read_text().splitlines()]
            rows[0]['clock_owner'] = 'observation_bracket'
            path.write_text(''.join(json.dumps(row) + '\n' for row in rows))
            with self.assertRaisesRegex(ValueError, 'producer-owned'):
                media.plan_presentations(left, right)


class Capture(unittest.TestCase):
    def test_public_command_admits_pcm_before_trigger_and_retains_raster_windows(self):
        self.capture_fixture(False)

    def test_owned_capture_uses_render_boundary_without_per_raster_observation_requests(self):
        self.capture_fixture(True)

    def test_public_command_normalizes_the_published_pending_step(self):
        self.capture_fixture(True, pending=True)

    def test_monitor_command_does_not_require_the_optional_generation_helper(self):
        import shutil
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            for name in ('gameplay-media-capture.py', 'paced-mixer-capture.py'):
                shutil.copy2(Path(__file__).with_name(name), directory/name)
            result = subprocess.run([sys.executable, str(directory/'gameplay-media-capture.py'), '--help'],
                                    capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)

    def test_public_command_uses_generation_clock_without_monitor_delivery(self):
        self.capture_fixture(True, generated_audio=True)

    def capture_fixture(self, owned, pending=False, generated_audio=False):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            recorder = root / 'parec'
            recorder.write_text("""#!/usr/bin/env python3
import signal, struct, sys, time, wave
running = True
def stop(*args):
    global running
    running = False
signal.signal(signal.SIGINT, stop)
with open(sys.argv[-1], 'wb', buffering=0) as out:
    start = time.monotonic()
    frames = 0
    while running:
        needed = int((time.monotonic() - start) * 48000) - frames
        if needed > 0:
            out.write(struct.pack('<hh', 1000, -1000) * needed)
            frames += needed
        time.sleep(.005)
""")
            recorder.chmod(0o755)
            adapter = root / 'adapter.py'
            adapter.write_text("""import json, pathlib, sys
root = pathlib.Path(sys.argv[1])
mode = sys.argv[2]
if mode == 'observe':
    observations = root / 'observation-count'
    observations.write_text(str(int(observations.read_text()) + 1 if observations.exists() else 1))
    if (root / 'fail-observer').exists() and (root / 'triggered').exists():
        sys.stderr.write('actor observation interrupted after trigger');sys.exit(7)
    path = root / 'counter'
    count = int(path.read_text()) + 1 if path.exists() else 0
    path.write_text(str(count))
    print(json.dumps(dict(ready=True, scene='fixture', frame=count,
        game_seconds=count / 4, elapsed_scale=count * 768, normal_frame_rate=12, generation=1, execution='running', player={'health':100})))
elif mode == 'trigger':
    assert (root / 'capture/audio.raw').stat().st_size > 44
    (root / 'triggered').touch()
    count = int((root / 'counter').read_text())
    print(json.dumps(dict(acknowledged=True, boundary=dict(ready=True, scene='fixture',
        frame=count, game_seconds=count / 4, elapsed_scale=count * 768,
        normal_frame_rate=12, event='placement_applied', phase='before_ordinary_update',
        execution='stopped', player={'health':100}))))
else:
    import base64
    pathlib.Path(sys.argv[3]).write_bytes(base64.b64decode(
        'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aG1cAAAAASUVORK5CYII='))
    if mode == 'owned-frame':
        path = root / 'counter'
        count = int(path.read_text()) + 1
        path.write_text(str(count))
        cursor = (root / 'capture/audio.raw').stat().st_size // 4
        print(json.dumps(dict(captured=True, path=sys.argv[3], boundary=dict(
            ready=True, scene='fixture', frame=count, generation=1,
            game_seconds=count / 4, elapsed_scale=count * 768, normal_frame_rate=12,
            execution='running', player={'health':100}, event='render_frame',
            phase='render_extraction', render_frame=count, pcm_delivery_frames=cursor))))
""")
            if generated_audio:
                adapter.write_text(adapter.read_text().replace(
                    "assert (root / 'capture/audio.raw').stat().st_size > 44",
                    "assert (root / 'generated.raw').stat().st_size > 44").replace(
                    "cursor = (root / 'capture/audio.raw').stat().st_size // 4",
                    "import struct\n        cursor = count * 480\n"
                    "        (root / 'generated.raw').write_bytes(struct.pack('<hh', 1234, -1234) * cursor)\n"
                    "        (root / 'generated.jsonl').write_text(''.join(json.dumps(dict(sequence=i, "
                    "first=(i-1)*480, last=i*480, monotonic_ns=i*10000000, rate=48000))+'\\n' "
                    "for i in range(1, count+1)))").replace(
                    "pcm_delivery_frames=cursor", "pcm_delivery_frames=-1, pcm_generation=dict("
                    "frames=cursor, sequence=count, monotonic_ns=count*10000000, rate=48000)"))
            if pending:
                adapter.write_text(adapter.read_text().replace(
                    "count = int((root / 'counter').read_text())",
                    "count = int((root / 'counter').read_text()) + 1").replace(
                    "phase='before_ordinary_update',", "phase='before_ordinary_update', "
                    "clock_accounting='includes_pending_update', pending_frame_scale=768, "
                    "pending_accumulator_scale=768, startup_remaining=0,"))
            script = Path(__file__).with_name('gameplay-media-capture.py')
            import shlex
            prefix = shlex.join([sys.executable, str(adapter), str(root)])
            command = [sys.executable, str(script), str(root / 'capture'),
                '--observe', prefix + ' observe', '--trigger', prefix + ' trigger',
                '--screenshot', prefix + (' owned-frame' if owned else ' screenshot'), '--game-seconds', '1', '--timeout', '30']
            if owned:
                command.append('--owned-frames')
            if generated_audio:
                command += ['--generated-pcm', str(root)]
            env = {**os.environ, 'PATH': str(root) + ':' + os.environ['PATH']}
            result = subprocess.run(command, env=env, capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            report = json.loads((root / 'capture/result.json').read_text())
            self.assertTrue(report['success'])
            if generated_audio:
                self.assertTrue(report['audio']['mixer_generation_attribution'])
                self.assertFalse((root / 'capture/audio.raw').exists())
            if pending:
                self.assertEqual(report['trigger_boundary']['elapsed_scale'],
                                 report['trigger']['boundary']['elapsed_scale'] - 768)
                self.assertEqual(report['trigger_boundary']['excluded_pending_scale'], 768)
            self.assertGreaterEqual(report['post_trigger_game_seconds'], 1)
            self.assertGreater(report['trigger_observation']['game_seconds'],
                               report['trigger_boundary']['game_seconds'])
            self.assertGreater(report['audio']['peak'], 0)
            self.assertTrue((root / 'triggered').exists())
            rows = [json.loads(line) for line in (root / 'capture/windows.jsonl').read_text().splitlines()]
            self.assertGreaterEqual(len(rows), 2)
            self.assertLess(rows[0]['wall_seconds'][1], report['trigger_request_wall_seconds'][0])
            self.assertTrue(all((root / 'capture' / row['pixels']).exists() for row in rows))
            self.assertTrue(all('event_relative_scale' in row and 'pcm_frames' in row for row in rows))
            pcm = [json.loads(line) for line in (root / 'capture/pcm-windows.jsonl').read_text().splitlines()]
            self.assertEqual(pcm[0]['pcm_frames'][0], 0)
            self.assertEqual(pcm[-1]['pcm_frames'][1], report['audio']['frames'])
            self.assertTrue(all('clock_scale' in row and 'event_relative_scale' in row for row in pcm))
            if owned:
                self.assertEqual(int((root / 'observation-count').read_text()), 1 if generated_audio else 2)
                self.assertTrue(all(row['clock_owner'] == 'render_extraction' for row in rows))
                self.assertTrue(all(row['clock_scale'][0] == row['clock_scale'][1] for row in rows))
            self.assertFalse(report['audio']['exact_pcm_event_mapping'])
            self.assertEqual(report['pcm_unobserved_tail_frames'], 0)
            repeated = subprocess.run(command, env=env, capture_output=True, text=True)
            self.assertNotEqual(repeated.returncode, 0)
            self.assertEqual(json.loads((root / 'capture/result.json').read_text()), report)
            # A later observer failure retains live PCM and the earlier clock/cursor journal.
            if owned:
                return
            (root / 'triggered').unlink()
            (root / 'fail-observer').touch()
            command[2] = str(root / 'interrupted')
            interrupted = subprocess.run(command, env=env, capture_output=True, text=True)
            self.assertNotEqual(interrupted.returncode, 0)
            failed = json.loads((root / 'interrupted/result.json').read_text())
            self.assertFalse(failed['success'])
            self.assertGreater(failed['pcm_delivery_frames'], 0)
            self.assertGreaterEqual(failed['pcm_unobserved_tail_frames'], 0)
            points = (root / 'interrupted/observations.jsonl').read_text().splitlines()
            self.assertGreaterEqual(len(points), 3)
            self.assertIn('actor observation interrupted after trigger',
                          (root / 'interrupted/adapters.log').read_text())


    def test_failed_observer_preserves_diagnostic_without_trigger_or_pcm(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            with self.assertRaises(subprocess.CalledProcessError):
                media.capture(root / 'capture', [sys.executable, '-c',
                    "import sys;sys.stderr.write('actor unavailable');sys.exit(7)"],
                    ['must-not-trigger'], ['must-not-capture'], 1, 5, .1)
            self.assertIn('actor unavailable', (root / 'capture/adapters.log').read_text())
            self.assertFalse(json.loads((root / 'capture/result.json').read_text())['success'])
            self.assertFalse((root / 'capture/audio.raw').exists())


if __name__ == '__main__':
    unittest.main()
