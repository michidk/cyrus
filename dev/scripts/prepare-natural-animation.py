#!/usr/bin/env python3
"""Prepare a local-only installed draw/sheath fixture from existing asset exports."""
import argparse
import configparser
import hashlib
import json
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('animations', type=Path)
    parser.add_argument('sounds', type=Path)
    parser.add_argument('install', type=Path)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    data = json.loads(args.animations.read_text())
    actor = next(row for row in data['metadata']['actors'] if row['script_name'] == 'CYRUS')
    data.update(metadata={'actors': [actor]}, models={}, sections={}, unsupported={})
    system_path = args.install / 'SYSTEM.INI'
    system = system_path.read_text(errors='strict')
    ini = configparser.ConfigParser(strict=False, allow_no_value=True)
    ini.read_string(system)
    rate = ini.getint('system', 'normal_frame_rate')
    assert rate > 0, 'fixture requires explicit positive installed nominal rate'
    bank_path = args.sounds / 'bank.sounds'
    bank = json.loads(bank_path.read_text())
    effects, sources = [], [args.animations, system_path, bank_path, args.install / 'KEYS.INI']
    for index in (21, 22, 40):
        path = args.sounds / f'{index:03}.wav'
        effects.append(dict(index=index, wav=list(path.read_bytes()), **bank['effects'][index]))
        sources.append(path)
    assert effects[-1]['repetitions'] == 0, 'replacement control requires installed loop'
    assert all(row['repetitions'] == 1 for row in effects[:2]), 'draw/sheath must be one-shots'
    args.output.parent.mkdir(parents=True, exist_ok=True)
    if args.output.exists():
        raise ValueError('preserve prior fixture; choose a fresh output path')
    args.output.write_text(json.dumps(dict(animations=data, effects=effects, system=system, rate=rate)))
    sources.append(args.output)
    manifest = [{'path': str(path.resolve()), 'sha256': hashlib.sha256(path.read_bytes()).hexdigest()}
                for path in sources]
    args.output.with_suffix('.provenance.json').write_text(json.dumps(manifest, indent=2)+'\n')


if __name__ == '__main__':
    main()
