#!/usr/bin/env python3
"""Exercise the shared control transport against local JSONL servers."""
from contextlib import contextmanager
import json
import socket
import threading
import unittest

from harness import control_response, exchange


@contextmanager
def server(replies):
    received, errors = [], []
    with socket.socket() as listener:
        listener.bind(('127.0.0.1', 0))
        listener.listen()
        listener.settimeout(3)
        def serve():
            try:
                with listener.accept()[0] as connection, connection.makefile('rb') as stream:
                    for chunks in replies:
                        received.append(json.loads(stream.readline()))
                        for chunk in chunks:
                            connection.sendall(chunk)
            except Exception as error:
                errors.append(error)
        thread = threading.Thread(target=serve, daemon=True)
        thread.start()
        try:
            yield listener.getsockname()[1], received
        finally:
            thread.join(4)
            if thread.is_alive():
                raise AssertionError('control server did not stop')
            if errors:
                raise errors[0]


class ControlTransport(unittest.TestCase):
    def test_fragmented_reply_preserves_envelope_and_request(self):
        with server([[b'{"ok":true,', b'"result":{"frame":7}}', b'\n']]) as (port, requests):
            reply = control_response(port, {'type': 'state'}, timeout=1, request_id=9)
        self.assertEqual(reply, {'ok': True, 'result': {'frame': 7}})
        self.assertEqual(requests, [{'id': 9, 'cmd': {'type': 'state'}}])

    def test_rejection_is_returned_for_the_driver_to_validate(self):
        with server([[b'{"ok":false,"error":"invalid owner"}\n']]) as (port, _):
            self.assertEqual(control_response(port, {'type': 'session_load'})['error'], 'invalid owner')

    def test_eof_and_invalid_json_fail(self):
        for chunks, error in [([], ConnectionError), ([b'bad json\n'], json.JSONDecodeError)]:
            with self.subTest(chunks=chunks), server([chunks]) as (port, _):
                with self.assertRaises(error):
                    control_response(port, {'type': 'state'}, timeout=1)

    def test_read_timeout_does_not_wait_for_server_shutdown(self):
        release = threading.Event()
        with socket.socket() as listener:
            listener.bind(('127.0.0.1', 0))
            listener.listen()
            def stall():
                with listener.accept()[0] as connection, connection.makefile('rb') as stream:
                    stream.readline()
                    release.wait(2)
            thread = threading.Thread(target=stall, daemon=True)
            thread.start()
            try:
                with self.assertRaises(TimeoutError):
                    control_response(listener.getsockname()[1], {'type': 'state'},
                                     connect_timeout=1, timeout=.02)
            finally:
                release.set()
                thread.join(3)
            self.assertFalse(thread.is_alive())

    def test_persistent_stream_keeps_order_and_serial_ids(self):
        with server([[b'{"ok":true,"result":1}\n'], [b'{"ok":true,"result":2}\n']]) as (port, requests):
            with socket.create_connection(('127.0.0.1', port), timeout=1) as connection:
                with connection.makefile('rw') as stream:
                    for serial in (1, 2):
                        self.assertEqual(exchange(stream, {'type': 'state'}, request_id=serial)['result'], serial)
        self.assertEqual([row['id'] for row in requests], [1, 2])


if __name__ == '__main__':
    unittest.main()
