#!/usr/bin/env python3
"""Retain source-bound successful command receipts across runtime resets."""
import argparse
import fcntl
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def save(path, record):
    with tempfile.NamedTemporaryFile(mode='w', dir=path.parent, delete=False) as stream:
        temporary = Path(stream.name)
        json.dump(record, stream, sort_keys=True)
        stream.write('\n')
        stream.flush()
        os.fsync(stream.fileno())
    os.replace(temporary, path)
    descriptor = os.open(path.parent, os.O_RDONLY)
    try:
        os.fsync(descriptor)
    finally:
        os.close(descriptor)


def run(directory, command, inputs, environment, outputs=()):
    """Reuse only successful, unchanged commands with intact logs and artifacts.

    Callers must include the command's complete source/configuration inputs and
    relevant toolchain environment. Interrupted attempts restart from their setup;
    a process PID or partial log never establishes completion.
    """
    directory.mkdir(parents=True, exist_ok=True)
    with (directory / 'stage.lock').open('w') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        identity = dict(command=command, cwd=str(Path.cwd()),
                        inputs={str(path): digest(path) for path in inputs},
                        environment={name: os.environ.get(name) for name in environment},
                        outputs=[str(path) for path in outputs])
        receipt, log = directory / 'receipt.json', directory / 'output.log'
        previous = json.loads(receipt.read_text()) if receipt.exists() else {}
        if previous.get('status') == 'accepted' and previous.get('identity') == identity:
            artifacts = previous.get('artifacts', {})
            if (log.is_file() and digest(log) == previous.get('log_sha256')
                    and all(path.is_file() and digest(path) == artifacts.get(str(path))
                            for path in outputs)):
                return 0
        record = dict(status='running', identity=identity)
        save(receipt, record)
        with log.open('wb') as stream:
            result = subprocess.run(command, stdout=stream, stderr=subprocess.STDOUT)
            stream.flush()
            os.fsync(stream.fileno())
        unchanged = all(path.is_file() and digest(path) == identity['inputs'][str(path)]
                        for path in inputs)
        complete = result.returncode == 0 and unchanged and all(path.is_file() for path in outputs)
        record.update(status='accepted' if complete else 'rejected',
                      returncode=result.returncode, log_sha256=digest(log))
        if complete:
            record['artifacts'] = {str(path): digest(path) for path in outputs}
        save(receipt, record)
        return 0 if complete else result.returncode or 1


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('directory', type=Path)
    parser.add_argument('--input', action='append', type=Path, default=[])
    parser.add_argument('--environment', action='append', default=[])
    parser.add_argument('--output', action='append', type=Path, default=[])
    parser.add_argument('--command', nargs=argparse.REMAINDER, required=True)
    args = parser.parse_args()
    command = args.command[1:] if args.command[:1] == ['--'] else args.command
    if not command or not args.input:
        parser.error('a command and its source/configuration inputs are required')
    raise SystemExit(run(args.directory, command, args.input, args.environment, args.output))


if __name__ == '__main__':
    main()
