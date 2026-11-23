#!/usr/bin/env python3
"""Exact-frame comparison adapter over the original stopped-boundary reader.

The caller owns boot/staging/cleanup and provides Watchpoint plus project(raw).
No polling, interpolation or OS-key acknowledgement establishes acceptance here.
"""


class FrameSession:
    """Keep the game stopped between inputs and read every intervening boundary.

    counter(raw) returns the verified game frame. project(raw) returns normalized
    comparison observables. emit(raw) optionally retains every batched frame read.
    The caller must already have stopped at the initial boundary.
    """

    def __init__(self, boundary, counter, project, emit=lambda raw: None, timeout=10):
        self.boundary, self.counter, self.project = boundary, counter, project
        self.emit, self.timeout = emit, timeout
        self.current = boundary.raw_snapshot()
        self.origin = self._frame(self.current)
        self.emit(self.current)

    def _frame(self, raw):
        frame = self.counter(raw)
        if type(frame) is not int or frame < 0:
            raise ValueError('original frame counter unavailable')
        return frame

    def snapshot(self):
        return dict(self.project(self.current), frame=self._frame(self.current) - self.origin)

    def advance_to(self, frame):
        current = self._frame(self.current) - self.origin
        if type(frame) is not int or frame < current:
            raise ValueError(f'original frame {frame} already passed or invalid')
        while current < frame:
            self.boundary.next_boundary(self.timeout)
            raw = self.boundary.raw_snapshot()
            observed = self._frame(raw) - self.origin
            if observed != current + 1:
                raise ValueError(f'original frame gap: expected {current + 1}, observed {observed}')
            self.current = raw
            self.emit(raw)
            current = observed

    def input(self, key, action):
        before = self._frame(self.current)
        raw = self.boundary.latch_key(key, action)
        if self._frame(raw) != before:
            raise ValueError('original frame changed during key acknowledgement')
        self.current = raw
        return before - self.origin
