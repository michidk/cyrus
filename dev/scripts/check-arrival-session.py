#!/usr/bin/env python3
"""Check arrival draw eligibility and dock animation across public session restore."""
import argparse
import json
from pathlib import Path

from native_session_assertions import assert_arrival
from native_session_capture import document


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('before', type=Path)
    parser.add_argument('after', type=Path)
    parser.add_argument('expectations', type=Path)
    parser.add_argument('observations', type=Path, nargs='+')
    args = parser.parse_args()
    try:
        result = assert_arrival(document(args.before.read_text()), document(args.after.read_text()),
                                [json.loads(path.read_text()) for path in args.observations],
                                json.loads(args.expectations.read_text()))
    except (ValueError, KeyError, IndexError, TypeError, OSError) as error:
        parser.exit(1, f'arrival rejected: {error}\n')
    print(json.dumps(result, sort_keys=True))


if __name__ == '__main__':
    main()
