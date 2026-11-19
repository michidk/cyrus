#!/usr/bin/env python3
"""Prepare a running EXTPALAC session for native final-confrontation playtesting.

Use the ordinary console and installed map marker; never set won_game or actor health.
Run with CYRUS_CONTROL_ADDR and an existing host-mounted CYRUS_CONFIG_DIR session.
"""
import argparse
import importlib.util
import os
from pathlib import Path

spec = importlib.util.spec_from_file_location('native_trace', Path(__file__).with_name('native-trace.py'))
trace = importlib.util.module_from_spec(spec)
spec.loader.exec_module(trace)


def prepare(call):
    """Grant the installed confrontation prerequisite, then enter its staging marker."""
    state = call({'type': 'state'})
    scripts = call({'type': 'scripts'})
    if state['state'] != 'InGame' or scripts['world'] != 30:
        raise ValueError('start a settled EXTPALAC (WORLD.INI world 30) session first')
    flags = {f['definition']['name']: f['value'] for f in scripts['flags']['flags']}
    if flags['won_game'] != 0:
        raise ValueError('won_game is already set; use a fresh session')
    # Installed DRAMPAL and RICHTONP reveal themselves when HaveItem(79) succeeds.
    # WORLD.INI maps EXTPALAC to world 30; its scene staging entry is map marker 32.
    def console(line, expected):
        result = call({'type': 'console', 'line': line})
        reports = [row['text'] for row in result['console']['lines'] if row['report']]
        if not reports or reports[-1] != expected:
            raise ValueError(f'console rejected {line}: {reports[-1:]}')

    if state['script_hud']['inventory']['counts'].get('79', 0) == 0:
        console('item add,79', 'item add: 79')
    console('getmapmarker 32', 'getmapmarker: 32')
    call({'type': 'console', 'open': False})
    result = call({'type': 'schedule_input', 'events': [
        {'after_updates': 0}, {'after_updates': 2}]})
    state = call({'type': 'state'})
    if state['script_hud']['inventory']['counts'].get('79', 0) < 1:
        raise ValueError('console did not grant the confrontation prerequisite')
    if state['scripts']['world'] != 30 or state['scripts']['failed']:
        raise ValueError('finale setup changed world or scripts failed')
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--addr', default=os.environ.get('CYRUS_CONTROL_ADDR', '127.0.0.1:7117'))
    args = parser.parse_args()
    trace.wait_live(args.addr, 600)
    prepare(lambda command: trace.call(args.addr, command))
    print('Final confrontation prepared through console; continue with player inputs.')


if __name__ == '__main__':
    main()
