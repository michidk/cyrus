#!/usr/bin/env python3
"""Exercise the actual SDL callback tap, delayed writer, missing tap and overflow.

Arguments: public SDL fixture, mixer interposer, stalled-writer interposer.
"""
import importlib.util
import json
import os
from pathlib import Path
import struct
import subprocess
import sys
import tempfile

spec = importlib.util.spec_from_file_location('generated', Path(__file__).with_name('generated-pcm.py'))
generated = importlib.util.module_from_spec(spec); spec.loader.exec_module(generated)
fixture, tap, stall = [str(Path(value).resolve()) for value in sys.argv[1:]]
with tempfile.TemporaryDirectory() as temporary:
    root = Path(temporary)
    for name, preload, arguments, expected in (
            ('absent', '', [], 3), ('live', tap, [], 0),
            ('delayed', tap+':'+stall, [], 0),
            ('overflow', tap+':'+stall, ['overflow'], 3),
            ('unsupported', tap, ['unsupported'], 2)):
        directory = root/name; directory.mkdir()
        result = subprocess.run([fixture, *arguments], capture_output=True, text=True, timeout=15,
            env={**os.environ, 'SDL_AUDIODRIVER': 'dummy', 'LD_PRELOAD': preload,
                 'CYRUS_MIXER_CAPTURE_DIR': str(directory)})
        assert result.returncode == expected, (name, result.returncode, result.stderr)
        if name == 'absent':
            assert not (directory/'generated.jsonl').exists()
            continue
        blocks = [json.loads(line) for line in (directory/'generated.jsonl').read_text().splitlines()]
        if name == 'unsupported':
            assert not blocks
            continue
        if name == 'overflow':
            assert any('error' in row for row in blocks), blocks[-1]
            continue
        endpoint = json.loads(result.stdout)
        last = {key: endpoint[key] for key in ('frames', 'sequence', 'monotonic_ns', 'rate')}
        first = dict(frames=blocks[0]['last'], sequence=1,
                     monotonic_ns=blocks[0]['monotonic_ns'], rate=blocks[0]['rate'])
        metadata = generated.extract(directory, first, last, directory/'audio.wav')
        assert metadata['peak'] == 1234
        pcm = (directory/'generated.raw').read_bytes()
        assert pcm == struct.pack('<hh', 1234, -1234)*(len(pcm)//4)
        if name == 'delayed':
            assert endpoint['delivered_frames'] < endpoint['frames'], endpoint
print('SDL mixer: generation-owned PCM survives delayed delivery; absent/overflow/format controls reject')
