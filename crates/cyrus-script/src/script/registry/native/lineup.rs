//! Lineup native-call dispatch.
use super::super::{GameHost, Native};

impl GameHost<'_, '_, '_, '_> {
    pub(in crate::script::registry) fn prepare_lineup_call(&mut self, function: Native) {
        if function == Native::PlayerLineUp {
            // Unknown until the native stores this invocation's return value.
            self.registry.globals.lineup_result = None;
        }
    }
}
