//! Combat state, taunt presentation services and class-message queues.
use super::ScriptActorId;
use anyhow::Result;
use serde::Serialize;

const QUEUE_CAPACITY: usize = 8;

/// Constructor-owned combat state, separate from animation and death-default selection.
#[derive(Debug, Serialize)]
pub struct CombatControlState {
    /// Authored attack indices; empty slots do not terminate selection.
    pub attack_indices: [Option<u8>; 9],
    /// Authored defense indices, ending at the first zero attribute.
    pub defense_indices: [Option<u8>; 4],
    pub target: Option<ScriptActorId>,
    pub flags: u8,
    pub action: i32,
    pub cooldown: u32,
    /// Selected maintenance phase, separate from the nominal-tick cooldown.
    pub maintenance_phase: i32,
    /// Fighting-query eligibility state; exactly one admits the candidate.
    pub attacker_selection_state: i32,
    /// Retained action indices; their broader selection semantics are unsupported.
    pub retained_action_indices: [i32; 2],
    /// Shared RTX cache index, distinct from the retained dialogue record.
    pub taunt_cache: Option<usize>,
    pub taunt_dialogue: Option<usize>,
    pub taunt_active: i32,
}
impl Default for CombatControlState {
    fn default() -> Self {
        Self {
            attack_indices: [None; 9],
            defense_indices: [None; 4],
            target: None,
            flags: 0,
            action: -1,
            cooldown: 0,
            maintenance_phase: 0,
            attacker_selection_state: 0,
            retained_action_indices: [-1; 2],
            taunt_cache: None,
            taunt_dialogue: Some(0),
            taunt_active: 0,
        }
    }
}

impl CombatControlState {
    /// Bind authored slots following preservation's COMBAT.INI actor move selection.
    pub(crate) fn from_attributes(attributes: &[u8]) -> Self {
        let attack_indices = std::array::from_fn(|slot| {
            let index = attributes[70 + slot];
            (index != 0).then_some(index)
        });
        let mut defense_indices = [None; 4];
        for (slot, &index) in attributes[79..83].iter().enumerate() {
            if index == 0 {
                break;
            }
            defense_indices[slot] = Some(index);
        }
        Self {
            attack_indices,
            defense_indices,
            ..Self::default()
        }
    }
}

/// Synchronous taunt services following preservation's engine/sound.md.
/// Preparation must check all payload and playback dependencies before release.
/// A failure after release retains the last completed ownership transition.
pub trait CombatTauntPlayback {
    fn prepare(&mut self, label: [u8; 4], speech: bool, text_width: Option<u32>) -> Result<()>;
    fn release(&mut self, retained: &mut Option<usize>) -> Result<()>;
    fn load(&mut self, label: [u8; 4]) -> Result<usize>;
    /// Start speech when requested even when no text record will be created.
    fn present(
        &mut self,
        cache_index: usize,
        speech: bool,
        text_width: Option<u32>,
    ) -> Result<super::DialogueLine>;
}

/// Borrow prepared shared-sample taunt services for one synchronous native call.
/// Implementations must not retain the callback or reselect a taunt during loading.
pub trait CombatTauntSource: std::fmt::Debug {
    fn with_playback(
        &self,
        update: &mut dyn FnMut(&mut dyn CombatTauntPlayback) -> Result<()>,
    ) -> Result<()>;
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

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub(super) struct CombatMessagesSnapshot {
    classes: Vec<SavedClassQueue>,
}
#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct SavedClassQueue {
    write: u8,
    entries: Vec<Option<SavedMessage>>,
}
#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct SavedMessage {
    sender: (u64, usize),
    receiver: Option<(u64, usize)>,
    code: i32,
}
impl CombatMessages {
    pub(super) fn snapshot(&self) -> CombatMessagesSnapshot {
        CombatMessagesSnapshot {
            classes: self
                .classes
                .iter()
                .map(|queue| SavedClassQueue {
                    write: queue.write,
                    entries: queue
                        .entries
                        .iter()
                        .map(|entry| {
                            entry.map(|message| SavedMessage {
                                sender: (message.sender.generation, message.sender.index),
                                receiver: message.receiver.map(|id| (id.generation, id.index)),
                                code: message.code,
                            })
                        })
                        .collect(),
                })
                .collect(),
        }
    }

    /// Every retained reference is rebound, including entries behind a read cursor.
    pub(super) fn from_snapshot(
        saved: CombatMessagesSnapshot,
        rebind: &impl Fn(u64, usize) -> Result<ScriptActorId>,
    ) -> Result<Self> {
        anyhow::ensure!(
            saved.classes.len() == 256,
            "saved combat class count differs"
        );
        let mut classes = Vec::with_capacity(saved.classes.len());
        for (class, saved) in saved.classes.into_iter().enumerate() {
            anyhow::ensure!(
                usize::from(saved.write) < QUEUE_CAPACITY,
                "saved combat class {class}: write cursor outside queue"
            );
            anyhow::ensure!(
                saved.entries.len() == QUEUE_CAPACITY,
                "saved combat class {class}: queue capacity differs"
            );
            let mut entries = [None; QUEUE_CAPACITY];
            for (slot, message) in saved.entries.into_iter().enumerate() {
                if let Some(message) = message {
                    anyhow::ensure!(
                        message.code != 0,
                        "saved combat class {class} slot {slot}: occupied zero-code sentinel"
                    );
                    entries[slot] = Some(CombatMessage {
                        sender: rebind(message.sender.0, message.sender.1)?,
                        receiver: message
                            .receiver
                            .map(|(generation, index)| rebind(generation, index))
                            .transpose()?,
                        code: message.code,
                    });
                }
            }
            anyhow::ensure!(
                entries[usize::from(saved.write)].is_none(),
                "saved combat class {class}: write sentinel occupied"
            );
            classes.push(ClassQueue {
                write: saved.write,
                entries,
            });
        }
        Ok(Self { classes })
    }
}
