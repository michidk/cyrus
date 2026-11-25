"""Shared process coordination for bounded native and original test drivers."""
from contextlib import contextmanager
import fcntl
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
