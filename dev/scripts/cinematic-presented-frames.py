#!/usr/bin/env python3
"""Identify compositor pixels against an explicit installed movie export.

No published movie status is used as a presentation identity. The tiled
raster oracle follows the documented movie presentation contract. Every RGBA byte
must match; ambiguous images retain all matching frame numbers.
"""
import argparse
from bisect import bisect_left
import hashlib
import json
import math
import importlib.util
from pathlib import Path
from PIL import Image


def presentation_order(surfaces):
    """Order expected-display TimeTicks, preserving asynchronous encoding arrival.

    Chromium PageHandler records VideoFrame reference_time, then encodes on its
    thread pool. CDP event arrival is encoding completion, not display order.
    These are expected-display clocks, not proof of physical display scanout.
    """
    clocks, reversals, ids = set(), [], set()
    previous = None
    for surface in surfaces:
        clock = surface['metadata'].get('monotonicTimestamp')
        if not isinstance(clock, (int, float)) or not math.isfinite(clock):
            raise ValueError('missing compositor swap clock')
        if clock in clocks or surface['id'] in ids:
            raise ValueError('ambiguous duplicate presentation identity')
        clocks.add(clock)
        ids.add(surface['id'])
        if previous and clock < previous['metadata']['monotonicTimestamp']:
            reversals.append({'previous_id': previous['id'], 'id': surface['id'],
                              'previous_seconds': previous['metadata']['monotonicTimestamp'],
                              'seconds': clock})
        previous = surface
    return sorted(surfaces, key=lambda s: s['metadata']['monotonicTimestamp']), reversals


def compose(image, metadata, frame, subtitles, scanlines, padding_colour):
    """Compose a decoded frame using the independent original raster oracle."""
    if image.size != (640, 240):
        raise ValueError('movie oracle requires an unscaled decoded frame')
    spec = importlib.util.spec_from_file_location('raster', Path(__file__).with_name('cinematic-original-raster.py'))
    raster = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(raster)
    image = raster.compose(image, padding_colour, raster.movie_draws())
    if scanlines:
        pixels = image.load()
        for y in range(1, 480, 2):
            for x in range(640):
                pixels[x, y] = (0, 0, 0, 255)
    return raster.compose_subtitles(image, metadata, frame, metadata['subtitle_palette'], subtitles)


def subtitle_pixels(metadata, frame, subtitles):
    """Yield installed glyph pixels in shadow/main submission order, unclipped."""
    subtitle = next((s for s in metadata['definition']['subtitles']
                     if s['start'] <= frame < s['stop'] and (subtitles or s['always'])), None)
    if subtitle is None:
        return
    font = metadata['font']
    def glyph(ch):
        index = ch - font['start']
        g = font['glyphs'][index] if 0 <= index < len(font['glyphs']) else None
        return g if g and g['enabled'] else None
    def advance(ch):
        g = glyph(ch)
        return g['width'] + 1 + ((glyph(33) or {}).get('width', 0) if ch == 32 else 0) if g else 0
    text = subtitle['text'].encode('utf8')
    lines, start, search = [], 0, 35
    while search + 1 < len(text):
        if text[search] == 32:
            lines.append(text[start:search])
            start = search + 1
            search += 35
        search += 1
    lines.append(text[start:])
    for row, line in enumerate(lines):
        pen = 320 - sum(advance(ch) for ch in line) // 2
        y = 470 - (len(lines) - row) * font['line_height']
        for ch in line:
            g = glyph(ch)
            if g:
                for offset, alpha in [(2, 140), (0, 180)]:
                    for i, index in enumerate(g['pixels']):
                        x = pen + offset + g['offset'][0] + i % max(1, g['width'])
                        gy = y + offset + g['offset'][1] + i // max(1, g['width'])
                        if index:
                            yield x, gy, index, offset != 0, alpha
            pen += advance(ch)


def identify(surfaces, images, references):
    lookup = {}
    for number, image in references.items():
        digest = hashlib.sha256(image.convert('RGBA').tobytes()).hexdigest()
        lookup.setdefault(digest, []).append(number)
    result = []
    ordered, _ = presentation_order(surfaces)
    arrivals = {surface['id']: i for i, surface in enumerate(surfaces)}
    for sequence, surface in enumerate(ordered):
        clock = surface['metadata']['monotonicTimestamp']
        image = images[surface['id']].convert('RGBA')
        if image.size != (640, 480):
            raise ValueError('compositor viewport differs from movie reference')
        digest = hashlib.sha256(image.tobytes()).hexdigest()
        result.append({'id': surface['id'], 'expected_display_seconds': clock, 'presentation_sequence': sequence,
                       'arrival_sequence': arrivals[surface['id']], 'raw_metadata': surface['metadata'],
                       'sha256': digest, 'frames': sorted(lookup.get(digest, []))})
    if len({n for row in result for n in row['frames']}) < 2:
        raise ValueError('no progressing installed movie images at compositor swaps')
    return result


def require_endpoint(presentations, minimum):
    if not any(number >= minimum for row in presentations for number in row['frames']):
        raise ValueError('requested installed frame has not reached a compositor swap')


def align_movie_clock(presentations, trace, reads, frame_micros):
    """Retain bracketing production clock returns in Chromium's TimeTicks domain.

    This measures picture age relative to the sampled movie clock. It does not
    equate an expected-display reference with a physical audio-device cursor.
    """
    marks = {}
    for event in trace:
        if event.get('name') != 'TimeStamp':
            continue
        message = event.get('args', {}).get('data', {}).get('message', '')
        if not message.startswith(('movie-time-begin:', 'movie-time-end:')):
            continue
        if message in marks:
            raise ValueError('duplicate production movie-clock boundary')
        marks[message] = event['ts'] / 1_000_000
    intervals = []
    for index, read in enumerate(reads):
        begin = marks.get(f'movie-time-begin:{index}')
        end = marks.get(f'movie-time-end:{index}')
        if begin is not None and end is not None:
            if end < begin:
                raise ValueError('reversed production movie-clock call')
            if intervals and intervals[-1][1] > begin:
                raise ValueError('overlapping production movie-clock calls')
            intervals.append((begin, end, read['value']))
    if not intervals:
        raise ValueError('no bounded production movie-clock returns')
    begins = [row[0] for row in intervals]
    result = []
    duration = frame_micros / 1_000_000
    for picture in presentations:
        if not picture['frames']:
            continue
        clock = picture['expected_display_seconds']
        after = bisect_left(begins, clock)
        before = after - 1
        while before >= 0 and intervals[before][1] > clock:
            before -= 1
        if before < 0 or after == len(intervals):
            continue
        low, high = intervals[before], intervals[after]
        if high[2] < low[2]:
            # A seek crossed this bound. Never interpolate across generations.
            continue
        result.append({'id': picture['id'], 'frames': picture['frames'],
                       'expected_display_seconds': clock,
                       'clock_call_before': low, 'clock_call_after': high,
                       'picture_age_seconds': [low[2] - max(picture['frames']) * duration,
                                               high[2] - (min(picture['frames']) - 1) * duration]})
    if not result:
        raise ValueError('no identified picture bracketed by production movie-clock calls')
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('capture', type=Path)
    parser.add_argument('installed_window', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('--minimum-frame', type=int)
    parser.add_argument('--align', action='store_true', help='require retained trace and production clock returns')
    args = parser.parse_args()
    window = json.loads((args.installed_window / 'window.json').read_text())
    composition = json.loads((args.capture / 'movie-composition.json').read_text())
    metadata = composition['metadata']
    values = composition['preferences']['values']
    if any(metadata[key] != window['metadata'][key] for key in ('frames', 'frame_micros')):
        raise ValueError('installed movie composition differs from exported reference metadata')
    references = {}
    for frame in window['frames']:
        data = (args.installed_window / frame['picture']).read_bytes()
        image = Image.frombytes('RGBA', (640, 240), data)
        references[frame['number']] = compose(image, metadata, frame['number'], values['504'] != 0,
                                             values['505'] != 0, frame['padding_colour'])
    surfaces = json.loads((args.capture / 'surfaces.json').read_text())
    images = {f['id']: Image.open(args.capture / f"surface-{f['id']}.png") for f in surfaces}
    result = identify(surfaces, images, references)
    if args.minimum_frame is not None:
        require_endpoint(result, args.minimum_frame)
    alignment = None
    if args.align:
        trace = json.loads((args.capture / 'trace.json').read_text())
        generation = json.loads((args.capture / 'generation.json').read_text())
        alignment = align_movie_clock(result, trace, generation['graph']['returnedClockReads'], window['metadata']['frame_micros'])
    with args.output.open('x') as output:
        output.write(json.dumps({'frame_micros': window['metadata']['frame_micros'],
                                 'clock_domain': 'Chromium TimeTicks seconds, expected display reference_time',
                                 'encoding_arrival_reversals': presentation_order(surfaces)[1],
                                 'composition_sha256': hashlib.sha256((args.capture / 'movie-composition.json').read_bytes()).hexdigest(),
                                 'movie_clock_alignment': alignment,
                                 'presentations': result}, indent=2) + '\n')
    print(json.dumps({'swaps': len(result), 'identified': sum(bool(r['frames']) for r in result)}))


if __name__ == '__main__':
    main()
