#!/usr/bin/env python3
import importlib.util
from pathlib import Path
import tempfile
import unittest
import numpy as np
from scipy.io import wavfile
spec = importlib.util.spec_from_file_location('calibration', Path(__file__).with_name('check-monitor-calibration.py'))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)

class Calibration(unittest.TestCase):
    def test_unity_passes_and_gain_gap_or_pacing_failure_rejects(self):
        signal = np.random.default_rng(62).integers(-4000, 4000, 8000, dtype=np.int16)
        with tempfile.TemporaryDirectory() as root:
            root = Path(root); ref = root/'reference.wav'; monitor = root/'monitor.wav'
            wavfile.write(ref, 8000, signal)
            for value, pacing, expected in [
                (signal, True, True), (signal//2, True, False), (signal, False, False),
                (np.concatenate((signal[:4000],np.zeros(400,dtype=np.int16),signal[4000:])),True,False)]:
                wavfile.write(monitor,8000,np.concatenate((np.zeros(800,dtype=np.int16),value)))
                self.assertEqual(module.analyze(monitor,ref,{'success':pacing})['success'], expected)

    def test_opposite_channel_gains_cannot_average_to_unity(self):
        signal=np.random.default_rng(62).integers(-2000,2000,8000,dtype=np.int16)
        with tempfile.TemporaryDirectory() as root:
            root=Path(root);a=root/'a.wav';b=root/'b.wav'
            wavfile.write(a,8000,np.column_stack((signal*2,np.zeros_like(signal))))
            wavfile.write(b,8000,np.column_stack((signal,signal)))
            self.assertFalse(module.analyze(a,b,{'success':True})['success'])

    def test_different_rate_requires_its_own_calibration(self):
        with tempfile.TemporaryDirectory() as root:
            root=Path(root); a=root/'a.wav';b=root/'b.wav'
            wavfile.write(a,8000,np.zeros(8000,dtype=np.int16));wavfile.write(b,16000,np.zeros(16000,dtype=np.int16))
            with self.assertRaisesRegex(ValueError,'same-rate'): module.analyze(a,b,{'success':True})

if __name__ == '__main__': unittest.main()
