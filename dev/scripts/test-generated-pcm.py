#!/usr/bin/env python3
"""Producer sample-clock controls, including delayed/truncated writer delivery."""
import importlib.util
import json
from pathlib import Path
import struct
import tempfile
import unittest
import wave

spec = importlib.util.spec_from_file_location('generated', Path(__file__).with_name('generated-pcm.py'))
generated = importlib.util.module_from_spec(spec); spec.loader.exec_module(generated)
spec = importlib.util.spec_from_file_location('monitor', Path(__file__).with_name('paced-mixer-capture.py'))
monitor = importlib.util.module_from_spec(spec); spec.loader.exec_module(monitor)


def block(sequence):
    return dict(sequence=sequence, first=(sequence-1)*480, last=sequence*480,
                monotonic_ns=sequence*10000000, rate=48000)


def cursor(sequence):
    row = block(sequence)
    return dict(sequence=sequence, frames=row['last'], monotonic_ns=row['monotonic_ns'], rate=row['rate'])


class Generated(unittest.TestCase):
    def test_sample_clock_does_not_use_delivery_or_spawn_wall_time(self):
        rows = [dict(block(i), delivery_monotonic_ns=i*10000000+900000000) for i in range(1, 4)]
        result = generated.validate(rows, cursor(1), cursor(3))
        self.assertEqual(result['frames'], 960)
        self.assertEqual(result['seconds'], result['generation_seconds'])
        # The old process/recorder-wall comparison would reject this .9s delivery lag.
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary)/'delayed.wav'
            with wave.open(str(path), 'wb') as target:
                target.setparams((2, 2, 48000, 0, 'NONE', 'not compressed'))
                target.writeframes(struct.pack('<hh', 1234, -1234)*960)
            with self.assertRaisesRegex(ValueError, 'pacing'):
                monitor.validate(path, .92, [dict(sent=.1, acknowledged=.2, **{'from': 0, 'to': 1})], .25)

    def test_missing_duplicate_truncated_or_wrong_clock_blocks_reject(self):
        rows = [block(i) for i in range(1, 4)]
        for bad in (rows[1:], [rows[0], rows[2]], [rows[0], rows[0], rows[1]],
                    rows[:2], [rows[0], dict(rows[1], first=0), rows[2]],
                    [rows[0], dict(rows[1], rate=44100), rows[2]],
                    [rows[0], rows[1], dict(rows[2], monotonic_ns=900000000)]):
            with self.subTest(blocks=bad), self.assertRaises(ValueError):
                generated.validate(bad, cursor(1), cursor(3))
        with self.assertRaises(ValueError):
            generated.validate(rows, cursor(1), dict(cursor(3), frames=1441))
        with self.assertRaisesRegex(ValueError, 'pacing'):
            generated.validate([dict(block(i), monotonic_ns=i*1000000000) for i in range(1, 4)],
                               dict(cursor(1), monotonic_ns=1000000000),
                               dict(cursor(3), monotonic_ns=3000000000))

    def test_stall_and_catchup_cannot_hide_generation_gap(self):
        rows = [block(i) for i in range(1, 101)]
        for row in rows[1:]:
            row['monotonic_ns'] = 510000000 + (row['sequence']-2)*5000000
        # Total sample duration and endpoint duration still agree exactly.
        self.assertEqual((rows[-1]['monotonic_ns']-rows[0]['monotonic_ns'])/1e9, .99)
        with self.assertRaisesRegex(ValueError, 'discontinuity'):
            generated.validate(rows, cursor(1), cursor(100))

    def test_export_preserves_real_rate_and_rejects_truncated_or_failed_producer(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            log = root / 'generated.jsonl'
            log.write_text(''.join(json.dumps(block(i))+'\n' for i in range(1, 4)))
            raw = root / 'generated.raw'
            raw.write_bytes(struct.pack('<hh', 1234, -1234)*1440)
            result = generated.extract(root, cursor(1), cursor(3), root/'ok.wav')
            self.assertEqual(result['peak'], 1234)
            raw.write_bytes(raw.read_bytes()[:-4])
            with self.assertRaisesRegex(ValueError, 'truncated'):
                generated.extract(root, cursor(1), cursor(3), root/'bad.wav')
            log.write_text(log.read_text()+'{"error":"overflow"}\n')
            with self.assertRaisesRegex(ValueError, 'discontinuity'):
                generated.extract(root, cursor(1), cursor(3), root/'bad.wav')

    def test_native_float_generation_retains_exact_raw_hash_and_delivery_independence(self):
        import hashlib
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            rows = [dict(block(i), format='f32le') for i in range(1, 4)]
            (root/'generated.jsonl').write_text(''.join(json.dumps(row)+'\n' for row in rows))
            raw = struct.pack('<ff', .5, -.5)*1440
            (root/'generated.raw').write_bytes(raw)
            first, last = dict(cursor(1), format='f32le'), dict(cursor(3), format='f32le')
            result = generated.extract(root, first, last, root/'native.wav')
            self.assertEqual(result['generated_raw_sha256'], hashlib.sha256(raw[480*8:]).hexdigest())
            self.assertEqual(result['frames'], 960)
            self.assertEqual(result['wav_conversion'], 'f32le_to_s16le_preview')
            with wave.open(str(root/'native.wav')) as source:
                self.assertEqual(source.getnframes(), 960)
            # A changed format or a genuine producer stall must still reject.
            rows[1]['format'] = 's16le'
            with self.assertRaises(ValueError):
                generated.validate(rows, first, last)
            rows[1]['format'] = 'f32le'
            rows[1]['monotonic_ns'] += 900000000
            with self.assertRaises(ValueError):
                generated.validate(rows, first, last)

    def test_rejected_generation_report_identifies_callback_stall_not_delivery(self):
        rows = [dict(block(i), delivery_monotonic_ns=i*10000000+900000000) for i in range(1,4)]
        report = generated.pacing_report(rows, cursor(1), cursor(3))
        self.assertFalse(report['acceptance'])
        self.assertEqual(report['discontinuities'], [])
        rows[2]['monotonic_ns'] += 900000000
        report = generated.pacing_report(rows, cursor(1), dict(cursor(3), monotonic_ns=930000000))
        self.assertEqual(report['discontinuities'][0]['sequence'], 3)
        self.assertAlmostEqual(report['largest_difference']['difference_seconds'], .9)
        with self.assertRaisesRegex(ValueError, 'discontinuity'):
            generated.validate(rows, cursor(1), dict(cursor(3), monotonic_ns=930000000))


if __name__ == '__main__':
    unittest.main()
