"""Validate a stopped direct-call return using locally established bindings."""
import struct


class DirectCallReturn:
    """Require a matching direct call and an observed read of its return slot.

    A stack-shaped value alone is not an active call witness. The caller must
    bind a stopped guest stack slot and observe its subsequent access using the
    paired read/write comparators. This validates the instruction and slot, not
    the semantic meaning of the callee or the completeness of a call stack.
    Executable-specific addresses and operation names stay with the local probe.
    """

    def __init__(self, read, *, call, target, return_slot):
        for value in (call, target, return_slot):
            if type(value) is not int or value <= 0:
                raise ValueError('direct call requires positive local bindings')
        self.read, self.return_slot = read, return_slot
        code = read(call, 5)
        if len(code) != 5 or code[0] != 0xe8:
            raise ValueError('direct call instruction missing or truncated')
        if call + 5 + struct.unpack_from('<i', code, 1)[0] != target:
            raise ValueError('direct call target differs from bound operation')
        self.expected = call + 5
        self.completed = False
        self.invalidated = False
        self._check_slot()

    def _check_slot(self):
        data = self.read(self.return_slot, 4)
        if len(data) != 4 or struct.unpack('<I', data)[0] != self.expected:
            raise ValueError('direct call return slot missing or overwritten')

    def observe(self, access):
        """Accept exactly one tracee read; stores and repeated witnesses reject."""
        if access != 'read' or self.completed or self.invalidated:
            self.invalidated = True
            raise ValueError('direct call requires one observed return-slot read')
        try:
            self._check_slot()
        except ValueError:
            self.invalidated = True
            raise
        self.completed = True
        return dict(direct_call_target_matches=True, return_slot_read=True)

    def result(self):
        if not self.completed or self.invalidated:
            raise ValueError('direct call return has not been observed')
        return dict(direct_call_target_matches=True, return_slot_read=True)
