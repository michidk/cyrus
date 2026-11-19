#!/usr/bin/env python3
"""Fill missing original-game definition/presentation files in a writable run copy."""
import argparse
import os
from pathlib import Path
import shutil


def fill_missing(source, destination):
    if destination.is_symlink():
        raise ValueError(f'original run recovery refuses linked destination: {destination}')
    if source.is_dir():
        destination.mkdir(exist_ok=True)
        for child in source.iterdir():
            fill_missing(child, destination / child.name)
    elif not destination.exists():
        shutil.copy2(source, destination)


def prepare_map_state(run):
    """Keep installed map assets linked, but world-save files local to the run."""
    maps = Path(run) / 'maps'
    if not maps.is_symlink():
        return
    source = maps.resolve(strict=True)
    if not source.is_dir():
        raise ValueError(f'original map asset directory absent: {source}')
    assets = [child for child in source.iterdir() if child.suffix.lower() != '.tsg']
    maps.unlink()
    maps.mkdir()
    for asset in assets:
        (maps / asset.name).symlink_to(os.path.relpath(asset, maps))


def prepare(install, run):
    install, run = Path(install), Path(run)
    if install.resolve() == run.resolve():
        raise ValueError('original run recovery requires a separate writable copy')
    if not (install / 'soup386/SOUP386.DEF').is_file():
        raise ValueError(f'original install definition absent: {install / "soup386/SOUP386.DEF"}')
    if not (install / 'system').is_dir():
        raise ValueError(f'original install presentation directory absent: {install / "system"}')
    if not (install / 'fonts').is_dir():
        raise ValueError(f'original install font directory absent: {install / "fonts"}')
    if not run.is_dir():
        raise ValueError(f'original writable run directory absent: {run}')
    prepare_map_state(run)
    for directory in ('soup386', 'system', 'fonts'):
        fill_missing(install / directory, run / directory)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('install', type=Path, help='read-only installed Redguard directory')
    parser.add_argument('run', type=Path, help='existing writable Redguard run directory')
    args = parser.parse_args()
    prepare(args.install, args.run)


if __name__ == '__main__':
    main()
