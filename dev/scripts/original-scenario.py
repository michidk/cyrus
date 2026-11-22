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
IMAGE = "cyrus-retro:dev"


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
        self.name = f"retro-{name}"
        self.run = MAGIC / f"retro-run-{name}"
        reader = MAGIC / "tools/original-state.py"
        if not reader.exists():
            sys.exit(f"{reader} is missing; restore the local original-game tooling bundle")
        if not self.run.exists():
            shutil.copytree(MAGIC / "retro-run", self.run, symlinks=True,
                            ignore=shutil.ignore_patterns("*.png", "*.wav"))
        ini = self.run / "Redguard/WORLD.INI"
        text = ini.read_text(errors="surrogateescape")
        text = re.sub(r"(?m)^start_world=\d+", f"start_world={world}", text)
        text = re.sub(r"(?m)^start_marker=\d+", f"start_marker={marker}", text)
        ini.write_text(text, errors="surrogateescape")
        sh("docker", "rm", "-f", self.name)
        sh("docker", "run", "--rm", "--entrypoint", "find", "-v", f"{self.run}:/game", IMAGE,
           "/game/Redguard/maps", "-iname", "*.tsg", "-delete")
        sh("docker", "run", "-d", "--rm", "--name", self.name, "--cap-add", "SYS_PTRACE",
           "-v", f"{self.run}:/game", "-v", f"{MAGIC}/gog:/gog:ro",
           "-v", f"{MAGIC}/gog/extracted/app/game.gog:/game/game.gog:ro", IMAGE)
        sh("docker", "cp", str(reader), f"{self.name}:/tmp/original-state.py")
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
        return raw["counters"]["mainloop_frame"] if raw else None

    def player(self):
        raw = self.raw() or {}
        return next((a for a in raw.get("actors", []) if a["selected"]), None)

    def wait(self, frames, timeout=900, until=None):
        return self.fight.wait_frames(self.sampler, frames, timeout, until=until)

    def stop(self):
        self.sampler.stop()
        sh("docker", "rm", "-f", self.name)


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--name", default="soup3")
    parser.add_argument("--world", type=int, default=1)
    parser.add_argument("--marker", type=int, default=0)
    parser.add_argument("--quicksave", action="store_true", help="press F9 once the world is live")
    parser.add_argument("--out", required=True)
    args = parser.parse_args()
    lock = open(MAGIC / f"retro-run-{args.name}.lock", "w")
    fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
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
