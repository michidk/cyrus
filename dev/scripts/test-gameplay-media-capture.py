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


class Capture(unittest.TestCase):
    def test_public_command_admits_pcm_before_trigger_and_retains_raster_windows(self):
        self.capture_fixture(False)

    def test_owned_capture_uses_render_boundary_without_per_raster_observation_requests(self):
        self.capture_fixture(True)

    def capture_fixture(self, owned):
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
            script = Path(__file__).with_name('gameplay-media-capture.py')
            import shlex
            prefix = shlex.join([sys.executable, str(adapter), str(root)])
            command = [sys.executable, str(script), str(root / 'capture'),
                '--observe', prefix + ' observe', '--trigger', prefix + ' trigger',
                '--screenshot', prefix + (' owned-frame' if owned else ' screenshot'), '--game-seconds', '1', '--timeout', '5']
            if owned:
                command.append('--owned-frames')
            env = {**os.environ, 'PATH': str(root) + ':' + os.environ['PATH']}
            result = subprocess.run(command, env=env, capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            report = json.loads((root / 'capture/result.json').read_text())
            self.assertTrue(report['success'])
            self.assertGreaterEqual(report['post_trigger_game_seconds'], 1)
            self.assertGreater(report['trigger_observation']['game_seconds'],
                               report['trigger_boundary']['game_seconds'])
            self.assertGreater(report['audio']['peak'], 0)
            self.assertTrue((root / 'triggered').exists())
            rows = [json.loads(line) for line in (root / 'capture/windows.jsonl').read_text().splitlines()]
            self.assertGreaterEqual(len(rows), 2)
            self.assertLess(rows[0]['wall_seconds'][1], report['audio']['transitions'][0]['sent'])
            self.assertTrue(all((root / 'capture' / row['pixels']).exists() for row in rows))
            self.assertTrue(all('event_relative_scale' in row and 'pcm_frames' in row for row in rows))
            pcm = [json.loads(line) for line in (root / 'capture/pcm-windows.jsonl').read_text().splitlines()]
            self.assertEqual(pcm[0]['pcm_frames'][0], 0)
            self.assertEqual(pcm[-1]['pcm_frames'][1], report['audio']['frames'])
            self.assertTrue(all('clock_scale' in row and 'event_relative_scale' in row for row in pcm))
            if owned:
                self.assertEqual(int((root / 'observation-count').read_text()), 2)
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
