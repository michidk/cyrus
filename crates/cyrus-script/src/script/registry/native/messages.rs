//! Messages native-call dispatch.
use super::super::{Argument, CallResult, GameHost, Native, Result};
use anyhow::Context;

impl GameHost<'_, '_, '_, '_> {
    pub(in crate::script::registry) fn call_delete_broadcast(
        &mut self,
        function: Native,
        arguments: &[Argument],
    ) -> Result<Option<CallResult>> {
        if function == Native::DeleteBroadcast {
            anyhow::ensure!(
                arguments.is_empty(),
                "DeleteBroadcast requires no arguments"
            );
            self.registry
                .globals
                .combat_messages
                .delete_broadcast(&mut self.registry.actors[self.index].broadcast_index);
            return Ok(Some(CallResult::Complete(0)));
        }
        Ok(None)
    }

    pub(in crate::script::registry) fn call_messages(
        &mut self,
        function: Native,
        arguments: &[Argument],
    ) -> Result<Option<CallResult>> {
        // GetMessageSender returns the sender of the receiver's selected class message as an actor
        // reference, or zero when nothing is selected or the entry has no sender.
        if function == Native::GetMessageSender {
            anyhow::ensure!(
                arguments.is_empty(),
                "GetMessageSender requires no arguments"
            );
            let receiver = self.registry.id(self.index)?;
            return Ok(Some(
                match self.registry.selected_message_sender(receiver)? {
                    Some(sender) => {
                        self.registry.globals.actor_result = Some(sender);
                        CallResult::CompleteWithoutValue
                    }
                    None => CallResult::Complete(0),
                },
            ));
        }

        // GetBroadcastSender returns the selected broadcast entry's sender pointer, unchanged, as an
        // actor reference; zero when nothing is selected or the entry has no sender.
        if function == Native::GetBroadcastSender {
            anyhow::ensure!(
                arguments.is_empty(),
                "GetBroadcastSender requires no arguments"
            );
            let receiver = self.registry.id(self.index)?;
            return Ok(Some(
                match self.registry.selected_broadcast_sender(receiver)? {
                    Some(sender) => {
                        self.registry.globals.actor_result = Some(sender);
                        CallResult::CompleteWithoutValue
                    }
                    None => CallResult::Complete(0),
                },
            ));
        }

        // GetMessageSenderId returns the selected entry's sender's runtime identifier. The original
        // reads the sender without a null check, so a selected entry without one is an error here.
        if function == Native::GetMessageSenderId || function == Native::GetBroadcastSenderId {
            anyhow::ensure!(arguments.is_empty(), "{function:?} requires no arguments");
            let receiver = self.registry.id(self.index)?;
            let entry = if function == Native::GetMessageSenderId {
                self.registry.selected_message_entry(receiver)?
            } else {
                self.registry.selected_broadcast_entry(receiver)?
            };
            let Some(entry) = entry else {
                return Ok(Some(CallResult::Complete(0)));
            };
            let sender =
                entry.with_context(|| format!("{function:?}: selected entry has no sender"))?;
            let index = self
                .registry
                .resolve(sender)
                .with_context(|| format!("{function:?} sender"))?;
            return Ok(Some(CallResult::Complete(i32::from(
                self.registry.actors[index]
                    .runtime_identifier
                    .with_context(|| {
                        format!("{function:?}: sender runtime identifier unavailable")
                    })?,
            ))));
        }

        // SendMessage(recipient, value) writes sender (the receiver), recipient and value on the
        // recipient's class channel. The original takes the value from a third operand, so the
        // declared single-argument form passes the receiver token as the value and an integer
        // recipient is read as a pointer; neither is representable and both fail here. A zero
        // value returns before the recipient is read.
        if function == Native::SendMessage {
            let [recipient, value] = arguments else {
                anyhow::bail!(
                    "SendMessage requires a recipient actor reference and a value: the original reads the value from an operand the declared single argument does not supply"
                )
            };
            if value.integer()? == 0 {
                return Ok(Some(CallResult::Complete(0)));
            }
            let recipient = recipient.actor().context("SendMessage recipient")?;
            let sender = self.registry.id(self.index)?;
            self.registry
                .send_combat_message(sender, recipient, value.integer()?.into())?;
            return Ok(Some(CallResult::Complete(0)));
        }

        // IsMessageWaiting is one when the receiver's selection differs from its channel's
        // no-selection index; it scans nothing and changes nothing.
        if function == Native::IsMessageWaiting {
            anyhow::ensure!(
                arguments.is_empty(),
                "IsMessageWaiting requires no arguments"
            );
            let receiver = self.registry.id(self.index)?;
            return Ok(Some(CallResult::Complete(i32::from(
                self.registry.selected_message_entry(receiver)?.is_some(),
            ))));
        }

        // SendBroadcast appends the value to channel zero with the receiver as sender and no
        // recipient. A zero value returns before anything is read or written.
        if function == Native::SendBroadcast {
            let [value] = arguments else {
                anyhow::bail!("SendBroadcast requires one value argument")
            };
            let sender = self.registry.id(self.index)?;
            self.registry
                .send_broadcast(sender, value.integer()?.into())?;
            return Ok(Some(CallResult::Complete(0)));
        }

        // ReplyBroadcastSender answers the selected broadcast's sender on channel zero.
        if function == Native::ReplyBroadcastSender {
            let [value] = arguments else {
                anyhow::bail!("ReplyBroadcastSender requires one value")
            };
            let receiver = self.registry.id(self.index)?;
            self.registry
                .reply_to_broadcast_sender(receiver, value.integer()?)?;
            return Ok(Some(CallResult::Complete(0)));
        }

        // IsBroadcastWaiting is one when the receiver's broadcast selection differs from the
        // no-selection index; it scans nothing and changes nothing.
        if function == Native::IsBroadcastWaiting {
            anyhow::ensure!(
                arguments.is_empty(),
                "IsBroadcastWaiting requires no arguments"
            );
            let receiver = self.registry.id(self.index)?;
            return Ok(Some(CallResult::Complete(i32::from(
                self.registry.selected_broadcast_entry(receiver)?.is_some(),
            ))));
        }

        // ReceiveBroadcast is ReceiveMessage over the broadcast selection and channel zero.
        if function == Native::ReceiveBroadcast {
            anyhow::ensure!(
                arguments.is_empty(),
                "ReceiveBroadcast requires no arguments"
            );
            let receiver = self.registry.id(self.index)?;
            return Ok(Some(CallResult::Complete(
                self.registry.receive_broadcast(receiver)?,
            )));
        }

        // ReplySender sends the value to the sender of the receiver's selected message.
        if function == Native::ReplySender {
            let [value] = arguments else {
                anyhow::bail!("ReplySender requires one value")
            };
            let receiver = self.registry.id(self.index)?;
            self.registry
                .reply_to_message_sender(receiver, value.integer()?)?;
            return Ok(Some(CallResult::Complete(0)));
        }

        // ReceiveMessage returns the value word of the first entry addressed to the receiver.
        if function == Native::ReceiveMessage {
            anyhow::ensure!(arguments.is_empty(), "ReceiveMessage requires no arguments");
            let receiver = self.registry.id(self.index)?;
            return Ok(Some(CallResult::Complete(
                self.registry.receive_message(receiver)?,
            )));
        }

        // DeleteMessage advances the receiver's message selection and returns zero.
        if function == Native::DeleteMessage {
            anyhow::ensure!(arguments.is_empty(), "DeleteMessage requires no arguments");
            let receiver = self.registry.id(self.index)?;
            self.registry.advance_message_selection(receiver)?;
            return Ok(Some(CallResult::Complete(0)));
        }

        // MyId is the actor's signed 16-bit runtime identifier (pool index plus three).
        if function == Native::MyId {
            return Ok(Some(CallResult::Complete(i32::from(
                self.registry.actors[self.index]
                    .runtime_identifier
                    .context("MyId: runtime identifier unavailable")?,
            ))));
        }
        Ok(None)
    }
}
