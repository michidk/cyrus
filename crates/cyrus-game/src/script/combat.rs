//! Combat state, taunt presentation services and class-message queues.
use super::ScriptActorId;
use anyhow::{Context, Result};
use serde::Serialize;

const QUEUE_CAPACITY: usize = 8;

/// Retained values of the nonplayer decision stage, set by combat construction (see preservation's
/// engine/SOUP.md "Combat maintenance") and changed by the stage and its attack and defence helpers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct CombatDecision {
    /// Attack and defence executor state: 0 idle, 1 defending, 2 attacking.
    pub attack_state: i32,
    /// First move-table selector (the attack list index of the current move).
    pub move_selector: i32,
    /// Aggression budget: starts at 50, refilled by the stage and spent by each attack.
    pub aggression: i32,
    /// Near state of the attacker slot (0 or 1) and the ticks spent far from the target.
    pub near_state: i32,
    pub far_ticks: i32,
    /// Pacing divisors: 129 minus attributes 63 (attack) and 62 (defence); the defence counter
    /// starts at 129 minus attribute 58.
    pub attack_pacing: i32,
    pub defence_pacing: i32,
    pub defence_counter: i32,
}

impl Default for CombatDecision {
    fn default() -> Self {
        Self {
            attack_state: 0,
            move_selector: 0,
            aggression: 50,
            near_state: 0,
            far_ticks: 0,
            attack_pacing: 0,
            defence_pacing: 0,
            defence_counter: 0,
        }
    }
}

impl CombatDecision {
    /// Combat construction: the pacing values are 129 minus actor attributes 63, 62 and 58.
    pub(crate) fn from_attributes(attributes: &[u8]) -> Self {
        Self {
            attack_pacing: 129 - i32::from(attributes[63]),
            defence_pacing: 129 - i32::from(attributes[62]),
            defence_counter: 129 - i32::from(attributes[58]),
            ..Self::default()
        }
    }
}

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
    /// World object-pool allocation index, retained across combat reinitialization.
    pub allocation_index: i32,
    /// Fighting-query eligibility state; exactly one admits the candidate.
    pub attacker_selection_state: i32,
    /// Retained action indices; their broader selection semantics are unsupported.
    pub retained_action_indices: [i32; 2],
    /// Shared RTX cache index, distinct from the retained dialogue record.
    pub taunt_cache: Option<usize>,
    pub taunt_dialogue: Option<usize>,
    pub taunt_active: i32,
    /// Signed constructor-owned timed-taunt divisor: 129 minus attribute 87.
    pub taunt_period: i32,
    /// Script-assigned strength and armor values (`setStrength`, `setArmor`).
    pub strength: i32,
    pub armor: i32,
    /// Nonplayer decision-stage state; see [`CombatDecision`].
    pub decision: CombatDecision,
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
            allocation_index: 0,
            attacker_selection_state: 0,
            retained_action_indices: [-1; 2],
            taunt_cache: None,
            taunt_dialogue: Some(0),
            taunt_active: 0,
            taunt_period: 0,
            strength: 0,
            armor: 0,
            decision: CombatDecision::default(),
        }
    }
}

impl CombatControlState {
    /// Allocate only for a new combat record; reinitialization retains its index.
    pub(crate) fn initialize(
        attributes: &[u8],
        previous: Option<&Self>,
        next_index: &mut i32,
    ) -> Option<Self> {
        if attributes[14] == 0 {
            return None;
        }
        let allocation_index = previous.map_or_else(
            || {
                let index = *next_index;
                *next_index = next_index.wrapping_add(1);
                index
            },
            |record| record.allocation_index,
        );
        Some(Self {
            allocation_index,
            ..Self::from_attributes(attributes)
        })
    }

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
            // The constructor copies attributes 2 and 4 into the strength and armor values.
            strength: i32::from(attributes[2]),
            armor: i32::from(attributes[4]),
            taunt_period: 129 - i32::from(attributes[87]),
            decision: CombatDecision::from_attributes(attributes),
            ..Self::default()
        }
    }
}

/// Select a timed taunt for an already admitted request outside the ordered family.
/// Follows preservation's [selection rules](https://github.com/michidk/redguard-preservation/pull/113).
/// Consumes fifteen inclusive 0..=32767 draws even after selecting a record. The
/// caller supplies the wrapping allocation-index/frame sum and the retained signed period.
/// The actor list has no one-past sentinel: that draw rejects after consumption.
/// An invalid period rejects only if a matching nonempty record was selected.
pub fn select_timed_combat_taunt(
    taunts: &[cyrus_assets::script::CombatTaunt],
    trigger: u8,
    pacing_sum: i32,
    period: i32,
    draw: &mut dyn FnMut() -> i32,
) -> Result<Option<usize>> {
    anyhow::ensure!(
        !matches!(trigger, 0 | 2 | 6 | 10 | 11),
        "combat taunt type {trigger} requires ordered selection"
    );
    anyhow::ensure!(taunts.len() <= 255, "combat taunt count exceeds actor byte");
    if taunts.is_empty() {
        return Ok(None);
    }
    let mut selected = None;
    for _ in 0..15 {
        let random = draw();
        anyhow::ensure!(
            (0..=32767).contains(&random),
            "combat taunt random draw {random} outside 0..=32767"
        );
        let index = random as usize * taunts.len() / 32767;
        let taunt = taunts.get(index).with_context(|| {
            format!(
                "combat taunt index {index} outside {}-entry actor pointer list; one-past access unsupported",
                taunts.len()
            )
        })?;
        if taunt.trigger == trigger
            && taunt.label != [0; 4]
            && (selected.is_none() || pacing_sum % 2 == 0)
        {
            selected = Some(index);
        }
    }
    if selected.is_some()
        && pacing_sum
            .checked_rem(period)
            .context("combat taunt invalid signed period divisor")?
            != 0
    {
        selected = None;
    }
    Ok(selected)
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

/// Whether `SheathSword` acts: a combat record with no cooldown, no weapon-move state and
/// no draw or sheathe in progress, and a held weapon. Animation and task-system readiness
/// are checked by the caller.
pub(super) fn sheath_eligible(
    control: &super::registry::ActorControlState,
    holding: bool,
    drawing: bool,
    sheathing: bool,
) -> bool {
    control
        .combat
        .as_ref()
        .is_some_and(|combat| combat.cooldown == 0)
        && !(27..=32).contains(&control.living_state)
        && !drawing
        && !sheathing
        && holding
}

/// Enter the sheathing state: lifecycle byte 21, companion byte cleared, combat flag 0x04
/// cleared and a 24-tick cooldown.
pub(super) fn begin_sheathing(control: &mut super::registry::ActorControlState) {
    control.living_state = 21;
    control.motion_companion = 0;
    let combat = control
        .combat
        .as_mut()
        .expect("eligible actor has combat state");
    combat.flags &= !4;
    combat.cooldown = 24;
}

/// Whether `control` is a valid combat target for the actor `own` (preservation's "Valid target"
/// rule): not killed while in lifecycle state 8, has a combat record, and does not target itself.
pub(super) fn combat_target_valid(
    control: &super::registry::ActorControlState,
    own: super::ScriptActorId,
) -> bool {
    !(control.death_selector == 8 && control.killed)
        && control
            .combat
            .as_ref()
            .is_some_and(|combat| combat.target != Some(own))
}

/// How an actor decides it is engaged with its target ("Engagement" rule).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Engagement {
    /// `attr_combat_ignore_in_sight` is set.
    Always,
    /// In sight within this distance (Q8 units).
    InSightWithin(i32),
}

/// Attribute 121 (`attr_combat_ignore_in_sight`) and 123 (`attr_combat_long_sight`).
pub(super) fn engagement(attributes: &[u8]) -> Engagement {
    if attributes[121] != 0 {
        Engagement::Always
    } else if attributes[123] != 0 {
        Engagement::InSightWithin(0x80000)
    } else {
        Engagement::InSightWithin(0x40000)
    }
}

#[cfg(test)]
mod sheath_tests {
    use super::*;
    use crate::script::registry::ActorControlState;

    fn armed() -> ActorControlState {
        ActorControlState {
            combat: Some(CombatControlState {
                flags: 0x07,
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn sheathing_requires_combat_no_cooldown_a_held_weapon_and_no_weapon_motion() {
        assert!(sheath_eligible(&armed(), true, false, false));
        let mut no_combat = armed();
        no_combat.combat = None;
        let mut cooling = armed();
        cooling.combat.as_mut().unwrap().cooldown = 1;
        let mut moving = armed();
        moving.living_state = 27;
        for (control, holding, drawing, sheathing) in [
            (&no_combat, true, false, false),
            (&cooling, true, false, false),
            (&moving, true, false, false),
            (&armed(), false, false, false),
            (&armed(), true, true, false),
            (&armed(), true, false, true),
        ] {
            assert!(!sheath_eligible(control, holding, drawing, sheathing));
        }
    }

    #[test]
    fn begin_sheathing_sets_state_clears_the_combat_flag_and_starts_the_cooldown() {
        let mut control = armed();
        control.motion_companion = 9;
        begin_sheathing(&mut control);
        let combat = control.combat.unwrap();
        assert_eq!((control.living_state, control.motion_companion), (21, 0));
        assert_eq!((combat.flags, combat.cooldown), (0x03, 24));
    }
}

#[cfg(test)]
mod decision_tests {
    use super::*;
    use crate::script::{ScriptActorId, registry::ActorControlState};

    fn id(index: usize) -> ScriptActorId {
        ScriptActorId {
            generation: 1,
            index,
        }
    }

    fn with_target(target: Option<ScriptActorId>) -> ActorControlState {
        ActorControlState {
            combat: Some(CombatControlState {
                target,
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn a_valid_target_has_a_combat_record_does_not_target_itself_and_is_not_dead_and_killed() {
        assert!(combat_target_valid(&with_target(Some(id(2))), id(1)));
        assert!(combat_target_valid(&with_target(None), id(1)));
        assert!(
            !combat_target_valid(&with_target(Some(id(1))), id(1)),
            "self target"
        );
        assert!(
            !combat_target_valid(&ActorControlState::default(), id(1)),
            "no combat record"
        );
        let mut dead = with_target(None);
        dead.death_selector = 8;
        assert!(
            combat_target_valid(&dead, id(1)),
            "state 8 alone is still valid"
        );
        dead.killed = true;
        assert!(!combat_target_valid(&dead, id(1)), "state 8 and killed");
        let mut killed = with_target(None);
        killed.killed = true;
        assert!(
            combat_target_valid(&killed, id(1)),
            "killed alone is still valid"
        );
    }

    #[test]
    fn engagement_follows_the_ignore_and_long_sight_attributes() {
        let mut attributes = vec![0u8; 256];
        assert_eq!(engagement(&attributes), Engagement::InSightWithin(0x40000));
        attributes[123] = 1;
        assert_eq!(engagement(&attributes), Engagement::InSightWithin(0x80000));
        attributes[121] = 1;
        assert_eq!(engagement(&attributes), Engagement::Always);
    }
}

#[cfg(test)]
mod constructor_tests {
    use super::*;

    #[test]
    fn reinitialization_retains_allocation_but_resets_executor_state() {
        let mut attributes = [0; 256];
        let mut next = 7;
        assert!(CombatControlState::initialize(&attributes, None, &mut next).is_none());
        assert_eq!(next, 7);
        attributes[14] = 1;
        let mut record = CombatControlState::initialize(&attributes, None, &mut next).unwrap();
        record.decision.attack_state = 2;
        attributes[87] = 124;
        let record = CombatControlState::initialize(&attributes, Some(&record), &mut next).unwrap();
        assert_eq!(
            (
                record.allocation_index,
                next,
                record.decision.attack_state,
                record.taunt_period
            ),
            (7, 8, 0, 5)
        );
    }

    #[test]
    fn construction_copies_attributes_two_and_four_into_strength_and_armor() {
        let mut attributes = vec![0u8; 256];
        attributes[2] = 7;
        attributes[4] = 9;
        let combat = CombatControlState::from_attributes(&attributes);
        assert_eq!((combat.strength, combat.armor), (7, 9));
        let blank = CombatControlState::from_attributes(&[0u8; 256]);
        assert_eq!((blank.strength, blank.armor), (0, 0));
        // The decision values: aggression 50, pacing divisors 129 minus attributes 63, 62 and 58.
        attributes[63] = 9;
        attributes[62] = 29;
        attributes[58] = 100;
        let decision = CombatControlState::from_attributes(&attributes).decision;
        assert_eq!(
            (
                decision.aggression,
                decision.attack_pacing,
                decision.defence_pacing,
                decision.defence_counter,
                decision.attack_state,
                decision.near_state
            ),
            (50, 120, 100, 29, 0, 0)
        );
    }
}

#[cfg(test)]
mod timed_taunt_tests {
    use super::select_timed_combat_taunt;
    use cyrus_assets::script::CombatTaunt;

    #[test]
    fn timed_selection_consumes_fifteen_draws_and_uses_signed_pacing() {
        let records = [11u32, 22, 33].map(|label| CombatTaunt {
            trigger: 1,
            label: label.to_le_bytes(),
            animation: 0,
        });
        for (sum, period, expected) in [(0, -5, Some(2)), (1, 1, Some(0)), (1, 5, None)] {
            let mut draws = 0;
            let got = select_timed_combat_taunt(&records, 1, sum, period, &mut || {
                let result = [0, 16384, 32766][draws % 3];
                draws += 1;
                result
            })
            .unwrap();
            assert_eq!((got, draws), (expected, 15));
        }
        for (sum, period) in [(0, 0), (i32::MIN, -1)] {
            let mut draws = 0;
            let error = select_timed_combat_taunt(&records, 1, sum, period, &mut || {
                draws += 1;
                0
            })
            .unwrap_err();
            assert!(error.to_string().contains("invalid signed period"));
            assert_eq!(draws, 15);
        }
        let mut draws = 0;
        assert_eq!(
            select_timed_combat_taunt(&records, 3, 0, 0, &mut || {
                draws += 1;
                0
            })
            .unwrap(),
            None
        );
        assert_eq!(draws, 15);
    }

    #[test]
    fn one_past_draw_rejects_instead_of_clamping_or_inventing_a_record() {
        let records = [CombatTaunt {
            trigger: 1,
            label: *b"TEST",
            animation: 0,
        }];
        let mut draws = 0;
        let error = select_timed_combat_taunt(&records, 1, 0, 1, &mut || {
            draws += 1;
            32767
        })
        .unwrap_err();
        assert!(error.to_string().contains("one-past access unsupported"));
        assert_eq!(draws, 1);
        assert!(
            select_timed_combat_taunt(&[], 1, 0, 0, &mut || panic!("empty list draws"))
                .unwrap()
                .is_none()
        );
        assert!(
            select_timed_combat_taunt(&records, 0, 0, 1, &mut || panic!("ordered request draws"))
                .is_err()
        );
    }
}
