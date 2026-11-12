#!/usr/bin/env python3
"""Run against the real SDL2 dummy backend and the built public interposer.

Usage: test-sdl-frame-capture-runtime.py FIXTURE_EXECUTABLE INTERPOSER_SO FAILED_BACKEND_SO [MIXER_SO]
"""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
from PIL import Image

fixture, interposer, failed_backend = map(lambda value: str(Path(value).resolve()), sys.argv[1:4])
mixer = str(Path(sys.argv[4]).resolve()) if len(sys.argv) > 4 else None
with tempfile.TemporaryDirectory() as temporary:
    root = Path(temporary)
    absent = root / 'absent'
    absent.mkdir()
    absent_output = absent / 'output'
    absent_output.mkdir()
    control = subprocess.run([fixture, str(absent_output), 'valid'], timeout=10,
        env={**os.environ, 'SDL_VIDEODRIVER': 'dummy', 'LD_PRELOAD': '',
             'CYRUS_FRAME_CAPTURE_DIR': str(absent)})
    assert control.returncode == 5, control.returncode
    assert not (absent_output / 'result.json').exists()
    modes = ['valid', 'rectangles', 'invalid', 'failed']
    if mixer:
        modes.append('generated')
    for mode in modes:
        request = root / mode
        request.mkdir()
        output = request / 'output'
        output.mkdir()
        subprocess.run([fixture, str(output), mode], check=True, timeout=10,
                       env={**os.environ, 'SDL_VIDEODRIVER': 'dummy', 'LD_PRELOAD': interposer + (':' + failed_backend if mode == 'failed' else '') + (':' + mixer if mode == 'generated' else ''),
                            'SDL_AUDIODRIVER': 'dummy',
                            **({'CYRUS_MIXER_CAPTURE_DIR': str(request)} if mode == 'generated' else {}),
                            'CYRUS_FRAME_CAPTURE_DIR': str(request)})
        result = json.loads((output / 'result.json').read_text())
        assert result['owner'] == 'sdl_surface_present'
        assert result['sequence'] > 0
        if mode not in ('invalid', 'failed'):
            assert result['ok'] is True, result
            if mode == 'generated':
                assert result['pcm_generation']['frames'] > 0
                assert result['pcm_generation']['rate'] == 48000
                assert result['pcm_generation']['sequence'] > 0
                assert result['pcm_delivery_frames'] == -1
            assert int.from_bytes(bytes.fromhex(result['fields']['clock']), 'little') == 123
            with Image.open(output / 'frame.bmp') as image:
                assert image.convert('RGB').getpixel((8, 8)) == (255, 0, 0)
        else:
            assert result['ok'] is False, result
            if mode == 'invalid':
                assert not (output / 'frame.bmp').exists()
print('SDL presentation: frozen red pixels/clock survive subsequent blue/999 state; invalid binding rejected')
