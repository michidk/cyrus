#!/usr/bin/env python3
"""Validate installed command/actual source ownership and paced native mixer PCM."""
import argparse
import array
import json
from pathlib import Path


def validate(rows):
    assert rows[-1]['kind'] == 'complete', 'unfinished runtime is not acceptance'
    commands = [row for row in rows if row['kind'] == 'command']
    plays = [row for row in commands if row['sound'].startswith('Play')]
    assert len(plays) == 2 and 'effect: 22,' in plays[0]['sound'] and 'effect: 21,' in plays[1]['sound']
    assert [row['frame'] for row in plays] == [6, 6], 'installed sound command timing changed'
    ends = [row for row in rows if row['kind'] == 'end']
    assert len(ends) == 2 and all(row['frame'] == 12 for row in ends)
    ownership = [row for row in rows if row['kind'] == 'ownership']
    admissions = []
    seen = set()
    for row in ownership:
        slots = row['ownership']['samples']['slots']
        assert len(slots) == 16
        for slot, sample in enumerate(slots):
            if sample is not None and sample['token'] not in seen:
                seen.add(sample['token'])
                admissions.append(dict(row, slot=slot, sample=sample))
    assert [row['sample']['effect'] for row in admissions] == [22, 21, 40], 'missing/duplicate/stale source'
    assert [row['slot'] for row in admissions] == [0, 0, 0]
    assert [row['sample']['owner']['Script']['index'] for row in admissions] == [0, 0, 1]
    assert [row['sample']['owner']['Script']['generation'] for row in admissions] == [1, 1, 1]
    assert [row['phase'] for row in admissions] == [1, 3, 3]
    assert not any(row['phase'] == 1 and row['frame'] > 6
                   and row['ownership']['samples']['slots'][0] is None for row in ownership), \
        'draw source completed before its explicit EndAnimation stop'
    for row in admissions[:2]:
        assert len(row['ownership']['animation']) == 1 and row['ownership']['animation'][0][1] == 0
    replacement = admissions[-1]
    assert replacement['ownership']['animation'] == [], 'completed sample still owns reused slot'
    assert replacement['frame'] < ends[1]['frame'], 'replacement must be admitted before sheath EndAnimation'
    assert any(row['kind'] == 'replacement_survived' for row in rows)
    return admissions, ends


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('log', type=Path)
    parser.add_argument('pcm', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('--offset', type=float, default=0.0)
    args = parser.parse_args()
    rows = [json.loads(line) for line in args.log.read_text().splitlines() if line.startswith('{')]
    admissions, ends = validate(rows)
    samples = array.array('h', args.pcm.read_bytes())
    assert len(samples) % 2 == 0
    windows = []
    def check(name, start, end, present):
        window = samples[int((start + args.offset) * 48000) * 2:int((end + args.offset) * 48000) * 2]
        assert len(window) >= 4800, 'truncated actual output window'
        energy = sum(value*value for value in window) / len(window)
        assert (energy > 1) if present else (energy == 0), (name, energy)
        windows.append(dict(name=name, start=start, end=end, mean_square=energy, present=present))
    for name, row in zip(('draw', 'sheath'), admissions[:2]):
        check(name, row['seconds']+.12, row['seconds']+.3, True)
    check('between-transitions', ends[0]['seconds']+.5, ends[0]['seconds']+1.0, False)
    stopped = next(row for row in rows if row['kind'] == 'replacement_survived')['seconds']
    check('replacement-after-end', ends[1]['seconds']+.5, stopped-.2, True)
    check('final-stop', stopped+.5, stopped+1.0, False)
    args.output.write_text(json.dumps(dict(rows=rows, windows=windows), indent=2)+'\n')


if __name__ == '__main__':
    main()
