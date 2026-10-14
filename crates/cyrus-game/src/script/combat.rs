//! Combat message queues follow preservation's SOUP combat-maintenance rules.
use super::ScriptActorId;
use anyhow::Result;
use serde::Serialize;

const QUEUE_CAPACITY: usize = 8;

/// Constructor-owned combat state, separate from animation and death-default selection.
#[derive(Debug, Serialize)]
pub struct CombatControlState {
    pub target: Option<ScriptActorId>,
    pub flags: u8,
    pub action: i32,
    pub cooldown: u32,
}
impl Default for CombatControlState {
    fn default() -> Self {
        Self {
            target: None,
            flags: 0,
            action: -1,
            cooldown: 0,
        }
    }
}

/// Logical identities retain sender and addressed receiver across queue operations.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct CombatMessage {
    pub sender: ScriptActorId,
    pub receiver: Option<ScriptActorId>,
    pub code: i32,
}

#[derive(Default, Debug, Serialize)]
struct ClassQueue {
    write: u8,
    entries: [Option<CombatMessage>; QUEUE_CAPACITY],
}

/// Shared class queues, separate from each actor's retained byte read cursor.
/// Startup creates empty queues; the combat reset owner must explicitly reset them.
#[derive(Debug, Serialize)]
pub struct CombatMessages {
    classes: Vec<ClassQueue>,
}
impl Default for CombatMessages {
    fn default() -> Self {
        Self {
            classes: (0..256).map(|_| ClassQueue::default()).collect(),
        }
    }
}
impl CombatMessages {
    /// Send a nonzero message and clear the next slot's code. Zero does nothing.
    /// The caller validates receiver identity and supplies its authored class.
    pub fn send(&mut self, class: u8, message: CombatMessage) {
        if message.code == 0 {
            return;
        }
        let queue = &mut self.classes[usize::from(class)];
        queue.entries[usize::from(queue.write)] = Some(message);
        queue.write = (queue.write + 1) % QUEUE_CAPACITY as u8;
        queue.entries[usize::from(queue.write)] = None;
    }

    /// Consume at most one addressed nonzero message, skipping other receivers.
    /// A cleared slot stops reading without advancing, matching the zero-code sentinel.
    pub fn consume(
        &self,
        class: u8,
        actor: ScriptActorId,
        read: &mut u8,
    ) -> Result<Option<CombatMessage>> {
        anyhow::ensure!(
            usize::from(*read) < QUEUE_CAPACITY,
            "combat message class {class} actor {} read cursor {} outside queue",
            actor.index,
            *read
        );
        let queue = &self.classes[usize::from(class)];
        while *read != queue.write {
            let Some(message) = queue.entries[usize::from(*read)] else {
                return Ok(None);
            };
            *read = (*read + 1) % QUEUE_CAPACITY as u8;
            if message.receiver.is_none_or(|receiver| receiver == actor) {
                return Ok(Some(message));
            }
        }
        Ok(None)
    }

    /// Clear all class queues; actor cursor reset belongs to actor lifecycle.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}
