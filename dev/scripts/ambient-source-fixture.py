#!/usr/bin/env python3
"""Export a local mono PCM oracle from an installed sound WAV and SYSTEM.INI.

Inputs and output contain installed data and must remain outside tracked files.
"""
import argparse
import array
import hashlib
import json
import math
from pathlib import Path
import sys
import wave


def source_levels(path):
    with wave.open(str(path), 'rb') as audio:
        if audio.getnchannels() != 1 or audio.getsampwidth() not in (1, 2):
            raise ValueError('ambient oracle requires mono 8-bit or 16-bit PCM')
        width = audio.getsampwidth()
        frames, rate = audio.getnframes(), audio.getframerate()
        raw = audio.readframes(frames)
    if not raw:
        raise ValueError('ambient oracle requires nonempty PCM')
    if width == 1:
        values = [(value - 128) / 128 for value in raw]
    else:
        samples = array.array('h')
        samples.frombytes(raw)
        if sys.byteorder != 'little':
            samples.byteswap()
        values = [value / 32768 for value in samples]
    return {'frames': frames, 'rate': rate, 'bits': width * 8, 'source_rms': math.sqrt(sum(v*v for v in values) / len(values)),
            'wav_sha256': hashlib.sha256(path.read_bytes()).hexdigest()}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('wav', type=Path)
    parser.add_argument('system_ini', type=Path)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    result = source_levels(args.wav)
    result['system'] = args.system_ini.read_text(encoding='latin1')
    args.output.write_text(json.dumps(result))


if __name__ == '__main__':
    main()
