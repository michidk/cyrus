"""Frame-driven input for the original game's developer console.

At a few frames per second a key tapped for one frame is missed, and the console only accepts typing
while it is the active editor (preservation console.md: a visible console background does not establish that
the command editor is accepting input). This driver therefore holds every key until the original's own
`mainloop_frame` has advanced, opens and closes the console only after the reader's `console_mode`
flag says so, and reports a rejected step with the state it saw instead of carrying on.

The callables are injected so the logic is testable without the game:

    hold(keysym, frames)   press the key, wait for `frames` original frames, release it
    console_mode()         "open" or "closed" from the state reader, None if unreadable
    wait_frames(n)         block until the original has advanced n frames
    shot(tag)              save a screenshot as evidence and return its path
"""
import time

SPECIAL = {" ": "space", ",": "comma", "=": "equal", "-": "minus", ".": "period", "_": "underscore",
           "/": "slash", "'": "apostrophe"}
HOLD_FRAMES = 2
OPEN_BUDGET_FRAMES = 60


class ConsoleError(RuntimeError):
    """A console step did not take effect; the message names the step and the state seen."""


def keysym(char):
    """The xdotool key name for one console character. Input is lowercased by the console itself."""
    char = char.lower()
    if char.isascii() and char.isalnum():
        return char
    if char in SPECIAL:
        return SPECIAL[char]
    raise ConsoleError(f"no key mapping for {char!r}")


def plan(line):
    """The key sequence that types `line`; a semicolon ends console input, so it is refused."""
    if ";" in line:
        raise ConsoleError("a semicolon ends the console input and does not separate commands")
    if len(line) > 100:
        raise ConsoleError("the console edits at most 100 characters per command line")
    return [keysym(char) for char in line]


class Console:
    def __init__(self, hold, console_mode, wait_frames, shot, submit="Return", toggle="F12"):
        self.hold, self.console_mode, self.wait_frames, self.shot = hold, console_mode, wait_frames, shot
        self.submit, self.toggle = submit, toggle
        self.evidence = []

    def _await_mode(self, wanted, step):
        """Wait up to a frame budget for the reader to report `wanted`; raise with what it saw."""
        seen = None
        for _ in range(OPEN_BUDGET_FRAMES // 4):
            seen = self.console_mode()
            if seen == wanted:
                return
            self.wait_frames(4)
        raise ConsoleError(f"{step}: console_mode stayed {seen!r}, wanted {wanted!r}")

    def open(self):
        if self.console_mode() == "open":
            return
        self.hold(self.toggle, HOLD_FRAMES)
        self._await_mode("open", "open the console")

    def close(self):
        if self.console_mode() == "closed":
            return
        self.hold(self.toggle, HOLD_FRAMES)
        self._await_mode("closed", "close the console")

    def run(self, line, tag):
        """Open, type `line` one held key at a time, submit it, keep screenshots of the typed and
        the answered console, and close it again. Returns the screenshot paths."""
        keys = plan(line)
        self.open()
        for name in keys:
            self.hold(name, HOLD_FRAMES)
            self.wait_frames(1)
        typed = self.shot(f"{tag}-typed")
        self.hold(self.submit, HOLD_FRAMES)
        self.wait_frames(8)
        answered = self.shot(f"{tag}-answered")
        self.close()
        self.evidence.append({"line": line, "tag": tag, "typed": typed, "answered": answered,
                              "keys": len(keys)})
        return typed, answered


def frame_hold(key_down, key_up, current_frame, keysym_name, frames, wall_limit=2.0):
    """Press `keysym_name`, hold it until the original has advanced `frames` frames (or `wall_limit`
    seconds pass), then release it. Returns the frames the key was held for."""
    start = current_frame()
    key_down(keysym_name)
    deadline = time.monotonic() + wall_limit
    held = 0
    while time.monotonic() < deadline:
        now = current_frame()
        if now is not None and start is not None:
            held = now - start
            if held >= frames:
                break
        time.sleep(0.05)
    key_up(keysym_name)
    return held
