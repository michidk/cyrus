"""Frame-driven input for the original game's developer console.

At a few frames per second a key tapped for one frame is missed, and the console only accepts typing
while it is the active editor (preservation console.md: a visible console background does not establish that
the command editor is accepting input). This driver opens and closes the console only after the reader's
`console_mode` flag says so, holds each key long enough to register, and reports a rejected step with the
state it saw instead of carrying on. Frame waits do not work inside the console, because the original's
frame counter stops while it is open, so the console steps use wall time.

The callables are injected so the logic is testable without the game:

    hold(keysym, frames)    open the console: press, wait for `frames` original frames, release
    press(keysym, seconds)  hold a key for wall seconds (used while the console is open)
    console_mode()          "open" or "closed" from the state reader, None if unreadable
    pause(seconds)          sleep
    shot(tag)               save a screenshot as evidence and return its path
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
    """Type into the original's console.

    While the console is open the original's `mainloop_frame` does not advance (measured: it stayed
    at frame 57 for minutes with the console open), so inside the console every wait is wall time:
    `press(keysym, seconds)` holds a key, `pause(seconds)` sleeps, and the open/closed state is polled
    from the reader. Only the toggle that opens the console runs while frames still advance."""

    def __init__(self, hold, press, console_mode, pause, shot, submit="Return", toggle="F12",
                 key_seconds=0.35, poll_seconds=0.25, budget_seconds=15.0):
        self.hold, self.press, self.console_mode, self.pause, self.shot = hold, press, console_mode, pause, shot
        self.submit, self.toggle = submit, toggle
        self.key_seconds, self.poll_seconds, self.budget_seconds = key_seconds, poll_seconds, budget_seconds
        self.evidence = []

    def _await_mode(self, wanted, step):
        """Poll the reader until it reports `wanted`; raise with the last state seen."""
        seen = None
        for _ in range(max(1, int(self.budget_seconds / self.poll_seconds))):
            seen = self.console_mode()
            if seen == wanted:
                return
            self.pause(self.poll_seconds)
        raise ConsoleError(f"{step}: console_mode stayed {seen!r}, wanted {wanted!r}")

    def open(self):
        if self.console_mode() == "open":
            return
        self.hold(self.toggle, HOLD_FRAMES)  # the world is running, so its frame counter advances
        self._await_mode("open", "open the console")

    def close(self):
        if self.console_mode() == "closed":
            return
        self.press(self.toggle, self.key_seconds)  # the counter is frozen while the console is open
        self._await_mode("closed", "close the console")

    def run(self, line, tag):
        """Open, type `line` one held key at a time, submit it, keep screenshots of the typed and
        the answered console, and close it again. Returns the screenshot paths."""
        keys = plan(line)
        self.open()
        for name in keys:
            self.press(name, self.key_seconds)
            self.pause(self.poll_seconds)
        typed = self.shot(f"{tag}-typed")
        self.press(self.submit, self.key_seconds)
        self.pause(1.0)
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
