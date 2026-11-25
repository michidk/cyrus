"""Shared process coordination for bounded native and original test drivers."""
from contextlib import contextmanager
import fcntl
import json
import socket
from pathlib import Path


LIVE_LOCK = Path('/tmp/cyrus-live.lock')


@contextmanager
def exclusive_lock(path):
    """Own a resource without waiting; close its handle on every exit path.

    The caller chooses one stable path per resource and creates its parent.
    Existing lock-file contents survive contention and reuse. Contention raises
    BlockingIOError, matching flock's nonblocking exclusive acquisition.
    """
    with Path(path).open('a') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        yield lock


def exchange(stream, command, *, request_id=1):
    """Exchange one JSONL control envelope over a caller-owned text stream.

    A persistent caller keeps the same stream so buffered later replies survive.
    The envelope is returned unchanged; command-specific rejection policy belongs
    to the driver. EOF and malformed JSON remain transport/protocol failures.
    """
    stream.write(json.dumps({'id': request_id, 'cmd': command}) + '\n')
    stream.flush()
    line = stream.readline()
    if not line:
        raise ConnectionError('game closed control connection')
    return json.loads(line)


def control_response(port, command, *, host='127.0.0.1', timeout=120,
                     connect_timeout=None, request_id=1):
    """Use one bounded connection, preserving separate connect/read budgets."""
    connection_timeout = timeout if connect_timeout is None else connect_timeout
    with socket.create_connection((host, int(port)), timeout=connection_timeout) as connection:
        connection.settimeout(timeout)
        with connection.makefile('rw') as stream:
            return exchange(stream, command, request_id=request_id)
