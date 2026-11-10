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


def stacked_response(capture, reference, event_times, before=0.2, after=0.8):
    """Average normalised correlation of a known sound at each lag relative to the events.

    A real stimulus shows one lag (the latency from key press to sound start) where the averaged
    correlation stands out from the other lags; the ratio of the peak to the median lag is the
    detection strength. Event times are on the recording's own clock."""
    cap, cap_rate = capture
    ref, ref_rate = reference
    factor = max(1, cap_rate // 8000)
    rate8 = cap_rate / factor
    cap8 = [sum(cap[i : i + factor]) / factor for i in range(0, len(cap) - factor + 1, factor)]
    ref8 = [ref[min(len(ref) - 1, int(i * ref_rate / rate8))] for i in range(int(len(ref) * rate8 / ref_rate))]
    c, r = high_pass(cap8), high_pass(ref8)
    norm = math.sqrt(sum(v * v for v in r)) or 1.0
    lags = range(-int(before * rate8), int(after * rate8))
    totals = [0.0] * len(lags)
    used = 0
    for t in event_times:
        base = int(t * rate8)
        if base + lags[-1] + len(r) >= len(c) or base + lags[0] < 0:
            continue
        used += 1
        for k, lag in enumerate(lags):
            seg = c[base + lag : base + lag + len(r)]
            local = math.sqrt(sum(v * v for v in seg)) or 1.0
            totals[k] += sum(a * b for a, b in zip(seg, r)) / (norm * local)
    if not used:
        return None
    mean = [v / used for v in totals]
    peak = max(range(len(mean)), key=lambda k: mean[k])
    median = sorted(mean)[len(mean) // 2]
    spread = sorted(abs(v - median) for v in mean)[len(mean) // 2] or 1e-9
    return {"events": used, "peak_lag_s": lags[peak] / rate8, "peak": mean[peak], "median": median, "peak_over_noise": (mean[peak] - median) / spread}


def stacked_envelope(samples, rate, event_times, before=0.5, after=2.0):
    """Mean high-passed energy envelope (10 ms) around the events, each divided by its own
    pre-event median, so a sound locked to the stimulus shows as a bump above 1.0 at one lag
    while the background bed averages to about 1.0. Needs no template of the sound."""
    hp = high_pass(samples)
    window = rate // 100
    steps = int((before + after) * 100)
    total, used = [0.0] * steps, 0
    for t in event_times:
        start = int((t - before) * rate)
        if start < 0 or start + steps * window > len(hp):
            continue
        env = [math.sqrt(sum(v * v for v in hp[start + k * window : start + (k + 1) * window]) / window) for k in range(steps)]
        base = sorted(env[: int(before * 100) - 10])[int(before * 50)] + 1
        for k in range(steps):
            total[k] += env[k] / base
        used += 1
    return [v / used for v in total] if used else None


def emulated_clock(timeline, rate):
    """Map script time to emulated audio seconds: the recording grows in 64 KiB steps, so the
    clock is interpolated linearly between the wall times at which each step appeared."""
    steps = []
    last = 0
    for t, size in timeline.get("sizes", []):
        if size > last:
            steps.append((t, (size - 44) / (rate * 4)))
            last = size
    if len(steps) < 2:
        return None

    def at(t):
        for (t0, a0), (t1, a1) in zip(steps, steps[1:]):
            if t0 <= t <= t1:
                return a0 + (a1 - a0) * (t - t0) / (t1 - t0)
        return None

    return at


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("out", type=Path)
    parser.add_argument("--reference", type=Path)
    parser.add_argument("--window-ms", type=int, default=20)
    parser.add_argument("--stack-envelope", action="store_true", help="with --keys: print the template-free stacked energy envelope around those events")
    parser.add_argument("--action", help="only events with this action (key, keydown, keyup)")
    parser.add_argument("--keys", help="comma-separated key names whose events are stacked against --reference")
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
    for t, action, key, frame, *_ in timeline["events"]:
        by_wall = t * seconds / wall_span
        by_frame = (frame - start["frame"]) * seconds / frame_span
        print(f"event {action} {key}: wall {t:.2f}s -> audio {by_wall:.2f}s by wall, {by_frame:.2f}s by frame")
        for label, at in (("wall", by_wall), ("frame", by_frame)):
            index = int(at * 1000 / args.window_ms)
            floor = sorted(env[max(0, index - 20) : index])[len(env[max(0, index - 20) : index]) // 2] if index else 0
            rise = next((i for i in range(index, min(len(env), index + 100)) if env[i] > 1.5 * floor + 200), None)
            if rise is not None:
                print(f"   first rise after the {label}-mapped time: +{(rise - index) * args.window_ms / 1000:.2f}s")
    if args.stack_envelope and args.keys:
        clock = emulated_clock(timeline, rate)
        wanted = set(args.keys.split(","))
        times = [clock(e[0]) for e in timeline["events"] if e[2] in wanted and (not args.action or e[1] == args.action) and clock]
        times = [t for t in times if t is not None]
        stack = stacked_envelope(samples, rate, times)
        if stack:
            peak = max(range(len(stack)), key=lambda k: stack[k])
            print(f"{len(times)} events; stacked envelope peak {stack[peak]:.2f}x at {-0.5 + peak * 0.01:+.2f}s from the event")
            print([(round(-0.5 + k * 0.01, 2), round(v, 2)) for k, v in enumerate(stack) if k % 5 == 0])
    if args.reference:
        reference = load(args.reference)
        print(f"reference {args.reference.name}: {len(reference[0]) / reference[1]:.3f}s")
        if args.keys:
            wanted = set(args.keys.split(","))
            first = start.get("wav_bytes", 44)
            clock = emulated_clock(timeline, rate)
            times = [
                clock(event[0]) if clock else (event[4] - 44) / (rate * 4)
                for event in timeline["events"]
                if len(event) > 4 and event[2] in wanted
            ]
            times = [t for t in times if t is not None]
            print("event emulated times (from the recording's size at each key):", [round(t, 3) for t in times])
            print(stacked_response((samples, rate), reference, times))
        else:
            for score, at in best_matches((samples, rate), reference):
                print(f"   match {score:.3f} at {at:.2f}s")


main()
