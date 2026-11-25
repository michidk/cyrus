//! Coordination native-call dispatch.
use super::super::{Argument, Attribute, CallKind, CallResult, GameHost, Native, Result};

impl GameHost<'_, '_, '_, '_> {
    pub(in crate::script::registry) fn call_reset_sync(
        &mut self,
        kind: CallKind,
        function: Native,
        arguments: &[Argument],
    ) -> Result<Option<CallResult>> {
        if kind == CallKind::Function && function == Native::ResetUserSync {
            anyhow::ensure!(arguments.is_empty(), "ResetUserSync requires no arguments");
            let channel = self.registry.actors[self.index].attributes[Attribute::User];
            self.registry
                .globals
                .group_sync
                .reset_user(self.index, channel);
            return Ok(Some(CallResult::Complete(0)));
        }

        if kind == CallKind::Function && function == Native::ResetGroupSync {
            anyhow::ensure!(arguments.is_empty(), "ResetGroupSync requires no arguments");
            // The native clears this actor's posted value.  The channel update retains the
            // released byte; the next world-loop update observes this member as unposted.
            self.registry.globals.group_sync.reset_current(
                self.index,
                self.registry.actors[self.index].attributes[Attribute::Group],
            );
            return Ok(Some(CallResult::Complete(0)));
        }
        Ok(None)
    }

    pub(in crate::script::registry) fn call_coordination(
        &mut self,
        function: Native,
        arguments: &[Argument],
    ) -> Result<Option<CallResult>> {
        // ChangeGroup(group) and ChangeUser(user) move the receiver between the synchronization
        // memberships and store the low eight bits of the argument in its group or user byte. The
        // membership move runs first, so a rejected change leaves the byte untouched. Group-list
        // management is always enabled here. Returns zero.
        if function == Native::ChangeGroup || function == Native::ChangeUser {
            let [value] = arguments else {
                anyhow::bail!("{function} requires one argument")
            };
            let value = value.integer()? as u8;
            let attribute = if function == Native::ChangeGroup {
                Attribute::Group
            } else {
                Attribute::User
            };
            let old = self.registry.actors[self.index].attributes[attribute];
            let sync = &mut self.registry.globals.group_sync;
            if function == Native::ChangeGroup {
                sync.change_group(self.index, old, value)?;
            } else {
                sync.change_user(self.index, old, value)?;
            }
            self.registry.actors[self.index].attributes[attribute] = value;
            return Ok(Some(CallResult::Complete(0)));
        }

        if function == Native::TrueFunction {
            anyhow::ensure!(arguments.len() == 2, "TrueFunction: wrong argument count");
            return Ok(Some(CallResult::Complete(1)));
        }
        Ok(None)
    }
}
