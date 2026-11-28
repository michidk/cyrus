#!/usr/bin/env python3
"""Whole-archive verification survives interrupted range acquisition."""
import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('digest', Path(__file__).with_name('resume-artifact-digest.py'))
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)


class Tests(unittest.TestCase):
    def test_checkpoint_and_result_publication_is_durable(self):
        with tempfile.TemporaryDirectory() as d, patch.object(m.os,'fsync',wraps=m.os.fsync) as synced:
            path=Path(d)/'result.json';m.atomic_json(path,{'complete':True})
            self.assertEqual(synced.call_count,2)
            self.assertEqual(json.loads(path.read_text()),{'complete':True})
            self.assertFalse(path.with_suffix('.json.partial').exists())

    def test_sha_state_roundtrip_matches_standard_digest_at_partial_blocks(self):
        data = bytes(range(256)) * 19
        for split in [0, 1, 63, 64, 65, 1000, len(data)]:
            first = m.Digest(); first.update(data[:split])
            resumed = m.Digest(json.loads(json.dumps(first.state())))
            resumed.update(data[split:])
            self.assertEqual(resumed.hexdigest(), hashlib.sha256(data).hexdigest())

    def test_interruption_resumes_without_repeating_accepted_ranges(self):
        with tempfile.TemporaryDirectory() as d:
            checkpoint, receipt = Path(d)/'checkpoint', Path(d)/'receipt'
            data = bytes(range(233)) * 17
            identity = dict(artifact=17, run=19, size=len(data), sha256=hashlib.sha256(data).hexdigest())
            def stop(offset, count):
                if offset >= 130: raise OSError('replacement')
                return data[offset:offset+count]
            with self.assertRaises(OSError): m.authenticate(identity, checkpoint, receipt, stop, 65)
            self.assertFalse(receipt.exists())
            offsets = []
            def resume(offset, count):
                offsets.append(offset);return data[offset:offset+count]
            self.assertEqual(m.authenticate(identity, checkpoint, receipt, resume, 997), identity['sha256'])
            self.assertEqual(offsets[0], 130)
            self.assertEqual(json.loads(receipt.read_text())['bytes'], len(data))
            wrong = dict(identity, artifact=18)
            with self.assertRaisesRegex(ValueError, 'another immutable'): m.authenticate(wrong, checkpoint, receipt, resume)

    def test_corruption_short_read_and_invalid_position_never_publish(self):
        with tempfile.TemporaryDirectory() as d:
            checkpoint, receipt = Path(d)/'checkpoint', Path(d)/'receipt'
            identity = dict(artifact=1, size=3, sha256=hashlib.sha256(b'abc').hexdigest())
            with self.assertRaisesRegex(ValueError, 'incomplete'): m.authenticate(identity, checkpoint, receipt, lambda *_: b'a')
            self.assertFalse(checkpoint.exists())
            with self.assertRaisesRegex(ValueError, 'digest mismatch'): m.authenticate(identity, checkpoint, receipt, lambda *_: b'xyz')
            self.assertFalse(receipt.exists())
            saved=json.loads(checkpoint.read_text());saved['offset']=2;checkpoint.write_text(json.dumps(saved))
            with self.assertRaisesRegex(ValueError, 'position'): m.authenticate(identity, checkpoint, receipt, lambda *_: b'abc')


if __name__ == '__main__': unittest.main()
