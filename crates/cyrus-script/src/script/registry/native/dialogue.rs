//! Dialogue native-call dispatch.
use super::super::{Argument, CallResult, GameHost, Native, Result};

impl GameHost<'_, '_, '_, '_> {
    pub(in crate::script::registry) fn call_dialogue(
        &mut self,
        function: Native,
        arguments: &[Argument],
    ) -> Result<Option<CallResult>> {
        // The dialogue and topic-menu mode words. DialogBegin also empties the remembered topic
        // text, the first byte of the buffer the menu compares new entries against.
        if let Some((dialogue, begin)) = match function {
            Native::DialogBegin => Some((true, true)),
            Native::DialogEnd => Some((true, false)),
            Native::MenuBegin => Some((false, true)),
            Native::MenuEnd => Some((false, false)),
            _ => None,
        } {
            anyhow::ensure!(arguments.is_empty(), "{function} requires no arguments");
            match (dialogue, begin) {
                (true, begin) => {
                    if begin {
                        self.registry.globals.forget_choice();
                    }
                    self.registry.globals.set_dialogue_state(begin)
                }
                (false, true) => self.registry.globals.begin_topic_menu(),
                (false, false) => self.registry.globals.end_topic_menu(),
            }
            return Ok(Some(CallResult::Complete(0)));
        }
        Ok(None)
    }
}
