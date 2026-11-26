#!/usr/bin/env python3
"""Observer admission diagnostics with synthetic owned-child handshakes."""
import errno
import unittest
from unittest.mock import Mock, patch

import original_observer_preflight as preflight


class Admission(unittest.TestCase):
    def setUp(self):
        self.child = Mock(pid=123)
        self.ready = dict(pid=123, ppid=100, uid=10, euid=10, dumpable=1)
        self.trace, self.wait = Mock(), Mock()

    def probe(self):
        return preflight.probe(self.child, self.ready, self.trace, self.wait,
                               parent_pid=100, uid=10, euid=10)

    def test_attach_denial_retains_operation_errno_and_relationship_without_retry(self):
        self.trace.side_effect = OSError(errno.EACCES, 'denied')
        report = self.probe()
        self.assertFalse(report['supported'])
        self.assertEqual(report['stage'], 'PTRACE_ATTACH')
        self.assertEqual(report['errno'], errno.EACCES)
        self.assertEqual(report['child'], self.ready)
        self.assertEqual(self.trace.call_args_list, [((16, 123),)])
        self.wait.assert_not_called()

    def test_ready_owned_child_is_stopped_and_detached_before_success(self):
        self.assertTrue(self.probe()['supported'])
        self.assertEqual(self.trace.call_args_list, [((16, 123),), ((17, 123),)])
        self.wait.assert_called_once_with(123)

    def test_wrong_pid_parent_identity_or_dumpability_never_attempts_attach(self):
        for field, value in [('pid', 124), ('ppid', 99), ('uid', 11), ('euid', 11), ('dumpable', 0)]:
            with self.subTest(field=field):
                original = self.ready[field]
                self.ready[field] = value
                report = self.probe()
                self.assertFalse(report['supported'])
                self.assertEqual(report['stage'], 'readiness')
                self.trace.assert_not_called()
                self.ready[field] = original

    def test_launch_failure_is_a_diagnostic_not_an_uncaught_exception(self):
        with patch.object(preflight.subprocess, 'Popen', side_effect=OSError(errno.ENOENT, 'missing interpreter')):
            report = preflight.run()
        self.assertFalse(report['supported'])
        self.assertEqual(report['stage'], 'launch')
        self.assertEqual(report['errno'], errno.ENOENT)

    def test_readiness_timeout_kills_and_reaps_owned_child_without_attach(self):
        child = Mock(pid=123, returncode=-9)
        child.poll.return_value = None
        with patch.object(preflight.subprocess, 'Popen', return_value=child), \
                patch.object(preflight.select, 'select', return_value=([], [], [])), \
                patch.object(preflight.ctypes, 'CDLL') as libc:
            report = preflight.run(timeout=.01)
        self.assertFalse(report['supported'])
        self.assertEqual(report['stage'], 'readiness')
        self.assertTrue(report['child_reaped'])
        child.kill.assert_called_once()
        child.communicate.assert_called_once_with(timeout=.01)
        libc.return_value.ptrace.assert_not_called()

    def test_wait_or_detach_failure_is_not_reported_as_an_attach_denial(self):
        self.wait.side_effect = TimeoutError('not stopped')
        report = self.probe()
        self.assertEqual(report['stage'], 'wait_stopped')
        self.assertFalse(report['supported'])
        self.assertIn(((17, 123),), self.trace.call_args_list)
        self.wait.side_effect = None
        self.trace.reset_mock()
        self.trace.side_effect = [None, OSError(errno.EPERM, 'detach denied'), None]
        report = self.probe()
        self.assertEqual(report['stage'], 'PTRACE_DETACH')
        self.assertEqual(report['errno'], errno.EPERM)


if __name__ == '__main__':
    unittest.main()
