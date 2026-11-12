#!/usr/bin/env python3
"""One admitted recording must contain its calibration, cues and clock bounds."""
import importlib.util
import hashlib
import json
from pathlib import Path
import tempfile
import unittest
import numpy as np
from scipy.io import wavfile

spec = importlib.util.spec_from_file_location('paired_check', Path(__file__).with_name('check-paired-book-audio.py'))
check = importlib.util.module_from_spec(spec)
spec.loader.exec_module(check)

class Admission(unittest.TestCase):
    def test_clock_bounds_include_epoch_uncertainty_and_reject_missing_or_reset_samples(self):
        samples = [dict(before=10, after=10.1, game_seconds=2),
                   dict(before=11, after=11.1, game_seconds=2.5),
                   dict(before=12, after=12.1, game_seconds=3)]
        self.assertEqual(check.clock_bounds(samples,10.5,11.5),[2,3])
        for values, low, high in [(samples,9,11),(samples,11,13),
            ([samples[0],dict(samples[1],game_seconds=1)],10.2,10.5)]:
            with self.assertRaises(ValueError):check.clock_bounds(values,low,high)

    def test_shared_epoch_error_does_not_become_independent_cue_jitter(self):
        samples = [dict(before=i/10, after=i/10, game_seconds=i/10) for i in range(21)]
        bounds=check.cadence_bounds(samples,.6,.8,.25)
        self.assertGreaterEqual(bounds[0],.099999999)
        self.assertLessEqual(bounds[1],.400000001)

    def test_single_recording_unity_and_cue_pass_but_interfering_source_rejects(self):
        rate=48000;rng=np.random.default_rng(62)
        calibration=rng.integers(-4000,4000,(rate,2),dtype=np.int16)
        cue=rng.integers(-2000,2000,rate,dtype=np.int16)
        pcm=np.zeros((rate*5,2),dtype=np.int16)
        pcm[rate//2:rate//2+rate]=calibration
        pcm[3*rate:4*rate]=cue[:,None]
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp);(root/'capture').mkdir();(root/'calibration').mkdir()
            ref=root/'calibration/reference.wav';wavfile.write(ref,rate,calibration)
            effect=root/'effect.wav';wavfile.write(effect,rate,cue)
            (root/'calibration/result.json').write_text(json.dumps(dict(success=True,
                reference_sha256=hashlib.sha256(ref.read_bytes()).hexdigest())))
            (root/'capture/result.json').write_text(json.dumps(dict(success=True,
                epoch_monotonic_seconds=100,pacing_tolerance_seconds=.25,
                transitions=[dict(sent=2.8,acknowledged=4.2)])))
            (root/'calibration-settings.json').write_text(json.dumps(dict(
                before={'mute':False},after={'mute':False},started=100,restored=102)))
            (root/'clocks.jsonl').write_text(''.join(json.dumps(dict(before=100+i/10,
                after=100+i/10+.001,game_seconds=i/10))+'\n' for i in range(51)))
            path=root/'capture/audio.wav';wavfile.write(path,rate,pcm)
            result=check.analyze(root,[effect]);self.assertTrue(result['success'])
            self.assertEqual(result['clock_samples'],51)
            # A second live producer's DC output preserves timing and correlation,
            # but does not establish exact known-signal unity.
            contaminated=pcm.copy();contaminated[:2*rate]+=1
            wavfile.write(path,rate,contaminated)
            self.assertFalse(check.analyze(root,[effect])['success'])

if __name__ == '__main__':unittest.main()
