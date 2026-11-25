#!/usr/bin/env python3
"""Drive the original game through one scenario, frame by frame, from its own start or save.

Shared helpers for the side-by-side validation: boot the original under DOSBox-X in the retro
container (image `cyrus-retro:dev`) from a private run copy `$CYRUS_MAGIC/retro-run-<name>` (never
the shared read-only game links), stream the state reader, send keys through xdotool and wait on
the original's `mainloop_frame`, never on wall time. The reader and helpers hold executable
addresses and stay in the gitignored `$CYRUS_MAGIC/tools` and `$CYRUS_MAGIC/helpers`.

    CYRUS_MAGIC=~/workspaces/cyrus/magic python3 dev/scripts/original-scenario.py \\
        --name soup3 --world 1 --marker 22 --quicksave --out boot.json
"""
import argparse
from contextlib import ExitStack
from harness import exclusive_lock
import hashlib
import importlib.util
import json
import os
import re
import shutil
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
MAGIC = Path(os.environ.get("CYRUS_MAGIC", ROOT / "magic"))


def load_fight():
    """The frame-driven sampler of original-fight.py, reused rather than copied."""
    spec = importlib.util.spec_from_file_location("original_fight", ROOT / "dev/scripts/original-fight.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def sh(*command, **kwargs):
    return subprocess.run(command, capture_output=True, text=True, **kwargs)


def reload_boundary_ready(raw):
    """Whether a live world can open Load without carrying its startup fade into the save.

    Fade and clock state are independent: a clear view can still be in clock
    warmup. Missing reader bindings reject admission instead of assuming zero.
    """
    if not raw or raw.get("world") in (None, -1, (1 << 32) - 1):
        return False
    counters = raw.get("counters") or {}
    if not counters.get("mainloop_frame") or not any(a.get("selected") for a in raw.get("actors", [])):
        return False
    fade = counters.get("world_fade")
    warmup = (counters.get("clock_continuation") or {}).get("warmup")
    if type(fade) is not int or type(warmup) is not int:
        raise ValueError("original reload requires fade and clock warmup reader bindings")
    return fade == 0 and warmup == 0


class Original:
    """One original-game container with a streaming sampler and key input."""

    def __init__(self, name, world, marker, interval=0.2, quicksave_enabled=False, stage=None):
        self.fight = load_fight()
        image = subprocess.check_output(
            [str(ROOT / "dev/scripts/ensure-retro-runtime.sh")], text=True).strip()
        self.name = f"retro-{name}"
        self.run = MAGIC / f"retro-run-{name}"
        self.run_lock = ExitStack()
        self.run_lock.enter_context(exclusive_lock(self.run.with_suffix(".lock")))
        reader = MAGIC / "tools/original-state.py"
        if not reader.exists():
            sys.exit(f"{reader} is missing; restore the local original-game tooling bundle")
        if not self.run.exists():
            shutil.copytree(MAGIC / "retro-run", self.run, symlinks=True,
                            ignore=shutil.ignore_patterns("*.png", "*.wav"))
        maps = self.run / "Redguard/maps"
        if maps.is_symlink() or not maps.resolve().is_relative_to(self.run.resolve()):
            raise ValueError(f"{maps}: original needs run-local writable maps")
        ini = self.run / "Redguard/WORLD.INI"
        text = ini.read_text(errors="surrogateescape")
        text = re.sub(r"(?m)^start_world=\d+", f"start_world={world}", text)
        text = re.sub(r"(?m)^start_marker=\d+", f"start_marker={marker}", text)
        ini.write_text(text, errors="surrogateescape")
        if quicksave_enabled:
            enable_quicksave(self.run)
        if stage is not None:
            stage(self.run)
        sh("docker", "rm", "-f", self.name)
        sh("docker", "run", "--rm", "--entrypoint", "find", "-v", f"{self.run}:/game", image,
           "/game/Redguard/maps", "-iname", "*.tsg", "-delete", check=True)
        sh("docker", "run", "-d", "--rm", "--name", self.name, "--cap-add", "SYS_PTRACE",
           "-v", f"{self.run}:/game", "-v", f"{MAGIC}/gog:/gog:ro",
           "-v", f"{MAGIC}/gog/extracted/app/game.gog:/game/game.gog:ro", image, check=True)
        sh("docker", "cp", str(reader), f"{self.name}:/tmp/original-state.py", check=True)
        sh("docker", "cp", str(ROOT / "dev/scripts/original-memory-region.py"),
           f"{self.name}:/tmp/original-memory-region.py", check=True)
        self.sampler = self.fight.Sampler(self.name, interval)

    def key(self, *args):
        return sh("docker", "exec", "-e", "DISPLAY=:1", self.name, "xdotool", *args)

    def hold(self, key, seconds):
        """A one-frame press is missed at a few frames per second: hold for wall time."""
        self.key("keydown", key)
        time.sleep(seconds)
        self.key("keyup", key)

    def raw(self):
        return self.sampler.get()[1]

    def frame(self):
        raw = self.raw()
        return (raw or {}).get("counters", {}).get("mainloop_frame")

    def player(self):
        raw = self.raw() or {}
        return next((a for a in raw.get("actors", []) if a["selected"]), None)

    def alive(self):
        """Whether the container is still running."""
        return bool(sh("docker", "ps", "-q", "--filter", f"name=^{self.name}$").stdout.strip())

    def screenshot(self, path):
        """Capture the display (see capture_screen); the result keeps every attempt's status and stderr."""
        def run(argv):
            done = sh("docker", "exec", "-e", "DISPLAY=:1", self.name, *argv)
            return done.returncode, done.stderr

        def copy(source, destination):
            return sh("docker", "cp", f"{self.name}:{source}", destination).returncode

        return capture_screen(run, copy, path)

    def book_save(self, description, shots):
        """Save through the book (see book_save_keys); returns (new SAVEGAME entries, evidence). A
        screenshot must succeed after every step, else the run stops there: evidence is never lost
        silently."""
        pages = menu_pages((self.run / "Redguard/MENU.INI").read_text(errors="replace"))
        before = {p.name for p in (self.run / "Redguard/SAVEGAME").iterdir()}
        evidence = []
        for number, (action, argument, settle) in enumerate(book_save_keys(description, pages)):
            if action == "hold":
                self.hold(argument, 0.4)
            else:
                self.key("type", "--delay", "120", argument)
            time.sleep(settle)
            shot = self.screenshot(Path(shots) / f"book-{number}-{action}-{argument[:12]}.png")
            evidence.append({"step": number, "action": action, "argument": argument, "shot": shot})
            failure = book_step_failure(shot, self.alive())
            if failure:
                raise RuntimeError(f"book step {number} ({action} {argument}): {failure}")
        after = {p.name for p in (self.run / "Redguard/SAVEGAME").iterdir()}
        return sorted(after - before), evidence

    def wait_or_stall(self, frames, window=5.0, timeout=900):
        """Frame wait that reports a frozen book instead of timing out (see counter_stalled)."""
        return wait_frames_or_stall(self.frame, frames, window=window, timeout=timeout)

    def wait(self, frames, timeout=900, until=None):
        return self.fight.wait_frames(self.sampler, frames, timeout, until=until)

    def wait_reload_boundary(self, frames=240, timeout=180):
        """Finish the current world's fade and warmup before staging a shared-save reload.

        This observes ordinary progress; it never resets a counter or modifies the
        saved continuation. The reader must bind both fade and clock warmup.
        """
        return self.wait(frames, timeout, until=lambda _: reload_boundary_ready(self.raw()))

    def capture_quicksave(self, destination, timeout=180):
        """Publish a private copy after F9's slot changes and survives two game frames.

        Enable final_version before boot. This stages a save, not tape zero:
        the original backend must reload it before the paired recording starts.
        """
        slot = self.run / 'Redguard/SAVEGAME/SAVEGAME.000'
        source = slot / 'SAVEGAME.SAV'
        if slot.is_symlink() or not slot.resolve().is_relative_to(self.run.resolve()):
            raise ValueError(f'{slot}: quick-save must be run-local')
        if any(path.is_symlink() for path in slot.rglob('*')):
            raise ValueError(f'{slot}: linked save content is unsupported')
        previous = source.read_bytes() if source.exists() else None
        self.key('key', '--delay', '200', 'F9').check_returncode()
        deadline, stable, receipt = time.monotonic() + timeout, None, None
        while time.monotonic() < deadline:
            frame = self.frame()
            current = source.read_bytes() if source.exists() else None
            if current and current != previous and type(frame) is int:
                digest = hashlib.sha256(current).hexdigest()
                if stable and stable[0] == digest and frame >= stable[1] + 2:
                    receipt = dict(frame=frame, sha256=digest)
                    break
                if stable is None or stable[0] != digest:
                    stable = digest, frame
            time.sleep(.02)
        if receipt is None:
            raise TimeoutError(f'{source}: F9 did not publish a new stable quick-save')
        if any(path.is_symlink() for path in slot.rglob('*')):
            raise ValueError(f'{slot}: linked save content is unsupported')
        shutil.copytree(slot, destination)
        if hashlib.sha256((Path(destination) / 'SAVEGAME.SAV').read_bytes()).hexdigest() != receipt['sha256']:
            raise RuntimeError('original quick-save changed during copy')
        return receipt

    def stop(self):
        self.sampler.stop()
        sh("docker", "rm", "-f", self.name)
        self.run_lock.close()


def enable_quicksave(run):
    """Enable the documented F9 consumer in the private original configuration."""
    path = Path(run) / 'Redguard/SYSTEM.INI'
    if path.is_symlink() or not path.resolve().is_relative_to(Path(run).resolve()):
        raise ValueError(f'{path}: configuration must be run-local')
    text, count = re.subn(r'(?m)^final_version=\d+', 'final_version=1', path.read_text())
    if count != 1:
        raise ValueError(f'{path}: expected one final_version setting')
    path.write_text(text)


def project_prompt(raw):
    """The comparison schema's `prompts` entry from one reader sample: the pending activation
    prompt text (None when nothing is pending), the owner actor's id and the hold-off timer, or
    None when the reader carries no prompt state. The addresses live in the gitignored reader;
    this only shapes what it reports so cyrus' `activation` state can be compared per frame."""
    prompts = (raw or {}).get("prompts")
    if not prompts:
        return None
    return {"text": prompts.get("pending_text") or None, "owner_id": prompts.get("owner_id"),
            "holdoff": prompts.get("holdoff")}


def prompt_windows(rows):
    """Prompt windows over a frame-ordered series of {"frame", "text"} rows: one
    {"text", "first_frame", "last_frame", "frames"} per run of equal non-empty text, with frames
    counted inclusively as sampled (a coarser sampler widens a window by one sample at most)."""
    windows, current = [], None
    for row in rows:
        text = row.get("text")
        if current and text == current["text"]:
            current["last_frame"] = row["frame"]
        else:
            if current:
                windows.append(current)
            current = {"text": text, "first_frame": row["frame"], "last_frame": row["frame"]} if text else None
    if current:
        windows.append(current)
    for window in windows:
        window["frames"] = window["last_frame"] - window["first_frame"] + 1
    return windows


def menu_pages(text):
    """Pages of a MENU.INI document as {page: {item: {"default", "action", "text"}}} (only the keys
    the book walk needs). The installed file defines the book, so the walk reads it instead of
    assuming positions; an item with `default[n] = 1` is the one Return activates first."""
    pages, page = {}, None
    for line in text.splitlines():
        line = line.split(";", 1)[0].strip()
        header = re.fullmatch(r"\[page(\d+)\]", line)
        if header:
            page = pages.setdefault(int(header.group(1)), {})
            continue
        field = re.fullmatch(r"(text|default|action)\[(\d+)\]\s*=\s*(.*)", line)
        if page is not None and field:
            item = page.setdefault(int(field.group(2)), {})
            item[field.group(1)] = field.group(3).strip()
    return pages


def default_action(pages, page):
    """The action number of a page's default item, or None."""
    for number in sorted(pages.get(page, {})):
        item = pages[page][number]
        if item.get("default") == "1" and "action" in item:
            return int(item["action"])
    return None


def book_save_keys(description, pages):
    """The (xdotool action, argument, settle seconds) steps that save through the book from a live
    world, derived from the installed MENU.INI: Escape opens page 0, Return activates its default
    item, which must be action 1 (open Save page 1); Return on page 1's default item, which must be
    action 101 (new save, ask for a description); the description (1-64 ASCII characters, uppercased
    by the input; preservation menu.md) and Return confirm it. F2 is not used: it only works with
    `[debug] final_version=1`. The book freezes the frame counter, so every step is paced by wall time."""
    text = description.strip()
    if not text or len(text) > 64 or not text.isascii():
        raise ValueError("a save description is 1-64 ASCII characters")
    if default_action(pages, 0) != 1:
        raise ValueError("MENU.INI page 0 default item is not the Save action (1)")
    if default_action(pages, 1) != 101:
        raise ValueError("MENU.INI page 1 default item is not the new-save action (101)")
    return [("hold", "Escape", 6.0), ("hold", "Return", 6.0), ("hold", "Return", 6.0),
            ("type", text, 3.0), ("hold", "Return", 8.0)]


def selectable_actions(pages, page):
    """The action numbers of a MENU.INI page's selectable items in item order (an item without an
    `action` is skipped: it is a label)."""
    items = pages.get(page, {})
    return [int(items[n]["action"]) for n in sorted(items) if "action" in items[n]]


def book_load_keys(pages, slot_row, disabled=(1,)):
    """The (xdotool action, argument, settle seconds) steps that load an existing save from the
    retail startup book, derived from MENU.INI. At startup the book's Save action (1) is disabled and
    the selection starts on the first enabled item after the default (preservation menu.md), so the
    walk counts Down presses from there to the Load action (2), activates it, then on page 2 counts
    `slot_row` Down presses (rows are ordered newest save first) and activates the row (action 202).
    The book freezes the frame counter, so every step is paced by wall time."""
    actions = selectable_actions(pages, 0)
    if 2 not in actions:
        raise ValueError("MENU.INI page 0 has no Load action (2)")
    if default_action(pages, 0) is None or default_action(pages, 0) not in actions:
        raise ValueError("MENU.INI page 0 has no default item")
    enabled = [a for a in actions if a not in disabled]
    start = enabled[0] if default_action(pages, 0) in disabled else default_action(pages, 0)
    downs = enabled.index(2) - enabled.index(start)
    if downs < 0:
        raise ValueError("the Load action precedes the initial selection")
    if 202 not in selectable_actions(pages, 2):
        raise ValueError("MENU.INI page 2 has no load row action (202)")
    if slot_row < 0:
        raise ValueError("slot_row is a nonnegative row index")
    steps = [("hold", "Down", 2.0)] * downs + [("hold", "Return", 8.0)]
    steps += [("hold", "Down", 2.0)] * slot_row + [("hold", "Return", 15.0)]
    return steps


def stage_retail_save(run, pristine_ini, source_slot, slot_number=0):
    """Prepare a private run copy so the retail startup book offers one existing save: the retail
    SYSTEM.INI (no debug entry, so the book and introduction run as a player sees them) and the
    chosen SAV, journal and per-world TSG sidecars copied into an otherwise empty SAVEGAME directory. Everything stays under `run`;
    nothing is written into the shared game links. Returns the staged slot directory."""
    run = Path(run).resolve()
    ini = run / "Redguard/SYSTEM.INI"
    savegame = run / "Redguard/SAVEGAME"
    for path in (ini, savegame):
        if path.is_symlink() or not path.resolve().is_relative_to(run):
            raise ValueError(f"{path}: must be run-local")
    source = Path(source_slot)
    saved = source / "SAVEGAME.SAV"
    if not saved.is_file():
        raise ValueError(f"{source_slot}: no SAVEGAME.SAV")
    if source.is_symlink() or source.resolve().is_relative_to(savegame.resolve()):
        raise ValueError(f"{source_slot}: checkpoint source must be outside the replaced save directory")
    files = [saved] + sorted(p for p in source.iterdir() if p.suffix.lower() == '.tsg' or p.name.lower() == 'logbook.txt')
    for path in files:
        if path.is_symlink() or not path.is_file():
            raise ValueError(f"{path}: checkpoint file must be regular and run-local")
    shutil.copyfile(pristine_ini, ini)
    for entry in savegame.iterdir():
        shutil.rmtree(entry) if entry.is_dir() else entry.unlink()
    slot = savegame / f"SAVEGAME.{slot_number:03d}"
    slot.mkdir()
    for path in files:
        shutil.copy2(path, slot / path.name)
    return slot


def book_step_failure(shot, container_up):
    """Why a book step produced no evidence, or None when it did: a stopped container means the key
    ended the game itself (in the direct-start debug entry Escape quits DOSBox-X instead of opening
    the book, measured live), which is a different finding from a capture-tool failure and must not
    be reported as one."""
    if shot["ok"]:
        return None
    if not container_up:
        return "the original exited after this key (no book in this entry)"
    return "no valid screenshot: " + json.dumps(shot["attempts"])


def valid_image(path):
    """A PNG file with the PNG signature and more than a header's worth of bytes."""
    try:
        data = Path(path).read_bytes()
    except OSError:
        return False
    return len(data) > 1000 and data.startswith(b"\x89PNG\r\n\x1a\n")


CAPTURE_COMMANDS = (
    ("scrot", ["scrot", "-o", "{out}"]),
    ("import", ["import", "-window", "root", "{out}"]),
    ("xwd", ["sh", "-c", "xwd -root -silent | convert xwd:- {out}"]),
)


def capture_screen(run, copy, destination):
    """Capture the display into `destination` and return {"ok", "method", "attempts"}; `attempts`
    keeps each method's status and stderr, so a failure names what failed. `run(argv)` runs a command
    inside the container and returns (status, stderr); `copy(src, dst)` returns the copy status.
    Success requires a valid nonempty PNG at `destination`: a zero exit status is not enough."""
    attempts = []
    for method, template in CAPTURE_COMMANDS:
        argv = [part.replace("{out}", "/tmp/shot.png") for part in template]
        status, stderr = run(argv)
        entry = {"method": method, "status": status, "stderr": stderr[-300:]}
        attempts.append(entry)
        if status != 0:
            continue
        entry["copy"] = copy("/tmp/shot.png", str(destination))
        if entry["copy"] == 0 and valid_image(destination):
            return {"ok": True, "method": method, "attempts": attempts}
        entry["invalid_image"] = True
    return {"ok": False, "method": None, "attempts": attempts}


def counter_stalled(samples, window):
    """Whether the frame counter stood still over the last `window` seconds of
    (wall_seconds, frame) samples. The original's book stops the main loop (and its
    mainloop_frame) while it is open, so a frame wait inside it can never finish: callers use
    this to tell a frozen book from a slow game and switch to wall-time pacing."""
    if not samples:
        return False
    newest = samples[-1][0]
    if newest - samples[0][0] < window:
        return False
    recent = [frame for when, frame in samples if newest - when <= window]
    return len(set(recent)) == 1


def wait_frames_or_stall(read_frame, frames, window=5.0, poll=0.25, timeout=900, clock=time.monotonic,
                         sleep=time.sleep):
    """Wait for the frame counter to advance by `frames`; return ("advanced", frame) or, when the
    counter stands still for `window` seconds (a frozen book), ("stalled", frame) instead of
    timing out. `read_frame` returns the current frame or None before the first sample."""
    start = None
    samples = []
    deadline = clock() + timeout
    while clock() < deadline:
        frame = read_frame()
        now = clock()
        if frame is not None:
            if start is None:
                start = frame
            samples.append((now, frame))
            samples = [row for row in samples if now - row[0] <= window * 2]
            if frame - start >= frames:
                return "advanced", frame
            if counter_stalled(samples, window):
                return "stalled", frame
        sleep(poll)
    raise TimeoutError(f"no {frames} frames within {timeout} s")


class FrameSession:
    """Comparison adapter around the streaming original reader and real key input.

    The caller prepares matching state first and supplies project(raw), which maps the
    local reader to the comparison schema without hiding unmeasured fields. Polling can
    miss a frame: the paired runner rejects that observation rather than interpolating it.
    """

    KEYS = {"ControlLeft": "Control_L", "ShiftLeft": "Shift_L", "AltLeft": "Alt_L",
            "ArrowUp": "Up", "ArrowDown": "Down", "ArrowLeft": "Left", "ArrowRight": "Right",
            "Space": "space", "Enter": "Return", "Escape": "Escape"}

    def __init__(self, game, project):
        self.game, self.project = game, project
        self.origin = game.frame()
        if self.origin is None:
            raise ValueError("original comparison requires a live frame counter")

    def snapshot(self):
        raw = self.game.raw()
        frame = (raw or {}).get("counters", {}).get("mainloop_frame")
        if type(frame) is not int:
            raise ValueError("original comparison frame is unavailable")
        return dict(self.project(raw), frame=frame - self.origin)

    def advance_to(self, frame):
        current = self.game.frame()
        if current is None:
            raise ValueError("original comparison frame is unavailable")
        remaining = frame - (current - self.origin)
        if remaining < 0:
            raise ValueError(f"original frame {frame} already passed")
        target = self.origin + frame
        self.game.wait(0, until=lambda sample: sample["frame"] >= target)

    def input(self, key, action):
        if key not in self.KEYS or action not in ("press", "release"):
            raise ValueError(f"unsupported original input {key}/{action}")
        self.game.key("keydown" if action == "press" else "keyup", self.KEYS[key]).check_returncode()
        return self.game.frame() - self.origin


def skip_retail_introduction(game, wait, raw, checkpoints):
    """Acknowledge each installed skip and the following released-key movie frame."""
    while (raw.get("startup") or {}).get("movie_active"):
        frame = raw["startup"].get("movie_frame")
        if type(frame) is not int:
            raise ValueError("retail Introduction requires movie_frame reader binding")
        target = next((value - 1 for value in checkpoints if value - 1 > frame), None)
        game.hold("Escape", 0.4)
        raw = wait("movie skip", lambda r: (r.get("startup") or {}).get("movie_active") is False
                   or (target is not None and ((r.get("startup") or {}).get("movie_frame") or -1) >= target))
        if raw["startup"].get("movie_active"):
            released_at = raw["startup"]["movie_frame"]
            raw = wait("movie release", lambda r: (r.get("startup") or {}).get("movie_active") is False
                       or ((r.get("startup") or {}).get("movie_frame") or 0) > released_at)
    return raw


def enter_retail_world(game, timeout=180, poll=0.1, clock=time.monotonic, sleep=time.sleep):
    """Reach the retail world through acknowledged Introduction checkpoints.

    Startup has no gameplay frames. Reader startup bindings identify movie
    playback; only a selected player and advancing frames admit a world.
    The timeout bounds each transition, never supplies simulated gameplay time.
    """
    events = []

    def wait(label, predicate):
        deadline = clock() + timeout
        while clock() < deadline:
            raw = game.raw() or {}
            if predicate(raw):
                events.append({"transition": label, "frame": (raw.get("counters") or {}).get("mainloop_frame"),
                               "world": raw.get("world"), "clock_scale": (raw.get("counters") or {}).get("clock_scale"),
                               "startup": raw.get("startup")})
                print(json.dumps(events[-1]), file=sys.stderr, flush=True)
                return raw
            if not game.alive():
                raise RuntimeError(f"retail {label}: original exited")
            sleep(poll)
        raise TimeoutError(f"retail {label}: no acknowledgement; last state {raw.get('startup')}")

    def live(raw):
        return raw.get("world") not in (None, -1, (1 << 32) - 1) and any(
            actor.get("selected") for actor in raw.get("actors", []))

    # An Escape skips to the next installed movie checkpoint, not straight to the book.
    text = (game.run / "Redguard/MENU.INI").read_text(errors="replace")
    match = re.search(r"(?im)^\s*movie_keys\[0\]\s*=\s*([\d, ]+)", text)
    if match is None:
        raise ValueError("MENU.INI: missing Introduction movie checkpoints")
    checkpoints = [int(value) for value in match[1].split(",") if int(value)]
    raw = wait("startup", lambda r: ((r.get("startup") or {}).get("movie_active") is True
                                      and type((r.get("startup") or {}).get("movie_frame")) is int) or live(r))
    skip_retail_introduction(game, wait, raw, checkpoints)
    raw = wait("settled retail world", lambda r: live(r) and reload_boundary_ready(r))
    world = raw["world"]
    frame = (raw.get("counters") or {}).get("mainloop_frame", 0)
    scale = (raw.get("counters") or {}).get("clock_scale")
    if type(scale) is not int:
        raise ValueError("retail world requires clock_scale reader binding")
    raw = wait("world progress", lambda r: live(r) and r["world"] == world
               and (r.get("counters") or {}).get("mainloop_frame", 0) > frame
               and (r.get("counters") or {}).get("clock_scale", scale) > scale)
    return {"events": events, "state": raw}


def open_retail_load(game, timeout=60, poll=0.1, clock=time.monotonic, sleep=time.sleep):
    """Open the retail Load page from a naturally settled retail world.

    Call enter_retail_world first; a direct debug entry does not initialize the
    retail book. Hold F3 until the reader acknowledges the completed Load page,
    rather than assuming a host key duration contains a gameplay update. Always
    release the shortcut, including on timeout or original exit. The caller then
    selects the staged slot and owns its exact restoration boundary.
    """
    if not reload_boundary_ready(game.raw()):
        raise ValueError('retail Load requires a settled world; use enter_retail_world first')
    pages = menu_pages((game.run / 'Redguard/MENU.INI').read_text(errors='replace'))
    if 202 not in selectable_actions(pages, 2):
        raise ValueError('MENU.INI page 2 has no Load action (202)')
    deadline = clock() + timeout
    raw = {}
    try:
        game.key('keydown', 'F3').check_returncode()
        while clock() < deadline:
            raw = game.raw() or {}
            menu = raw.get('startup') or {}
            if (menu.get('menu_context') == 6 and menu.get('menu_page') == 2
                    and menu.get('menu_busy') is False and menu.get('menu_selection')):
                return raw
            if not game.alive():
                raise RuntimeError('retail Load: original exited before page acknowledgement')
            sleep(poll)
        raise TimeoutError(f"retail Load: no acknowledgement; last state {raw.get('startup')}")
    finally:
        released = game.key('keyup', 'F3')
        if game.alive():
            released.check_returncode()


def observe_retail(game, shots, seconds, interval=5.0):
    """Screenshot the retail entry every `interval` seconds for `seconds`, pressing no key, and
    return one row per shot ({"t", "frame", "world", "shot"}), so the first live run records what the
    retail startup shows (introduction, book or START) instead of assuming it. Stops early when the
    container exits (the row then says so)."""
    rows, start = [], time.monotonic()
    index = 0
    while time.monotonic() - start < seconds:
        raw = game.raw() or {}
        shot = game.screenshot(Path(shots) / f"retail-{index:03d}.png")
        rows.append({"t": round(time.monotonic() - start, 1), "frame": game.frame(), "world": raw.get("world"),
                     "shot": shot["ok"], "alive": game.alive()})
        if not rows[-1]["alive"]:
            break
        index += 1
        time.sleep(interval)
    return rows


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--name", default="soup3")
    parser.add_argument("--world", type=int, default=1)
    parser.add_argument("--marker", type=int, default=0)
    parser.add_argument("--quicksave", action="store_true", help="press F9 once the world is live")
    parser.add_argument("--retail-slot", help="existing slot directory to stage for the retail entry; "
                        "stages it privately and enters the retail world through acknowledged Introduction skips")
    parser.add_argument("--observe-seconds", type=float, default=120)
    parser.add_argument("--out", required=True)
    args = parser.parse_args()
    if args.retail_slot:
        pristine = MAGIC / "retro-pristine/SYSTEM.INI"
        game = Original(args.name, args.world, args.marker,
                        stage=lambda run: stage_retail_save(run, pristine, args.retail_slot))
        try:
            shots = Path(args.out).with_suffix("")
            shots.mkdir(parents=True, exist_ok=True)
            result = enter_retail_world(game, timeout=args.observe_seconds)
            result["screenshot"] = game.screenshot(shots / "retail-world.png")
            Path(args.out).write_text(json.dumps(result, indent=1))
            print(json.dumps(result["events"]))
        finally:
            game.stop()
        return
    game = Original(args.name, args.world, args.marker)
    try:
        game.wait(0, 600, until=lambda s: s["frame"] > 0 and s["ph"])
        game.wait(60, 900)
        before = sorted(p.name for p in (game.run / "Redguard/SAVEGAME").glob("*"))
        if args.quicksave:
            game.key("key", "--delay", "200", "F9")
            game.wait(60, 300)
        after = sorted(p.name for p in (game.run / "Redguard/SAVEGAME").glob("*"))
        player = game.player()
        Path(args.out).write_text(json.dumps({
            "frame": game.frame(), "player": player, "saves_before": before, "saves_after": after}, indent=1))
        print(json.dumps({"frame": game.frame(), "position": player["position"], "yaw": player["yaw"],
                          "saves": after}))
    finally:
        game.stop()


if __name__ == "__main__":
    main()
