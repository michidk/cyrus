#!/usr/bin/env python3
"""Exercise original startup recording against a simulated DOS prompt and clock."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

INNER = Path(__file__).with_name("original-capture-inner.py")


class StartupCapture(unittest.TestCase):
    def test_records_before_launch_and_stops_at_first_world_progress(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "events.json").write_text("[]")
            (root / "START.OK").touch()
            reader = root / "reader.py"
            reader.write_text(
                "import json, pathlib, sys\n"
                "launched = pathlib.Path(sys.argv[0]).with_name('launched').exists()\n"
                "print(json.dumps({'counters': {'mainloop_frame': int(launched)}}))\n"
            )
            for name, content in {
                "scrot": 'printf shot >> "$CAPTURE_TEST/log"; touch "$2"',
                "xdotool": """
case "$*" in
  'type rgfx.exe') printf launch >> "$CAPTURE_TEST/log" ;;
  'key Return') touch "$CAPTURE_TEST/launched" ;;
  *) printf '%s\\n' "$*" >> "$CAPTURE_TEST/keys"; printf recorder >> "$CAPTURE_TEST/log" ;;
esac
""",
            }.items():
                script = root / name
                script.write_text(f"#!/bin/sh\n{content}\n")
                script.chmod(0o755)
            env = dict(os.environ, PATH=f"{root}:{os.environ['PATH']}",
                       CAPTURE_TEST=str(root), CYRUS_CAPTURE_DIR=str(root),
                       CYRUS_CAPTURE_STARTUP="1", CYRUS_SHOT_INTERVAL="0.1")
            result = subprocess.run(
                [sys.executable, str(INNER), str(reader), str(root / "events.json"), "30"],
                env=env, text=True, capture_output=True, timeout=6,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            timeline = json.loads(result.stdout)
            self.assertTrue((root / "log").read_text().startswith("recordershotlaunch"))
            self.assertLessEqual(timeline["shots"][0][1], timeline["launch"])
            self.assertEqual(timeline["start"]["frame"], 0)
            self.assertEqual(timeline["stop"]["frame"], 1)
            self.assertEqual(timeline["stop"]["reason"], "world-progress")
            chords = (root / "keys").read_text().splitlines()
            self.assertEqual(chords, [
                "keydown Control_L keydown Alt_L key w keyup Alt_L keyup Control_L",
            ] * 2)
            launcher = INNER.with_name("original-audio-capture.sh").read_text()
            self.assertIn('dosbox_host_args=(-set "dosbox hostkey=ctrlalt")', launcher)
            self.assertIn('cyrus-retro:dev "${dosbox_host_args[@]}" >', launcher)


if __name__ == "__main__":
    unittest.main()
