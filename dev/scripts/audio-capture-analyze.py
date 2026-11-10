#!/usr/bin/env python3
"""Analyse a capture from original-audio-capture.sh.

Reads <out>/audio.wav (DOSBox-X's emulated-time recording) and <out>/timeline.json (the start and
stop frames and each key event's wall time and game frame). For every event it reports the audio
time implied by two clocks (wall time scaled to the WAV length, and the game-frame count scaled
the same way), and the first energy rise after each implied time. With --reference it also finds
the best normalised-correlation position of a known sound (a WAV) in the recording, which
validates the rig: a known effect must be found at the expected event.

Usage: audio-capture-analyze.py <out-dir> [--reference file.wav] [--window-ms 20]
"""
import argparse
import json
import math
import struct
import wave
from pathlib import Path


def load(path):
    with wave.open(str(path)) as w:
        rate, channels, width = w.getframerate(), w.getnchannels(), w.getsampwidth()
        raw = w.readframes(w.getnframes())
    count = len(raw) // (width * channels)
    if width == 2:
        data = struct.unpack(f"<{count * channels}h", raw[: count * channels * 2])
    else:
        data = [(b - 128) * 256 for b in raw[: count * channels]]
    return [sum(data[i * channels : (i + 1) * channels]) / channels for i in range(count)], rate


def high_pass(x):
    return [x[i] - x[i - 1] for i in range(1, len(x))]


def best_matches(capture, reference, count=6):
    """Normalised correlation of the high-passed, 8 kHz reference against the capture."""
    cap, cap_rate = capture
    ref, ref_rate = reference
    factor = max(1, cap_rate // 8000)
    cap8 = [sum(cap[i : i + factor]) / factor for i in range(0, len(cap) - factor + 1, factor)]
    rate8 = cap_rate / factor
    ref8 = [ref[min(len(ref) - 1, int(i * ref_rate / rate8))] for i in range(int(len(ref) * rate8 / ref_rate))]
    c, r = high_pass(cap8), high_pass(ref8)
    norm = math.sqrt(sum(v * v for v in r)) or 1.0
    energy = [0.0]
    for v in c:
        energy.append(energy[-1] + v * v)
    scores = []
    for offset in range(0, len(c) - len(r), 2):
        dot = sum(a * b for a, b in zip(c[offset : offset + len(r)], r))
        local = energy[offset + len(r)] - energy[offset]
        scores.append((dot / (norm * math.sqrt(local) + 1e-9), offset / rate8))
    return sorted(sorted(scores, reverse=True)[:count], key=lambda s: s[1])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("out", type=Path)
    parser.add_argument("--reference", type=Path)
    parser.add_argument("--window-ms", type=int, default=20)
    args = parser.parse_args()
    timeline = json.loads((args.out / "timeline.json").read_text())
    samples, rate = load(args.out / "audio.wav")
    seconds = len(samples) / rate
    start, stop = timeline["start"], timeline["stop"]
    wall_span = stop["wall"] - start["wall"]
    frame_span = max(1, stop["frame"] - start["frame"])
    print(f"audio {seconds:.2f}s at {rate} Hz; wall {wall_span:.2f}s; frames {frame_span}")
    window = rate * args.window_ms // 1000
    env = [math.sqrt(sum(v * v for v in samples[i : i + window]) / window) for i in range(0, len(samples) - window, window)]
    for t, action, key, frame in timeline["events"]:
        by_wall = t * seconds / wall_span
        by_frame = (frame - start["frame"]) * seconds / frame_span
        print(f"event {action} {key}: wall {t:.2f}s -> audio {by_wall:.2f}s by wall, {by_frame:.2f}s by frame")
        for label, at in (("wall", by_wall), ("frame", by_frame)):
            index = int(at * 1000 / args.window_ms)
            floor = sorted(env[max(0, index - 20) : index])[len(env[max(0, index - 20) : index]) // 2] if index else 0
            rise = next((i for i in range(index, min(len(env), index + 100)) if env[i] > 1.5 * floor + 200), None)
            if rise is not None:
                print(f"   first rise after the {label}-mapped time: +{(rise - index) * args.window_ms / 1000:.2f}s")
    if args.reference:
        reference = load(args.reference)
        print(f"reference {args.reference.name}: {len(reference[0]) / reference[1]:.3f}s")
        for score, at in best_matches((samples, rate), reference):
            print(f"   match {score:.3f} at {at:.2f}s")


main()
