#!/usr/bin/env python3
"""Copy one test executable identified by Cargo's compiler-artifact messages."""
import json
import shutil
import sys
from pathlib import Path


def collect(messages, target, destination):
    executables = set()
    for line in messages:
        try:
            message = json.loads(line)
        except json.JSONDecodeError:
            continue  # libtest output shares stdout with Cargo's JSON messages.
        if (
            isinstance(message, dict)
            and message.get("reason") == "compiler-artifact"
            and message.get("target", {}).get("name") == target
            and message.get("profile", {}).get("test")
            and message.get("executable")
        ):
            executables.add(Path(message["executable"]))
    if len(executables) != 1:
        raise ValueError(f"expected one {target} test executable, found {len(executables)}")
    executable = executables.pop()
    if not executable.is_file() or not executable.stat().st_mode & 0o111:
        raise ValueError(f"test executable missing or not executable: {executable}")
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(executable, destination)


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == '--bundle':
        from test_bundle import main
        del sys.argv[1]
        main()
        raise SystemExit(0)
    if len(sys.argv) != 4:
        raise SystemExit("usage: collect-test-executable.py MESSAGES TARGET DESTINATION")
    with Path(sys.argv[1]).open() as messages:
        collect(messages, sys.argv[2], Path(sys.argv[3]))
