//! Combat state, taunt presentation services and class-message queues.
use super::ScriptActorId;
use crate::script::AiType;
use crate::script::LivingState;
use anyhow::{Context, Result};
use cyrus_assets::script::TauntType;
use cyrus_script::{Attribute, Attributes};
use serde::Serialize;

const QUEUE_CAPACITY: usize = 8;
/// Authored attack move slots, in selection order.
const ATTACK_SLOTS: [Attribute; 9] = [
    Attribute::AttackSingle,
    Attribute::AttackDouble,
    Attribute::AttackTriple,
    Attribute::AttackThrust,
    Attribute::AttackLunge,
    Attribute::AttackLow,
    Attribute::AttackJumpUp,
    Attribute::AttackJumpForward,
    Attribute::AttackDisarm,
];
/// Authored defense move slots, in selection order.
const DEFENSE_SLOTS: [Attribute; 4] = [
    Attribute::DefendLow,
    Attribute::DefendRight,
    Attribute::DefendLeft,
    Attribute::DefendHigh,
];

/// Attack and defence executor state (preservation SOUP.md "Attack executor"): a defence start
/// sets it to one, an attack start to two, and ending either clears it. Selected-player combat
/// processing reads it as its phase.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct ExecutorState(i32);

impl ExecutorState {
    pub const IDLE: Self = Self(0);
    pub const DEFENDING: Self = Self(1);
    pub const ATTACKING: Self = Self(2);

    pub const fn is_idle(self) -> bool {
        self.0 == Self::IDLE.0
    }
}

/// Retained values of the nonplayer decision stage, set by combat construction (see preservation's
/// engine/SOUP.md "Combat maintenance") and changed by the stage and its attack and defence helpers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct CombatDecision {
    /// Attack and defence executor state.
    pub attack_state: ExecutorState,
    /// First move-table selector (the attack list index of the current move).
    pub move_selector: i32,
    /// Aggression budget: starts at 50, refilled by the stage and spent by each attack.
    pub aggression: i32,
    /// Pacing divisors: 129 minus `attr_defense_skill` (attack) and `attr_attack_skill` (defence); the defence counter
    /// starts at 129 minus `attr_aggression`.
    pub attack_pacing: i32,
    pub defence_pacing: i32,
    pub defence_counter: i32,
}

impl Default for CombatDecision {
    fn default() -> Self {
        Self {
            attack_state: ExecutorState::IDLE,
            move_selector: 0,
            aggression: 50,
            attack_pacing: 0,
            defence_pacing: 0,
            defence_counter: 0,
        }
    }
}

impl CombatDecision {
    /// Combat construction: the pacing values are 129 minus `attr_defense_skill`, `attr_attack_skill` and `attr_aggression`.
    pub(crate) fn from_attributes(attributes: &Attributes) -> Self {
        Self {
            attack_pacing: 129 - i32::from(attributes[Attribute::DefenseSkill]),
            defence_pacing: 129 - i32::from(attributes[Attribute::AttackSkill]),
            defence_counter: 129 - i32::from(attributes[Attribute::Aggression]),
            ..Self::default()
        }
    }
}

/// Constructor-owned combat state, separate from animation and death-default selection.
#[derive(Debug, Serialize)]
pub struct CombatControlState {
    /// Authored attack indices; empty slots do not terminate selection.
    pub attack_indices: [Option<u8>; 9],
    /// Installed signed RAEX vertex and Q8 contact radius, independent of model bounds.
    pub contact_profile: cyrus_assets::animation::CombatContactProfile,
    /// Authored defense indices, ending at the first zero attribute.
    pub defense_indices: [Option<u8>; 4],
    pub target: Option<ScriptActorId>,
    /// Last target local Q8 position copied on fallback or successful engagement.
    pub retained_target_position: [i32; 3],
    /// Consecutive decision passes strictly below 25000 Q8 units, wrapping at 32 bits.
    pub close_distance_passes: i32,
    /// Lost-sight nominal ticks, compared before wrapping addition.
    pub lost_sight_ticks: i32,
    /// Nonzero after routed pursuit begins, cleared on reacquisition.
    pub lost_sight_routing: i32,
    pub flags: CombatFlags,
    /// Attack action selector: [`Self::NO_ACTION`], [`Self::STARTING_ACTION`], or 0 to 6 while an
    /// attack is processed.
    pub action: i32,
    pub cooldown: u32,
    /// World object-pool allocation index, retained across combat reinitialization.
    pub allocation_index: i32,
    /// Coordination selection state: [`Self::COORDINATION_ACTIVE`] takes the active branch and
    /// admits a candidate; other values, normally [`Self::COORDINATION_WAITING`], wait.
    pub attacker_selection_state: i32,
    /// Pending attack move, then defense attack-slot selector. Selection remains caller-owned.
    pub retained_action_indices: [i32; 2],
    /// Shared RTX cache index, distinct from the retained dialogue record.
    pub taunt_cache: Option<usize>,
    pub taunt_dialogue: Option<usize>,
    pub taunt_active: i32,
    /// Signed constructor-owned timed-taunt divisor: 129 minus `attr_chatty`.
    pub taunt_period: i32,
    /// Constructor near, far and averaged-midpoint ranges in runtime position units.
    pub spacing_ranges: [i32; 3],
    /// Signed spacing divisor: 129 minus `attr_twitch`.
    pub spacing_period: i32,
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
            contact_profile: Default::default(),
            defense_indices: [None; 4],
            target: None,
            retained_target_position: [0; 3],
            close_distance_passes: 0,
            lost_sight_ticks: 0,
            lost_sight_routing: 0,
            flags: CombatFlags::default(),
            action: Self::NO_ACTION,
            cooldown: 0,
            allocation_index: 0,
            attacker_selection_state: Self::COORDINATION_WAITING,
            retained_action_indices: [-1; 2],
            taunt_cache: None,
            taunt_dialogue: Some(0),
            taunt_active: 0,
            taunt_period: 0,
            spacing_ranges: [10000, 30000, 20000],
            spacing_period: 0,
            strength: 0,
            armor: 0,
            decision: CombatDecision::default(),
        }
    }
}

impl CombatControlState {
    /// No attack action; fight spacing runs only in this state.
    pub const NO_ACTION: i32 = -1;
    /// Set by the attack starter before the attack executor runs.
    pub const STARTING_ACTION: i32 = 9;
    // Actions 0 to 8 select the attack slot bound from DEF attributes 70 through 78
    // (`attr_attack_single` to `attr_attack_disarm`, preservation SOUP.md).
    pub const SINGLE_ACTION: i32 = 0;
    pub const DOUBLE_ACTION: i32 = 1;
    pub const TRIPLE_ACTION: i32 = 2;
    pub const THRUST_ACTION: i32 = 3;
    pub const LUNGE_ACTION: i32 = 4;
    pub const LOW_ACTION: i32 = 5;
    pub const JUMP_UP_ACTION: i32 = 6;
    pub const JUMP_FORWARD_ACTION: i32 = 7;
    pub const DISARM_ACTION: i32 = 8;
    /// Coordination state that waits for another actor to take its turn.
    pub const COORDINATION_WAITING: i32 = 0;
    /// Coordination state that takes the active attacking branch.
    pub const COORDINATION_ACTIVE: i32 = 1;

    /// Allocate only for a new combat record; reinitialization retains its index.
    pub(crate) fn initialize(
        attributes: &Attributes,
        previous: Option<&Self>,
        next_index: &mut i32,
        attacks: &[cyrus_assets::script::combat::Attack],
        contact_profile: cyrus_assets::animation::CombatContactProfile,
    ) -> Result<Option<Self>> {
        if attributes[Attribute::Combat] == 0 {
            return Ok(None);
        }
        let mut record = Self::from_attributes(attributes);
        record.spacing_ranges = record.derive_spacing_ranges(attacks)?;
        record.contact_profile = contact_profile;
        let allocation_index = previous.map_or_else(
            || {
                let index = *next_index;
                *next_index = next_index.wrapping_add(1);
                index
            },
            |record| record.allocation_index,
        );
        record.allocation_index = allocation_index;
        Ok(Some(record))
    }

    /// Aggregate authored slots using preservation's combat-construction rules.
    fn derive_spacing_ranges(
        &self,
        attacks: &[cyrus_assets::script::combat::Attack],
    ) -> Result<[i32; 3]> {
        let mut near = i32::MAX;
        let mut far = 0;
        let mut midpoint_sum = 0i32;
        let mut populated = 0;
        let mut matched = false;
        for (slot, index) in self.attack_indices.iter().enumerate() {
            let Some(index) = index else { continue };
            let attack = attacks.get(usize::from(*index)).with_context(|| {
                format!("combat spacing slot {slot}: attack{index:02} definition unavailable")
            })?;
            let motion = attack.motions.first().with_context(|| {
                format!("combat spacing slot {slot}: attack{index:02} first motion unavailable")
            })?;
            populated += 1;
            // The constructor tests the runtime motion record's first word, which is 1 for every
            // attack motion read from the original (COMBAT.INI types 0 and 3, alone or as a combo's
            // first move), not the INI type. The word for the other types is not observed, so those
            // slots stay counted but contribute no range (Unknown).
            if !matches!(motion.kind, 0 | 3) {
                continue;
            }
            matched = true;
            near = near.min(attack.min_range);
            if motion.max_range > far {
                far = attack.max_range;
            }
            let midpoint = attack
                .min_range
                .wrapping_add(attack.max_range.wrapping_sub(attack.min_range) / 2);
            midpoint_sum = midpoint_sum.wrapping_add(midpoint);
        }
        Ok(if matched {
            [near, far, midpoint_sum / populated]
        } else {
            [10000, 30000, 20000]
        })
    }

    /// Bind authored slots following preservation's COMBAT.INI actor move selection.
    pub(crate) fn from_attributes(attributes: &Attributes) -> Self {
        let attack_indices = std::array::from_fn(|slot| {
            let index = attributes[ATTACK_SLOTS[slot]];
            (index != 0).then_some(index)
        });
        let mut defense_indices = [None; 4];
        for (slot, index) in DEFENSE_SLOTS
            .map(|attribute| attributes[attribute])
            .into_iter()
            .enumerate()
        {
            if index == 0 {
                break;
            }
            defense_indices[slot] = Some(index);
        }
        Self {
            attack_indices,
            defense_indices,
            // The constructor copies `attr_strength` and `attr_armor` into the strength and armor values.
            strength: i32::from(attributes[Attribute::Strength]),
            armor: i32::from(attributes[Attribute::Armor]),
            taunt_period: 129 - i32::from(attributes[Attribute::Chatty]),
            spacing_period: 129 - i32::from(attributes[Attribute::Twitch]),
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
    trigger: TauntType,
    pacing_sum: i32,
    period: i32,
    draw: &mut dyn FnMut() -> i32,
) -> Result<Option<usize>> {
    anyhow::ensure!(
        !matches!(
            trigger,
            TauntType::BEGIN_COMBAT
                | TauntType::HURT_BY_OPPONENT
                | TauntType::DEATH
                | TauntType::UNARMED_MALE_OPPONENT
                | TauntType::UNARMED_FEMALE_OPPONENT
        ),
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
    /// Remembered cache sample, including a channel whose playback has stopped.
    fn remembered_sample(&self, cache_index: usize) -> Result<Option<u8>>;
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

/// An actor's combat flag byte. The engine leaves the bits unnamed; the names describe each bit's
/// documented use (preservation `docs/engine/SOUP.md`, combat sections).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct CombatFlags(u8);

impl CombatFlags {
    /// Raised when a defence move is picked or started; attack contact admission requires it.
    pub const MOVE_INTENT: Self = Self(0x01);
    /// Set by an accepted counter or player attack input; the hit-defence query requires it on the defender.
    pub const COUNTER_INTENT: Self = Self(0x02);
    /// Both intent bits, which messages and contact clear together.
    pub const INTENT: Self = Self(0x03);
    /// Combat stance: an actor is in combat stance when armed or when this bit is set.
    pub const STANCE: Self = Self(0x04);
    /// Toggled by each defended contact hit, cleared by a defended miss and at combat start.
    pub const DEFENDED_HIT_PARITY: Self = Self(0x08);
    /// Set on a counter's target. It blocks further counters, and the defence executor ends rather
    /// than step into an attack-end group.
    pub const ATTACKED: Self = Self(0x10);
    /// Set once an attack's collision window has admitted a contact test.
    pub const CONTACT_TESTED: Self = Self(0x20);
    /// Set when a want-to-switch-in taunt is presented; further such requests are ignored.
    pub const WANT_SWITCH_IN_PRESENTED: Self = Self(0x40);

    pub const fn from_bits(bits: u8) -> Self {
        Self(bits)
    }

    pub const fn bits(self) -> u8 {
        self.0
    }

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    pub fn insert(&mut self, other: Self) {
        self.0 |= other.0;
    }

    pub fn remove(&mut self, other: Self) {
        self.0 &= !other.0;
    }

    pub fn toggle(&mut self, other: Self) {
        self.0 ^= other.0;
    }
}

impl std::ops::BitOr for CombatFlags {
    type Output = Self;
    fn bitor(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

/// Logical identities retain sender and addressed receiver across queue operations.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct CombatMessage {
    pub sender: ScriptActorId,
    pub receiver: Option<ScriptActorId>,
    pub code: CombatMessageCode,
}

/// Combat message code. Zero marks an empty queue slot and is never sent; other codes outside
/// the named set are consumed without effect. The names describe each code's documented response.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct CombatMessageCode(i32);

impl CombatMessageCode {
    /// Empty queue slot.
    pub const NONE: Self = Self(0);
    /// Defender to attacker after a defended contact: displaces the sender and hands off coordination.
    pub const BLOCK: Self = Self(0xd000);
    /// Attacker to defender on contact: clears both actors' intent bits and runs the hit reaction.
    pub const HIT: Self = Self(0xd001);
    /// The receiver takes coordination state one and aggression 50.
    pub const RECEIVER_COORDINATES: Self = Self(0xd002);
    /// The sender takes coordination state one and aggression 50.
    pub const SENDER_COORDINATES: Self = Self(0xd003);
    /// Projectile parent to the hit actor: displaces the receiver and applies direct damage.
    pub const DIRECT_DAMAGE: Self = Self(0xd005);
}

impl From<i32> for CombatMessageCode {
    fn from(code: i32) -> Self {
        Self(code)
    }
}

impl From<CombatMessageCode> for i32 {
    fn from(code: CombatMessageCode) -> Self {
        code.0
    }
}

impl std::fmt::Display for CombatMessageCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:#x}", self.0)
    }
}

/// Generation no live actor carries; saved references to replaced worlds' actors restore under it.
pub(super) const DEPARTED_GENERATION: u64 = u64::MAX;

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
        if message.code == CombatMessageCode::NONE {
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

    /// Inspect the next addressed nonzero message without advancing the actor cursor.
    /// This lets production response dispatch choose a service-aware path before consuming.
    pub fn peek(&self, class: u8, actor: ScriptActorId, read: u8) -> Result<Option<CombatMessage>> {
        anyhow::ensure!(
            usize::from(read) < QUEUE_CAPACITY,
            "combat message class {class} actor {} read cursor {} outside queue",
            actor.index,
            read
        );
        let queue = &self.classes[usize::from(class)];
        let mut cursor = read;
        while cursor != queue.write {
            let Some(message) = queue.entries[usize::from(cursor)] else {
                return Ok(None);
            };
            cursor = (cursor + 1) % QUEUE_CAPACITY as u8;
            if message.receiver.is_none_or(|receiver| receiver == actor) {
                return Ok(Some(message));
            }
        }
        Ok(None)
    }

    /// `ReceiveMessage`: from the receiver's selection, skip entries addressed to another actor by
    /// advancing the selection, and return the value word (the code) of the first entry addressed
    /// to it or to no one; that entry stays selected. Zero when no selection remains. An entry
    /// never written has no recipient and a zero value, so it is accepted.
    pub fn receive(&self, class: u8, actor: ScriptActorId, read: &mut u8) -> Result<i32> {
        anyhow::ensure!(
            usize::from(*read) < QUEUE_CAPACITY,
            "combat message class {class} actor {} read cursor {} outside queue",
            actor.index,
            *read
        );
        let queue = &self.classes[usize::from(class)];
        while *read != queue.write {
            match queue.entries[usize::from(*read)] {
                None => return Ok(0),
                Some(message) if message.receiver.is_none_or(|receiver| receiver == actor) => {
                    return Ok(message.code.into());
                }
                Some(_) => *read = Self::next_selection(*read),
            }
        }
        Ok(0)
    }

    /// The selection after the one a receiver holds: one further, wrapping to zero at the queue's
    /// entry count.
    pub fn next_selection(read: u8) -> u8 {
        let next = read.wrapping_add(1);
        if usize::from(next) < QUEUE_CAPACITY {
            next
        } else {
            0
        }
    }

    /// The entry a receiver has selected: its read cursor in the class queue. The outer `None` is
    /// no selection (cursor equal to the write index); the inner `None` is a selected entry that
    /// was never written, whose sender pointer is null. The reference is returned as stored; it is
    /// not checked against the live world.
    pub fn selected_entry(&self, class: u8, read: u8) -> Result<Option<Option<ScriptActorId>>> {
        anyhow::ensure!(
            usize::from(read) < QUEUE_CAPACITY,
            "combat message class {class} read cursor {read} outside queue"
        );
        let queue = &self.classes[usize::from(class)];
        if read == queue.write {
            return Ok(None);
        }
        Ok(Some(
            queue.entries[usize::from(read)].map(|message| message.sender),
        ))
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
                                code: message.code.into(),
                            })
                        })
                        .collect(),
                })
                .collect(),
        }
    }

    /// Every retained reference is rebound, including entries behind a read cursor. The queues
    /// outlive world changes, so an entry can name an actor of a world generation that has since
    /// been replaced; it can never match a live actor, so it is restored under the departed
    /// marker generation, keeping its slot (an empty slot would stop reads instead of skipping).
    pub(super) fn from_snapshot(
        saved: CombatMessagesSnapshot,
        rebind: &impl Fn(u64, usize) -> Result<ScriptActorId>,
    ) -> Result<Self> {
        let rebind = |generation, index| {
            rebind(generation, index).unwrap_or(ScriptActorId {
                generation: DEPARTED_GENERATION,
                index,
            })
        };
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
                        sender: rebind(message.sender.0, message.sender.1),
                        receiver: message
                            .receiver
                            .map(|(generation, index)| rebind(generation, index)),
                        code: message.code.into(),
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
        && !control.living_state.is_bounce()
        && !drawing
        && !sheathing
        && holding
}

/// Enter the sheathing state: lifecycle byte 21, extra-time byte cleared, combat flag 0x04
/// cleared and a 24-tick cooldown.
pub(super) fn begin_sheathing(control: &mut super::registry::ActorControlState) {
    control.living_state = LivingState::SHEATHING_SWORD;
    control.extra_time = 0;
    let combat = control
        .combat
        .as_mut()
        .expect("eligible actor has combat state");
    combat.flags.remove(CombatFlags::STANCE);
    combat.cooldown = 24;
}

/// The unconditional part of `endCombat`: lifecycle selector 1, AI mode 0, hook enabled, combat flag
/// 0x04 cleared, close-distance pass counter and coordination selection state zeroed. Returns
/// false, changing nothing, when the actor has no combat record.
pub(super) fn end_combat_reset(control: &mut super::registry::ActorControlState) -> bool {
    let Some(combat) = control.combat.as_mut() else {
        return false;
    };
    combat.flags.remove(CombatFlags::STANCE);
    combat.close_distance_passes = 0;
    combat.attacker_selection_state = CombatControlState::COORDINATION_WAITING;
    control.ai_type = AiType::FIND_PLAYER;
    control.ai_mode = 0;
    control.script_control &= !2;
    true
}

/// Whether `control` is a valid combat target for the actor `own` (preservation's "Valid target"
/// rule): not killed while in lifecycle state 8, has a combat record, and does not target itself.
pub(super) fn combat_target_valid(
    control: &super::registry::ActorControlState,
    own: super::ScriptActorId,
) -> bool {
    !(control.ai_type == AiType::DEAD && control.killed)
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

/// `attr_combat_ignore_in_sight` and `attr_combat_long_sight`.
pub(super) fn engagement(attributes: &Attributes) -> Engagement {
    if attributes[Attribute::CombatIgnoreInSight] != 0 {
        Engagement::Always
    } else if attributes[Attribute::CombatLongSight] != 0 {
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
                flags: CombatFlags::from_bits(0x07),
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
        moving.living_state = LivingState::LANDED_ON_ACTOR;
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
    fn end_combat_resets_the_selector_mode_hook_flag_and_counters() {
        let mut control = ActorControlState {
            ai_type: AiType::COMBAT,
            ai_mode: 3,
            script_control: 0b1011,
            combat: Some(CombatControlState {
                flags: CombatFlags::from_bits(0x07),
                close_distance_passes: 12,
                attacker_selection_state: 5,
                cooldown: 9,
                ..Default::default()
            }),
            ..Default::default()
        };
        assert!(end_combat_reset(&mut control));
        let combat = control.combat.as_ref().unwrap();
        assert_eq!(combat.flags.bits(), 0x03);
        assert_eq!(combat.close_distance_passes, 0);
        assert_eq!(combat.attacker_selection_state, 0);
        // The cooldown belongs to sheathing, not to the reset.
        assert_eq!(combat.cooldown, 9);
        assert_eq!((control.ai_type, control.ai_mode), (AiType::FIND_PLAYER, 0));
        assert_eq!(control.script_control, 0b1001);
        let mut bare = ActorControlState {
            ai_type: AiType::COMBAT,
            ..Default::default()
        };
        assert!(!end_combat_reset(&mut bare));
        assert_eq!(bare.ai_type, AiType::COMBAT);
    }

    #[test]
    fn begin_sheathing_sets_state_clears_the_combat_flag_and_starts_the_cooldown() {
        let mut control = armed();
        control.motion_companion = 9;
        control.extra_time = 19;
        begin_sheathing(&mut control);
        let combat = control.combat.unwrap();
        assert_eq!(
            (
                control.living_state,
                control.motion_companion,
                control.extra_time
            ),
            (LivingState::SHEATHING_SWORD, 9, 0)
        );
        assert_eq!((combat.flags.bits(), combat.cooldown), (0x03, 24));
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
        dead.ai_type = AiType::DEAD;
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
        let mut attributes = Attributes::default();
        assert_eq!(engagement(&attributes), Engagement::InSightWithin(0x40000));
        attributes[Attribute::CombatLongSight] = 1;
        assert_eq!(engagement(&attributes), Engagement::InSightWithin(0x80000));
        attributes[Attribute::CombatIgnoreInSight] = 1;
        assert_eq!(engagement(&attributes), Engagement::Always);
    }
}

#[cfg(test)]
mod constructor_tests {
    use super::*;

    #[test]
    fn reinitialization_retains_allocation_but_resets_executor_state() {
        let mut attributes = Attributes::default();
        let mut next = 7;
        assert!(
            CombatControlState::initialize(&attributes, None, &mut next, &[], Default::default())
                .unwrap()
                .is_none()
        );
        assert_eq!(next, 7);
        attributes[Attribute::Combat] = 1;
        let mut record =
            CombatControlState::initialize(&attributes, None, &mut next, &[], Default::default())
                .unwrap()
                .unwrap();
        record.decision.attack_state = ExecutorState::ATTACKING;
        record.spacing_ranges = [-1; 3];
        record.spacing_period = -1;
        record.close_distance_passes = 123;
        record.lost_sight_ticks = -123;
        record.lost_sight_routing = 1;
        record.retained_target_position = [1, 2, 3];
        attributes[Attribute::Chatty] = 124;
        attributes[Attribute::Twitch] = 126;
        let record = CombatControlState::initialize(
            &attributes,
            Some(&record),
            &mut next,
            &[],
            Default::default(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            (
                record.allocation_index,
                next,
                record.decision.attack_state,
                record.taunt_period
            ),
            (7, 8, ExecutorState::IDLE, 5)
        );
        assert_eq!(record.spacing_ranges, [10000, 30000, 20000]);
        assert_eq!(record.spacing_period, 3);
        assert_eq!(record.close_distance_passes, 0);
        assert_eq!(record.lost_sight_ticks, 0);
        assert_eq!(record.lost_sight_routing, 0);
        assert_eq!(record.retained_target_position, [0; 3]);
    }

    #[test]
    fn constructor_spacing_matches_original_slot_aggregation() {
        use cyrus_assets::script::combat::{Attack, Motion};
        // Slots are [INI type, attack min, attack max, first-motion max]. Types 0 and 3 contribute
        // (the original's records for START's pirates and NECRTOWR's Vermai); type 4 is unobserved.
        let cases: &[(&[[i32; 4]], [i32; 3])] = &[
            (&[], [10000, 30000, 20000]),
            (&[[4, 10, 90, 90]], [10000, 30000, 20000]),
            (&[[0, 10, 90, 90]], [10, 90, 50]),
            (&[[3, 10, 90, 90]], [10, 90, 50]),
            (&[[0, 10, 90, 90], [4, 100, 200, 200]], [10, 90, 25]),
            (&[[0, 10, 90, 100], [0, 20, 200, 95]], [10, 200, 80]),
            (&[[0, -9, -2, 10]], [-9, -2, -6]),
            (
                &[[0, 2147483640, -2147483640, 2147483647]],
                [2147483640, -2147483640, i32::MIN],
            ),
            (
                &[[0, 2147483640, 2147483646, 2147483647]; 2],
                [2147483640, 2147483646, -5],
            ),
            // NECRTOWR's VERMAI01: attacks 75 (stab), 76 (stab) and 77 (melee), read back from the
            // original as [0, 55000, 27500].
            (
                &[
                    [3, 0, 55000, 55000],
                    [3, 0, 55000, 55000],
                    [0, 0, 55000, 55000],
                ],
                [0, 55000, 27500],
            ),
            // START's pirates: a combo of range 45000 whose first move reaches 35000, three single
            // attacks of 35000 and a second combo; the original holds [0, 45000, 19500].
            (
                &[
                    [0, 0, 45000, 35000],
                    [3, 0, 35000, 35000],
                    [0, 0, 35000, 35000],
                    [0, 0, 35000, 35000],
                    [0, 0, 45000, 35000],
                ],
                [0, 45000, 19500],
            ),
        ];
        for &(slots, expected) in cases {
            let mut attributes = Attributes::default();
            attributes[Attribute::Combat] = 1;
            attributes[Attribute::Twitch] = 127;
            let mut attacks = vec![Attack {
                is_combo: false,
                motions: vec![],
                min_range: 0,
                max_range: 0,
            }];
            for (slot, &[kind, min_range, max_range, motion_max]) in slots.iter().enumerate() {
                attributes[ATTACK_SLOTS[slot]] = attacks.len() as u8;
                let motion: Motion = serde_json::from_value(serde_json::json!({
                    "section":1,"kind":kind,"animation_group":0,
                    "min_range":0,"max_range":motion_max,
                    "first_collide_frame":0,"collide_duration":0
                }))
                .unwrap();
                attacks.push(Attack {
                    is_combo: false,
                    motions: vec![motion],
                    min_range,
                    max_range,
                });
            }
            let mut next = 0;
            let record = CombatControlState::initialize(
                &attributes,
                None,
                &mut next,
                &attacks,
                Default::default(),
            )
            .unwrap()
            .unwrap();
            assert_eq!(record.spacing_ranges, expected, "slots {slots:?}");
            assert_eq!(record.spacing_period, 2);
            assert_eq!(next, 1);
        }
        let mut attributes = Attributes::default();
        attributes[Attribute::Combat] = 1;
        attributes[Attribute::AttackSingle] = 9;
        let mut next = 5;
        let error =
            CombatControlState::initialize(&attributes, None, &mut next, &[], Default::default())
                .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("slot 0: attack09 definition unavailable")
        );
        assert_eq!(next, 5);
        let empty = cyrus_assets::script::combat::Attack {
            is_combo: true,
            motions: vec![],
            min_range: 0,
            max_range: 0,
        };
        let error = CombatControlState::initialize(
            &attributes,
            None,
            &mut next,
            &vec![empty; 10],
            Default::default(),
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("slot 0: attack09 first motion unavailable")
        );
        assert_eq!(next, 5);
    }

    #[test]
    fn construction_copies_attributes_two_and_four_into_strength_and_armor() {
        let mut attributes = Attributes::default();
        attributes[Attribute::Strength] = 7;
        attributes[Attribute::Armor] = 9;
        let combat = CombatControlState::from_attributes(&attributes);
        assert_eq!((combat.strength, combat.armor), (7, 9));
        let blank = CombatControlState::from_attributes(&Attributes::default());
        assert_eq!((blank.strength, blank.armor), (0, 0));
        // The decision values: aggression 50, pacing divisors 129 minus `attr_defense_skill`, `attr_attack_skill` and `attr_aggression`.
        attributes[Attribute::DefenseSkill] = 9;
        attributes[Attribute::AttackSkill] = 29;
        attributes[Attribute::Aggression] = 100;
        let decision = CombatControlState::from_attributes(&attributes).decision;
        assert_eq!(
            (
                decision.aggression,
                decision.attack_pacing,
                decision.defence_pacing,
                decision.defence_counter,
                decision.attack_state,
                decision.move_selector
            ),
            (50, 120, 100, 29, ExecutorState::IDLE, 0)
        );
    }
}

#[cfg(test)]
mod timed_taunt_tests {
    use super::select_timed_combat_taunt;
    use cyrus_assets::script::{CombatTaunt, TauntType};

    #[test]
    fn timed_selection_consumes_fifteen_draws_and_uses_signed_pacing() {
        let records = [11u32, 22, 33].map(|label| CombatTaunt {
            trigger: TauntType::ATTACK,
            label: label.to_le_bytes(),
            animation: 0,
        });
        for (sum, period, expected) in [(0, -5, Some(2)), (1, 1, Some(0)), (1, 5, None)] {
            let mut draws = 0;
            let got =
                select_timed_combat_taunt(&records, TauntType::ATTACK, sum, period, &mut || {
                    let result = [0, 16384, 32766][draws % 3];
                    draws += 1;
                    result
                })
                .unwrap();
            assert_eq!((got, draws), (expected, 15));
        }
        for (sum, period) in [(0, 0), (i32::MIN, -1)] {
            let mut draws = 0;
            let error =
                select_timed_combat_taunt(&records, TauntType::ATTACK, sum, period, &mut || {
                    draws += 1;
                    0
                })
                .unwrap_err();
            assert!(error.to_string().contains("invalid signed period"));
            assert_eq!(draws, 15);
        }
        let mut draws = 0;
        assert_eq!(
            select_timed_combat_taunt(&records, TauntType::HIT_OPPONENT, 0, 0, &mut || {
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
            trigger: TauntType::ATTACK,
            label: *b"TEST",
            animation: 0,
        }];
        let mut draws = 0;
        let error = select_timed_combat_taunt(&records, TauntType::ATTACK, 0, 1, &mut || {
            draws += 1;
            32767
        })
        .unwrap_err();
        assert!(error.to_string().contains("one-past access unsupported"));
        assert_eq!(draws, 1);
        assert!(
            select_timed_combat_taunt(&[], TauntType::ATTACK, 0, 0, &mut || panic!(
                "empty list draws"
            ))
            .unwrap()
            .is_none()
        );
        assert!(
            select_timed_combat_taunt(&records, TauntType::BEGIN_COMBAT, 0, 1, &mut || panic!(
                "ordered request draws"
            ))
            .is_err()
        );
    }
}

#[cfg(test)]
mod message_queue_tests {
    use super::*;

    fn id(index: usize) -> ScriptActorId {
        ScriptActorId {
            generation: 1,
            index,
        }
    }

    #[test]
    fn peek_preserves_cursor_and_skips_other_receivers() {
        let mut queue = CombatMessages::default();
        queue.send(
            7,
            CombatMessage {
                sender: id(2),
                receiver: Some(id(3)),
                code: CombatMessageCode::HIT,
            },
        );
        queue.send(
            7,
            CombatMessage {
                sender: id(2),
                receiver: Some(id(4)),
                code: CombatMessageCode::DIRECT_DAMAGE,
            },
        );
        let first = queue.peek(7, id(4), 0).unwrap().unwrap();
        assert_eq!(first.code, CombatMessageCode::DIRECT_DAMAGE);
        assert_eq!(queue.peek(7, id(4), 0).unwrap(), Some(first));
        assert_eq!(queue.consume(7, id(4), &mut 0).unwrap(), Some(first));
    }

    #[test]
    fn receive_skips_other_receivers_and_keeps_the_accepted_entry_selected() {
        let mut queue = CombatMessages::default();
        for (receiver, code) in [
            (Some(id(3)), CombatMessageCode::HIT),
            (Some(id(4)), CombatMessageCode::BLOCK),
        ] {
            queue.send(
                7,
                CombatMessage {
                    sender: id(2),
                    receiver,
                    code,
                },
            );
        }
        // An unaddressed entry is accepted by anyone, and a zero value is still a selection.
        queue.send(
            7,
            CombatMessage {
                sender: id(2),
                receiver: None,
                code: CombatMessageCode::from(0x55),
            },
        );
        let mut read = 0;
        assert_eq!(queue.receive(7, id(4), &mut read).unwrap(), 0xd000);
        assert_eq!(read, 1, "the skipped entry advanced the selection");
        assert_eq!(queue.receive(7, id(4), &mut read).unwrap(), 0xd000);
        assert_eq!(read, 1, "the accepted entry stays selected");
        let mut read = 0;
        assert_eq!(queue.receive(7, id(9), &mut read).unwrap(), 0x55);
        assert_eq!(read, 2);
        // The write index is the no-selection sentinel.
        let mut read = 3;
        assert_eq!(queue.receive(7, id(9), &mut read).unwrap(), 0);
        assert_eq!(read, 3);
    }

    #[test]
    fn a_saved_message_for_a_replaced_world_restores_as_a_skipped_entry() {
        let mut queue = CombatMessages::default();
        let departed = ScriptActorId {
            generation: 2,
            index: 19,
        };
        for receiver in [departed, id(4)] {
            queue.send(
                7,
                CombatMessage {
                    sender: id(2),
                    receiver: Some(receiver),
                    code: CombatMessageCode::HIT,
                },
            );
        }
        let live = |generation: u64, index: usize| {
            (generation == 1)
                .then_some(id(index))
                .ok_or_else(|| anyhow::anyhow!("departed"))
        };
        let restored = CombatMessages::from_snapshot(queue.snapshot(), &live).unwrap();
        let first = restored.classes[7].entries[0].unwrap();
        assert_eq!(first.receiver.unwrap().generation, DEPARTED_GENERATION);
        let mut read = 0;
        let delivered = restored.consume(7, id(4), &mut read).unwrap().unwrap();
        assert_eq!(delivered.receiver, Some(id(4)));
        assert_eq!(read, 2);
    }
}
