#!/usr/bin/env python3
"""Bind a PCM onset to observed game-clock bounds, without wall interpolation.

Input samples must come from the same recording epoch. Polling does not reveal
an exact update boundary; retain its enclosing game-clock interval instead.
"""
import math


def bind(samples, onset):
    if not math.isfinite(onset) or onset < 0 or len(samples) < 2:
        raise ValueError('onset requires at least two observed clock samples')
    previous = None
    for sample in samples:
        pcm, game = sample['pcm_seconds'], sample['game_seconds']
        if not all(math.isfinite(x) and x >= 0 for x in (pcm, game)):
            raise ValueError('clock samples must be finite nonnegative seconds')
        if previous and (pcm <= previous['pcm_seconds'] or game < previous['game_seconds']):
            raise ValueError('duplicate PCM epoch or game clock reset')
        previous = sample
    for sample in samples:
        if sample['pcm_seconds'] == onset:
            return [sample['game_seconds']] * 2
    for before, after in zip(samples, samples[1:]):
        if before['pcm_seconds'] < onset < after['pcm_seconds']:
            return [before['game_seconds'], after['game_seconds']]
    raise ValueError('cue onset is not bracketed by observed game clock')
