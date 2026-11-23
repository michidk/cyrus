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
import fcntl
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


class Original:
    """One original-game container with a streaming sampler and key input."""

    def __init__(self, name, world, marker, interval=0.2):
        self.fight = load_fight()
        image = subprocess.check_output(
            [str(ROOT / "dev/scripts/ensure-retro-runtime.sh")], text=True).strip()
        self.name = f"retro-{name}"
        self.run = MAGIC / f"retro-run-{name}"
        self.run_lock = self.run.with_suffix(".lock").open("w")
        fcntl.flock(self.run_lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
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

    def wait(self, frames, timeout=900, until=None):
        return self.fight.wait_frames(self.sampler, frames, timeout, until=until)

    def stop(self):
        self.sampler.stop()
        sh("docker", "rm", "-f", self.name)
        self.run_lock.close()


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


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--name", default="soup3")
    parser.add_argument("--world", type=int, default=1)
    parser.add_argument("--marker", type=int, default=0)
    parser.add_argument("--quicksave", action="store_true", help="press F9 once the world is live")
    parser.add_argument("--out", required=True)
    args = parser.parse_args()
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
