#!/usr/bin/env python3
"""Run a supplied native executable and public control driver without Docker.

The original installation stays in place. Optional session loading uses a prior
public checkpoint; it does not establish original book-slot adapter acceptance.
"""
import argparse
import json
import math
import os
import signal
import socket
import subprocess
import time
from pathlib import Path

import native_session_capture as capture
import replay_chain as replay
from harness import LIVE_LOCK, exclusive_lock


def run(args):
    binary, install, output = (path.resolve() for path in (args.binary, args.install, args.output))
    if output == install or install in output.parents:
        raise ValueError('native runtime output/config must be outside the original install')
    if not binary.is_file() or not install.is_dir():
        raise ValueError('native runtime requires an executable file and original install directory')
    if not args.driver:
        raise ValueError('native runtime requires a bounded public control driver after --')
    if not math.isfinite(args.timeout) or args.timeout <= 0:
        raise ValueError('native runtime timeout must be finite and positive')
    output.mkdir(parents=True, exist_ok=True)
    config = output / 'config'
    config.mkdir(exist_ok=True)
    if config.resolve() == install or install in config.resolve().parents:
        raise ValueError('native config cannot resolve inside the original install')
    if args.cache:
        cache = args.cache.resolve(strict=True)
        if not cache.is_dir() or cache == install or install in cache.parents:
            raise ValueError('shared native cache must be a directory outside the original install')
        target = config / 'cache'
        if target.exists() and target.resolve() != cache:
            raise ValueError(f'{target}: existing cache differs from the requested shared cache')
        if not target.exists():
            target.symlink_to(cache, target_is_directory=True)
    environment = dict(os.environ, CYRUS_CONFIG_DIR=str(config), RUST_MIN_STACK='8388608')
    with exclusive_lock(args.lock), (output / 'process.log').open('x') as log:
        with socket.socket() as listener:
            listener.bind(('127.0.0.1', 0))
            port = listener.getsockname()[1]
        command = [str(binary), '--headless', '--scene', args.scene, '--skip-startup',
                   '--replay-step-micros', '40000', '--volume', '0', '--control',
                   f'127.0.0.1:{port}', '--install', str(install)]
        driver = [value.replace('{port}', str(port)).replace('{output}', str(output))
                  for value in args.driver]
        receipt = dict(binary=str(binary), sha256=capture.file_digest(binary),
                       install=str(install), config=str(config), command=command, driver=driver)
        if args.session:
            receipt['session'] = str(args.session.resolve(strict=True))
            receipt['session_sha256'] = capture.file_digest(args.session)

        def record(phase, **fields):
            receipt.update(phase=phase, **fields)
            capture.atomic(output / 'runtime.json', json.dumps(receipt, indent=2).encode())

        record('starting')
        process = subprocess.Popen(command, env=environment, stdout=log, stderr=log)
        record('starting', pid=process.pid)
        try:
            deadline = time.monotonic() + args.timeout
            while True:
                if process.poll() is not None:
                    raise RuntimeError(f'native exited {process.returncode}; inspect {output / "process.log"}')
                try:
                    connection = socket.create_connection(('127.0.0.1', port), timeout=1)
                    break
                except OSError:
                    if time.monotonic() >= deadline:
                        raise TimeoutError('native control listener did not start')
                    time.sleep(.05)
            connection.settimeout(args.timeout)
            with connection:
                if args.session:
                    record('loading_session')
                    control = replay.Control(connection)
                    control.ready(timeout=args.timeout, min_updates=0)
                    replay.resume_session(control, args.session)
            record('driving')
            subprocess.run(driver, env=environment, check=True, timeout=args.timeout,
                           stdout=log, stderr=log)
            record('completed')
        except BaseException as error:
            record('failed', error=str(error))
            raise
        finally:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
            log.flush()
            os.fsync(log.fileno())


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--install', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--scene', required=True)
    parser.add_argument('--cache', type=Path)
    parser.add_argument('--session', type=Path)
    parser.add_argument('--lock', type=Path, default=LIVE_LOCK)
    parser.add_argument('--timeout', type=float, default=180)
    parser.add_argument('driver', nargs=argparse.REMAINDER)
    args = parser.parse_args()
    if args.driver[:1] == ['--']:
        args.driver.pop(0)
    def stop(*_):
        raise SystemExit('native runner terminated')

    signal.signal(signal.SIGTERM, stop)
    run(args)


if __name__ == '__main__':
    main()
