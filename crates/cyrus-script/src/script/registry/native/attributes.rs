//! Attributes native-call dispatch.
use super::super::{Argument, CallResult, GameHost, Native, Result};
use anyhow::Context;

impl GameHost<'_, '_, '_, '_> {
    pub(in crate::script::registry) fn call_attributes(
        &mut self,
        function: Native,
        arguments: &[Argument],
    ) -> Result<Option<CallResult>> {
        if function == Native::SetAttribute
            && let [index, value] = arguments
            && index.integer()? == 44
        {
            let id = self.registry.id(self.index)?;
            if self.registry.query_is_dead(id, self.selected_player())? {
                self.registry.release_held_walk_nodes(id)?;
                let actor = &mut self.registry.actors[self.index];
                let default = actor
                    .control
                    .as_mut()
                    .and_then(|control| control.combat_death_default.as_mut())
                    .context("SetAttribute attr_panic_group: dead actor without a combat record unsupported")?;
                *default = value.integer()?;
            }
        }

        let actor = &mut self.registry.actors[self.index];

        if let Some(value) = crate::script::attributes::call(
            function,
            arguments,
            &mut actor.attributes,
            actor.control.as_mut(),
        )? {
            return Ok(Some(CallResult::Complete(value)));
        }
        Ok(None)
    }
}
