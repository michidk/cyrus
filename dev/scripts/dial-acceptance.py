#!/usr/bin/env python3
"""Exercise installed CATACOMB CTDIAL01 through production input and inventory.

Start a fresh native process with --scene CATACOMB and run under the live-run lock:
  python3 dev/scripts/dial-acceptance.py --port 7117 --case rune
  python3 dev/scripts/dial-acceptance.py --port 7117 --case empty
Use a separate fresh process for each case. stdout is a JSONL acceptance record.
Placement follows the original/native dial observation; no installed script is replaced.
"""
import argparse
import json
import socket
import time


def check(sample):
    """Failures remain fatal even if a later update resumes or completes the task."""
    assert not sample['failures'], f"script failure: {sample['failures']}"
    assert sample['count'] in (0, 1), f"unexpected rune count: {sample}"


class Acceptance:
    def __init__(self, rune):
        self.rune = rune
        self.started = False
        self.consumed = False
        self.completed_frame = None
        self.failure = None

    def observe(self, sample):
        if self.failure is not None:
            raise AssertionError(self.failure)
        try:
            check(sample)
            self.started |= sample['tasks'] > 0
            if not self.rune:
                assert sample['count'] == 0, 'empty control acquired a rune'
            elif self.consumed:
                assert sample['count'] == 0, 'consumed rune returned'
            elif sample['count'] == 0:
                assert self.started, 'rune disappeared before activation task was observed'
                self.consumed = True
            if self.started and sample['tasks'] == 0 and sample['lockout'] == 0:
                if self.completed_frame is None:
                    self.completed_frame = sample['frame']
                return (not self.rune or self.consumed) and sample['frame'] - self.completed_frame >= 12
            return False
        except AssertionError as error:
            self.failure = str(error)
            raise


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--port', type=int, default=7117)
    parser.add_argument('--case', choices=('rune', 'empty'), required=True)
    parser.add_argument('--timeout', type=int, default=600)
    args = parser.parse_args()

    def call(command):
        with socket.create_connection(('127.0.0.1', args.port), timeout=120) as conn:
            conn.sendall((json.dumps({'id': 1, 'cmd': command}) + '\n').encode())
            reply = json.loads(conn.makefile().readline())
        assert reply.get('ok'), f"{command}: {reply}"
        return reply['result']

    def sample():
        actors = []
        while True:
            page = call({'type': 'scripts', 'offset': len(actors), 'limit': 64})
            actors.extend(page['actors'])
            if len(page['actors']) < 64:
                break
        state = call({'type': 'state'})
        assert state['scene'] == 'CATACOMB' and state['scene_ready'], 'wrong or retiring world'
        dial, = [actor for actor in actors if actor['name'] == 'CTDIAL01']
        counts = state['script_hud']['inventory']['counts']
        result = dict(frame=page['globals']['world_loop_frame'], tasks=dial['tasks'],
                      lockout=page['globals']['lockout'], count=counts.get('15', 0),
                      prompt=page['activation']['shown_prompt'],
                      locals=dial['locals'], failures=[(a['name'], a['failure']) for a in actors if a.get('failure')])
        print(json.dumps(result), flush=True)
        check(result)
        return result

    deadline = time.monotonic() + args.timeout
    while True:
        state = call({'type': 'state'})
        if state['scene'] == 'CATACOMB' and state['scene_ready'] and state['state'] == 'InGame':
            break
        assert time.monotonic() < deadline, 'CATACOMB readiness timeout'
        time.sleep(2)
    call({'type': 'cheat', 'name': 'drevil', 'value': 1})
    before = sample()
    assert before['count'] == 0, 'requires a fresh world without rune 15'
    if args.case == 'rune':
        call({'type': 'console', 'line': 'item add,15'})
        call({'type': 'select_item', 'id': 15})
    call({'type': 'place_player', 'x': 34757, 'y': 436, 'z': 32811, 'heading': 1280})
    call({'type': 'reset_camera'})
    expected = 'INSERT RUNE INTO NOTCH' if args.case == 'rune' else 'TURN DIAL'
    while True:
        ready = sample()
        if ready['prompt'] == expected:
            break
        assert time.monotonic() < deadline, f'missing prompt {expected}'
        time.sleep(2)
    assert ready['count'] == (args.case == 'rune'), 'setup did not establish rune ownership'
    call({'type': 'input', 'key': 'ControlLeft', 'action': 'tap'})
    acceptance = Acceptance(args.case == 'rune')
    while not acceptance.observe(sample()):
        assert time.monotonic() < deadline, 'dial task/continuation timeout'
        time.sleep(2)
    if args.case == 'rune':
        assert sample()['prompt'] == 'REMOVE RUNE FROM NOTCH', 'inserted rune not retained'
    else:
        # The empty dial can turn again. Inserting a rune changes the next action
        # to removal, so only the empty control repeats an ordinary turn.
        assert sample()['prompt'] == 'TURN DIAL', 'empty dial did not remain usable'
        call({'type': 'input', 'key': 'ControlLeft', 'action': 'tap'})
        continuation = Acceptance(False)
        while not continuation.observe(sample()):
            assert time.monotonic() < deadline, 'second dial activation timeout'
            time.sleep(2)
    print(json.dumps({'result': 'pass', 'case': args.case}), flush=True)


if __name__ == '__main__':
    main()
