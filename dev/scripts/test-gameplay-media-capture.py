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
                execution=execution, player={'position_q8': [1, 2, 3]})


class Windows(unittest.TestCase):
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
            trigger=dict(acknowledged=True, boundary=origin), trigger_observation=sample(start, 12))))
        row = media.window(sample(start, 12), sample(start + duration, 14), .1, .2, '1.png')
        (directory / 'windows.jsonl').write_text(json.dumps(row) + '\n')
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
with wave.open(sys.argv[-1], 'wb') as out:
    out.setparams((2, 2, 48000, 0, 'NONE', 'not compressed'))
    start = time.monotonic()
    frames = 0
    while running:
        needed = int((time.monotonic() - start) * 48000) - frames
        if needed > 0:
            out.writeframes(struct.pack('<hh', 1000, -1000) * needed)
            frames += needed
        time.sleep(.005)
""")
            recorder.chmod(0o755)
            adapter = root / 'adapter.py'
            adapter.write_text("""import json, pathlib, sys
root = pathlib.Path(sys.argv[1])
mode = sys.argv[2]
if mode == 'observe':
    path = root / 'counter'
    count = int(path.read_text()) + 1 if path.exists() else 0
    path.write_text(str(count))
    print(json.dumps(dict(ready=True, scene='fixture', frame=count,
        game_seconds=count / 4, execution='running', player={'health':100})))
elif mode == 'trigger':
    assert (root / 'capture/audio.wav').stat().st_size > 44
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
""")
            script = Path(__file__).with_name('gameplay-media-capture.py')
            import shlex
            prefix = shlex.join([sys.executable, str(adapter), str(root)])
            command = [sys.executable, str(script), str(root / 'capture'),
                '--observe', prefix + ' observe', '--trigger', prefix + ' trigger',
                '--screenshot', prefix + ' screenshot', '--game-seconds', '1', '--timeout', '5']
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
            repeated = subprocess.run(command, env=env, capture_output=True, text=True)
            self.assertNotEqual(repeated.returncode, 0)
            self.assertEqual(json.loads((root / 'capture/result.json').read_text()), report)

    def test_failed_observer_preserves_diagnostic_without_trigger_or_pcm(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            with self.assertRaises(subprocess.CalledProcessError):
                media.capture(root / 'capture', [sys.executable, '-c',
                    "import sys;sys.stderr.write('actor unavailable');sys.exit(7)"],
                    ['must-not-trigger'], ['must-not-capture'], 1, 5, .1)
            self.assertIn('actor unavailable', (root / 'capture/adapters.log').read_text())
            self.assertFalse(json.loads((root / 'capture/result.json').read_text())['success'])
            self.assertFalse((root / 'capture/audio.wav').exists())


if __name__ == '__main__':
    unittest.main()
