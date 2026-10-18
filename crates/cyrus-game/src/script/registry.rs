//! Logical script instances retain authored object order and reject stale world identities.
use cyrus_script::program::{Call, CallKind, Operation};
mod activation;
pub(super) mod body;
pub(super) mod bounds;
mod dialogue;
pub(super) mod falling;
mod follow;
pub(super) mod ground;
mod hazard;
mod late;
mod light;
pub(super) mod model;
pub(super) mod obstacle;
pub(super) mod ordinary;
pub(super) mod overlap;
mod persistence;
mod probe;
pub use persistence::{
    LogicalSessionCandidate, LogicalSessionRestoreInputs, LogicalSessionRestorePlan,
    LogicalSessionSnapshot, WorldRestoreContext, WorldSnapshot, WorldSnapshotSources,
};
pub(super) mod player;
pub(super) mod prevalidation;
pub(super) mod queued;
pub(super) mod recovery;
mod separate;
mod sight;
pub(super) mod slope;
pub(super) mod transition;
pub(super) mod vertical;
mod weapon;
use anyhow::{Context, Result};
use bevy::prelude::{Assets, Commands, DetectChanges, Handle, Res, ResMut, Resource};
use cyrus_assets::{
    animation::Animations,
    script::{CombatTaunt, Instance, Scripts},
};
use cyrus_script::{
    Argument, CallResult, CodeOrigin, Host, Route, Runtime, Target, TaskSource, Thread,
};
use cyrus_world::lifecycle::PreparedWorld;
use std::collections::BTreeMap;
use std::sync::Arc;
pub use weapon::{CombatEntryInputs, WeaponInputs};

/// Runtime identity is independent of render nodes and original object-name lookup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct ScriptActorId {
    pub generation: u64,
    pub index: usize,
}

/// Player and camera instances are independent of authored map placements.
#[derive(Debug, Clone)]
pub enum ScriptActorSource {
    Map(Instance),
    Player,
    AlternatePlayer,
    Camera,
}
impl ScriptActorSource {
    fn name(&self) -> &str {
        match self {
            Self::Map(placement) => &placement.actor,
            Self::Player => "CYRUS",
            Self::AlternatePlayer => "GREMLIN",
            Self::Camera => "CAMERA",
        }
    }
    fn label(&self) -> &'static str {
        match self {
            Self::Map(_) => "map",
            Self::Player => "player",
            Self::AlternatePlayer => "alternate_player",
            Self::Camera => "camera",
        }
    }
}

/// Explicit stages preserve separate animation, task and script-body invocations.
#[derive(Debug, Clone, Copy)]
pub enum ScriptStage {
    /// Caller has admitted the ordinary main stage; actor script-control flags still apply.
    OrdinaryMain,
    /// Separate-record main/task stage, without ordinary control/collision gates.
    SeparateMain,
    /// Separate-record native entry, gated only by pending unpaused tasks.
    SeparateHook,
    /// Caller has admitted this actor; task-system and hook-suppression gates still apply.
    OrdinaryHook,
    NativeHook,
    Animation,
    Entry,
    Tasks,
    Body(Thread),
}

/// Inputs supplied by the owning scheduler; missing values never imply a clock tick.
#[derive(Clone, Copy, Debug, Default)]
pub struct ScriptInputs<'a> {
    pub hud: Option<&'a super::ScriptHud>,
    /// Caller-established shell context for item operations; absence is unknown.
    pub shell_execution: Option<bool>,
    pub activation: Option<&'a super::ActivationInputs>,
    /// Held installed Activate binding for live logical-actor activation.
    pub activation_use_held: Option<bool>,
    pub nominal_ticks: Option<u32>,
    pub frame_scale: Option<u32>,
    pub player_query: Option<super::QueryInputs>,
    /// Explicit selected-player keyboard admission for the ordinary traversal.
    pub player_controller: Option<player::PlayerControllerInput<'a>>,
    pub ambient_sound: Option<&'a dyn cyrus_world::sound::AmbientSoundDriver>,
    /// Caller-admitted weapon and taunt gates for synchronous beginCombat.
    pub combat_entry: Option<CombatEntryInputs>,
    pub combat_taunts: Option<&'a dyn super::CombatTauntSource>,
    pub dialogue_source: Option<&'a dyn super::DialogueSource>,
    /// Explicit shared sample-reset flag from the owning engine update.
    pub sound_reset: Option<bool>,
    /// The blocked-ahead probe at distance 30, evaluated by the owning ordinary update for
    /// combat actors; required only by `MoveBackward` in combat stance.
    pub blocked_ahead_30: Option<bool>,
}

pub(crate) struct ScriptAnimation {
    pub(crate) playback: cyrus_world::animation::ActorAnimation,
    pub(crate) frame: Option<u32>,
}

/// Allocated ordinary-map and player control state, separate from animation playback.
/// Allocation and combat construction follow preservation's engine/animation.md.
/// Player role transitions and death/removal transitions remain unsupported.
#[derive(Debug, Default, serde::Serialize)]
pub struct ActorControlState {
    /// Runtime script flags, separate from installed attribute bytes.
    pub script_control: u8,
    /// Lifecycle selector: combat entry uses six; IsDead tests eight.
    pub death_selector: u8,
    /// Allocator-zero death notification byte, set to eight by health death.
    pub death_response: u8,
    /// Motion/weapon state, independent of the combat/death lifecycle selector.
    pub living_state: u8,
    /// Allocator-zero companion motion byte; weapon cleanup clears it with draw/sheath state.
    pub motion_companion: u8,
    /// Allocator-zeroed runtime motion bytes, independent of animation groups.
    pub sliding_state: u8,
    pub falling_state: u8,
    pub queued_motion: u8,
    pub motion_direction: u8,
    /// Retained movement-helper classification, distinct from displacement direction.
    pub motion_classification: u8,
    /// Allocator-zero directional-state bytes; low bits gate surface assignment.
    pub directional_state: [u8; 2],
    /// Retained directional motion bits, allocated as zero.
    pub movement_flags: u8,
    /// Retained integer motion components; obstacle interruption only clears them here.
    pub motion_vector: [i32; 3],
    /// Original fall origin in Q8 and byte countdown, allocated as zero.
    pub fall_origin_y: i32,
    pub fall_timer: u8,
    /// Allocator-zero runtime jump selector, separate from living and torch state.
    pub jump_selector: u8,
    /// Validity of the separately retained stable local position.
    pub stable_position_valid: bool,
    /// Separately retained stable local Q8 coordinates, allocated as zero.
    pub stable_local_position: [i32; 3],
    /// Accumulated sliding distance in Q8, cleared by ordinary jump entry.
    pub sliding_distance: i32,
    /// Ordered body response marks; cleared only by their owning update stage.
    pub body_response_flags: u8,
    pub collision_transition_flags: u8,
    /// Other bytes in the packed response record; retained until script-wrapper cleanup.
    pub collision_response_auxiliary: [u8; 2],
    /// Allocator-zero collision-response age, incremented before clearing prior marks.
    pub collision_response_age: u8,
    /// Original low-word overlap contact identifier; zero also denotes a static placement.
    pub overlap_contact_identifier: u16,
    pub rope_attached: bool,
    pub torch_burning: u8,
    pub weapon_drawn: bool,
    /// Low byte assigned by admitted HandItem; None leaves initial attachment state unestablished.
    pub held_item: Option<u8>,
    pub object_update_excluded: bool,
    /// Presentation hidden state; this does not exclude script or animation updates.
    pub hidden: bool,
    /// Set by `KillMe`, cleared by `Resurrect`; independent of the death selector.
    pub killed: bool,
    pub combat_death_default: Option<i32>,
    pub combat: Option<super::CombatControlState>,
}

pub struct ScriptActor {
    /// Allocator-zero class-message read cursor, independent of combat allocation.
    message_read: u8,
    pub source: ScriptActorSource,
    /// Signed low word used by namespace lookup; absent for separate map object paths.
    pub runtime_identifier: Option<i16>,
    pub namespace: Option<String>,
    native_hook: Option<Call>,
    combat_taunts: Vec<CombatTaunt>,
    runtime: Option<Runtime>,
    unavailable: Option<String>,
    attributes: Vec<u8>,
    control: Option<ActorControlState>,
    surface: Option<u8>,
    model: model::ModelState,
    collision: Option<ground::OrdinaryCollisionShape>,
    player_movement: Option<player::PlayerMovement>,
    light: Option<light::LightState>,
    pub(super) location_offsets: Vec<[i32; 3]>,
    tasks: super::tasks::NativeTasks,
    animation: Option<ScriptAnimation>,
    animation_failure: Option<String>,
    update_failure: Option<String>,
}
impl ScriptActor {
    pub fn name(&self) -> &str {
        self.source.name()
    }
    fn ordinary_collision_role(&self) -> bool {
        matches!(self.source, ScriptActorSource::Player)
            || self
                .placement()
                .is_some_and(|placement| placement.object_type == 1)
    }

    pub fn placement(&self) -> Option<&Instance> {
        match &self.source {
            ScriptActorSource::Map(placement) => Some(placement),
            _ => None,
        }
    }
    pub fn runtime(&self) -> Option<&Runtime> {
        self.runtime.as_ref()
    }
    /// Prepared attribute bytes, including immediate script writes and backup copies.
    pub fn attributes(&self) -> &[u8] {
        &self.attributes
    }
    /// Retained ordinary-map/player allocation state; separate records have no persistent control.
    pub fn control_state(&self) -> Option<&ActorControlState> {
        self.control.as_ref()
    }
    pub(super) fn bound_animation_group(&self) -> Option<Option<u16>> {
        self.animation
            .as_ref()
            .map(|animation| animation.playback.current_group())
    }
    /// Sticky script or animation failure; failed effects are never retried.
    pub fn failure(&self) -> Option<&str> {
        self.update_failure
            .as_deref()
            .or_else(|| self.animation_failure())
            .or_else(|| self.runtime.as_ref().and_then(Runtime::failure))
    }
    /// Retained animation construction or advancement failure, separate from SOUP errors.
    pub fn animation_failure(&self) -> Option<&str> {
        self.animation_failure.as_deref().or_else(|| {
            self.animation
                .as_ref()
                .and_then(|animation| animation.playback.failure())
        })
    }
    /// Whether a caller has initialized or explicitly bound animation playback.
    pub fn animation_initialized(&self) -> bool {
        self.animation.is_some()
    }
    pub fn unavailable(&self) -> Option<&str> {
        self.unavailable.as_deref()
    }
}

/// Logical world state shared by inspection and camera-centered ordinary updates.
pub struct ScriptWorld {
    pub world: u32,
    pub content_id: String,
    generation: u64,
    flags: Vec<i32>,
    flag_state: super::flags::Flags,
    updates: cyrus_assets::script::UpdateSettings,
    player_distance_override: bool,
    actors: Vec<ScriptActor>,
    spatial: Option<super::spatial::SpatialWorld>,
    activation: super::ActivationState,
    globals: super::GameplayGlobals,
    task_calls: BTreeMap<(CodeOrigin, u32), Call>,
    combat_attacks: Vec<cyrus_assets::script::combat::Attack>,
    in_combat_threshold: Option<i32>,
    attribute_names: Vec<String>,
    reference_names: Vec<String>,
    names: BTreeMap<i16, usize>,
    actor_lists: Option<super::order::ActorLists>,
    markers: Option<super::markers::Markers>,
    walk_nodes: Option<super::navigation::WalkNodeReservations>,
    ordinary_prepared: bool,
    model_admission: Option<model::AdmissionState>,
    map_updates: Vec<usize>,
    /// Shared surface-query veto; absent until an assigning query establishes it.
    surface_veto: Option<bool>,
}
impl ScriptWorld {
    /// Resolve a retained zero-based actor attack slot against the shared installed table.
    /// Empty authored slots return `None`; missing initialization or definitions are errors.
    pub fn combat_attack(
        &self,
        id: ScriptActorId,
        slot: usize,
    ) -> Result<Option<&cyrus_assets::script::combat::Attack>> {
        let actor = &self.actors[self.resolve(id)?];
        let context = || format!("world {} actor {} attack slot {slot}", self.world, id.index);
        let combat = actor
            .control
            .as_ref()
            .and_then(|control| control.combat.as_ref())
            .with_context(|| format!("{}: combat uninitialized", context()))?;
        let index = combat
            .attack_indices
            .get(slot)
            .with_context(|| format!("{}: slot outside authored list", context()))?;
        index
            .map(|index| {
                self.combat_attacks
                    .get(usize::from(index))
                    .with_context(|| {
                        format!("{}: attack{index:02} definition unavailable", context())
                    })
            })
            .transpose()
    }

    /// Send an addressed combat message using the receiver's authored class.
    /// A zero code has no effect; nonzero messages require live logical identities.
    pub fn send_combat_message(
        &mut self,
        sender: ScriptActorId,
        receiver: ScriptActorId,
        code: i32,
    ) -> Result<()> {
        if code == 0 {
            return Ok(());
        }
        self.resolve(sender)?;
        let index = self.resolve(receiver)?;
        self.globals.combat_messages.send(
            self.actors[index].attributes[125],
            super::CombatMessage {
                sender,
                receiver: Some(receiver),
                code,
            },
        );
        Ok(())
    }

    /// Consume one addressed message using the actor's retained class cursor.
    /// This is message transport; combat effects belong to the maintenance stage.
    pub fn consume_combat_message(
        &mut self,
        actor: ScriptActorId,
    ) -> Result<Option<super::CombatMessage>> {
        let index = self.resolve(actor)?;
        let receiver = &mut self.actors[index];
        self.globals.combat_messages.consume(
            receiver.attributes[125],
            actor,
            &mut receiver.message_read,
        )
    }

    /// Maintain an admitted ordinary combat record using shared nominal ticks.
    /// Targeting, messages, decision state and attacks retain unsupported guards.
    /// The owner supplies the selected player's drawing/sheathing and shell gates.
    pub fn maintain_map_combat(
        &mut self,
        id: ScriptActorId,
        ticks: u32,
        player_weapon_transition: bool,
        shell_execution: bool,
    ) -> Result<()> {
        let index = self.resolve(id)?;
        anyhow::ensure!(
            self.actors[index].placement().is_some()
                || matches!(self.actors[index].source, ScriptActorSource::Player),
            "ordinary combat requires a map actor or selected player"
        );
        if shell_execution {
            return Ok(());
        }
        let actor = &self.actors[index];
        let control = actor
            .control
            .as_ref()
            .context("ordinary combat control unavailable")?;
        let Some(combat) = control.combat.as_ref() else {
            return Ok(());
        };
        let selected = matches!(actor.source, ScriptActorSource::Player);
        if selected
            && self
                .globals
                .selected_combat_inhibition
                .context("selected-player combat entry inhibition unavailable")?
                != 0
        {
            return Ok(());
        }
        if !selected && control.death_selector == 8 {
            return Ok(());
        }
        anyhow::ensure!(
            selected
                || actor.attributes[116] != 0
                || combat.flags & 4 == 0
                || combat.target.is_none(),
            "ordinary combat target/perception stage unsupported"
        );
        self.actors[index].attributes[1] = self.actors[index].attributes[119];
        if let Some(message) = self.consume_combat_message(id)? {
            anyhow::bail!(
                "ordinary combat message {} from actor {} unsupported",
                message.code,
                message.sender.index
            );
        }
        let control = self.actors[index]
            .control
            .as_mut()
            .expect("admitted control");
        anyhow::ensure!(
            selected || control.death_selector != 6,
            "ordinary combat decision stage unsupported"
        );
        let combat = control.combat.as_mut().expect("admitted combat");
        if selected {
            anyhow::ensure!(
                combat.maintenance_phase == 0,
                "selected-player combat nonzero maintenance phase unsupported"
            );
            if combat.action != 0 {
                combat.action = -1;
                combat.retained_action_indices = [-1; 2];
            }
            // Both maintenance helpers with action -1 are inert in phase zero.
        }
        if !player_weapon_transition {
            if combat.cooldown != 0 {
                combat.cooldown = combat.cooldown.saturating_sub(ticks);
            } else if combat.action != -1 {
                anyhow::ensure!(
                    !(0..=6).contains(&combat.action),
                    "ordinary combat attack stage unsupported"
                );
                combat.action = -1;
            }
        }
        Ok(())
    }

    pub fn prepare(scripts: &Scripts, generation: u64) -> Result<Self> {
        scripts.validate()?;
        let programs: BTreeMap<_, _> = scripts
            .actors
            .iter()
            .map(|(name, actor)| (name.as_str(), Arc::new(actor.program.clone())))
            .collect();
        let mut task_calls = BTreeMap::new();
        for actor in scripts.actors.values() {
            for (&instruction, step) in &actor.program.steps {
                if let Operation::Call { call } | Operation::ObjectCall { call } = &step.operation {
                    let key = (CodeOrigin(actor.code_origin), instruction);
                    if let Some(existing) = task_calls.insert(key, call.clone()) {
                        anyhow::ensure!(
                            existing == *call,
                            "world {}: aliased script call differs at origin {} instruction {instruction}",
                            scripts.world,
                            actor.code_origin
                        );
                    }
                }
            }
        }
        let mut allocations = BTreeMap::<String, usize>::new();
        let mut allocate = |source: ScriptActorSource,
                            runtime_identifier: Option<i16>,
                            index: usize| {
            let name = source.name();
            let context = format!(
                "world {} instance {index} actor {name} script",
                scripts.world
            );
            let metadata = scripts.actors.get(name);
            let result = (|| {
                let actor = metadata.context("program absent")?;
                let allocated = allocations.entry(name.to_owned()).or_default();
                let locals = actor
                    .instance_variables
                    .get(*allocated)
                    .context("authored instance allocation exceeded")?
                    .clone();
                *allocated += 1;
                Runtime::from_shared(
                    Arc::clone(&programs[name]),
                    CodeOrigin(actor.code_origin),
                    locals,
                    context.clone(),
                )
            })();
            let (runtime, unavailable) = match result {
                Ok(runtime) => (Some(runtime), None),
                Err(error) => (None, Some(format!("{context}: {error:#}"))),
            };
            let control = match &source {
                ScriptActorSource::Map(placement) if !matches!(placement.object_type, 2 | 6) => {
                    Some(ActorControlState::default())
                }
                ScriptActorSource::Player | ScriptActorSource::AlternatePlayer => {
                    Some(ActorControlState::default())
                }
                _ => None,
            };
            let separate = matches!(&source, ScriptActorSource::Map(placement) if matches!(placement.object_type, 2 | 6));
            let light = match &source {
                ScriptActorSource::Map(placement) if placement.object_type == 6 => {
                    Some(light::LightState::authored(placement))
                }
                _ => None,
            };
            ScriptActor {
                light,
                message_read: 0,
                surface: (control.is_some()
                    || matches!(
                        &source,
                        ScriptActorSource::Player | ScriptActorSource::AlternatePlayer
                    ))
                .then_some(0),
                source,
                control,
                runtime_identifier,
                namespace: metadata.and_then(|actor| actor.namespace.clone()),
                native_hook: metadata.and_then(|actor| actor.native_hook.clone()),
                combat_taunts: metadata
                    .map(|actor| actor.combat_taunts.clone())
                    .unwrap_or_default(),
                runtime,
                unavailable,
                location_offsets: metadata
                    .map(|actor| actor.location_offsets.clone())
                    .unwrap_or_default(),
                tasks: Default::default(),
                animation: None,
                model: Default::default(),
                collision: None,
                player_movement: None,
                animation_failure: None,
                update_failure: None,
                attributes: (0..256)
                    .map(|index| {
                        (!separate)
                            .then_some(metadata)
                            .flatten()
                            .and_then(|actor| {
                                let key = scripts
                                    .definition
                                    .attributes
                                    .get(index)
                                    .filter(|name| !name.is_empty())
                                    .cloned()
                                    .unwrap_or_else(|| format!("attr_{index}"));
                                actor.attributes.get(&key).copied()
                            })
                            .unwrap_or(0)
                    })
                    .collect(),
            }
        };
        let mut counter = scripts.actor_pool_capacity.wrapping_add(3);
        let mut actors = Vec::new();
        for (index, placement) in scripts.instances.iter().enumerate() {
            let identifier = if matches!(placement.object_type, 2 | 6) {
                None
            } else {
                if placement.active != 0 {
                    counter = counter.wrapping_add(1);
                }
                counter = counter.wrapping_add(1);
                Some(counter as i16)
            };
            actors.push(allocate(
                ScriptActorSource::Map(placement.clone()),
                identifier,
                index,
            ));
        }
        for (name, source, identifier) in [
            ("CYRUS", ScriptActorSource::Player, 1),
            ("GREMLIN", ScriptActorSource::AlternatePlayer, 1),
            ("CAMERA", ScriptActorSource::Camera, 2),
        ] {
            if scripts.actors.contains_key(name) {
                let index = actors.len();
                actors.push(allocate(source, Some(identifier), index));
            }
        }
        let mut names = BTreeMap::new();
        for (index, actor) in actors.iter().enumerate() {
            if let Some(identifier) = actor
                .runtime_identifier
                .filter(|_| actor.namespace.is_some())
            {
                names.entry(identifier).or_insert(index);
            }
        }
        let mut globals = super::GameplayGlobals::default();
        globals.prepare_dialogue(&scripts.dialogue)?;
        Ok(Self {
            map_updates: Vec::new(),
            updates: scripts.updates,
            player_distance_override: scripts.player_distance_override,
            world: scripts.world,
            content_id: scripts.content_id.clone(),
            generation,
            flag_state: super::flags::Flags::prepare(&scripts.definition)
                .with_context(|| format!("world {} global flag initialization", scripts.world))?,
            flags: scripts
                .definition
                .flags
                .iter()
                .map(|flag| flag.initial_value)
                .collect(),
            actors,
            task_calls,
            combat_attacks: scripts.combat_attacks.clone(),
            in_combat_threshold: scripts.in_combat_threshold,
            attribute_names: scripts.definition.attributes.clone(),
            reference_names: scripts.definition.references.clone(),
            names,
            actor_lists: None,
            markers: None,
            walk_nodes: None,
            ordinary_prepared: false,
            surface_veto: None,
            model_admission: None,
            spatial: None,
            activation: Default::default(),
            globals,
        })
    }
    /// Prepare ordinary-world markers from the matching sidecar before script execution.
    /// Preparation is atomic and cannot replace marker state within a world generation.
    pub fn prepare_markers(&mut self, data: &Animations) -> Result<()> {
        data.require_version()?;
        anyhow::ensure!(
            data.world == self.world
                && data.metadata["script_content_id"].as_str() == Some(&self.content_id),
            "world {} marker/script cache generation mismatch; re-export the world",
            self.world
        );
        anyhow::ensure!(
            self.markers.is_none(),
            "world {} markers already prepared",
            self.world
        );
        self.markers = Some(
            super::markers::Markers::prepare(&data.metadata)
                .with_context(|| format!("world {} marker preparation", self.world))?,
        );
        Ok(())
    }

    /// Load cleared navigation reservations from the matching animation sidecar.
    /// Nonempty maps remain explicit unsupported movement; an absent table is an error.
    pub fn prepare_navigation(&mut self, data: &Animations) -> Result<()> {
        data.require_version()?;
        anyhow::ensure!(
            data.world == self.world
                && data.metadata["script_content_id"].as_str() == Some(&self.content_id),
            "world {} navigation/script cache generation mismatch; re-export the world",
            self.world
        );
        anyhow::ensure!(
            self.walk_nodes.is_none(),
            "world {} navigation already prepared",
            self.world
        );
        self.walk_nodes = Some(super::navigation::WalkNodeReservations::prepare(
            &data.metadata["walk_node_maps"],
            self.world,
        )?);
        Ok(())
    }

    /// Acquire a loaded WDNM reservation using the actor identifier's low byte.
    /// A foreign owner returns false; stale actors and invalid map/node indices fail.
    pub fn reserve_walk_node(
        &mut self,
        actor: ScriptActorId,
        map: usize,
        node: usize,
    ) -> Result<bool> {
        let index = self.resolve(actor)?;
        let identifier = self.actors[index]
            .runtime_identifier
            .context("walk-node actor identifier unavailable")?;
        self.walk_nodes
            .as_mut()
            .context("walk-node navigation not prepared")?
            .acquire(map, node, identifier)
            .with_context(|| {
                format!(
                    "world {} actor {} reserve navigation",
                    self.world, actor.index
                )
            })
    }

    /// Release only a reservation whose byte still matches this actor's identifier.
    pub fn release_walk_node(
        &mut self,
        actor: ScriptActorId,
        map: usize,
        node: usize,
    ) -> Result<()> {
        let index = self.resolve(actor)?;
        let identifier = self.actors[index]
            .runtime_identifier
            .context("walk-node actor identifier unavailable")?;
        self.walk_nodes
            .as_mut()
            .context("walk-node navigation not prepared")?
            .release(map, node, identifier)
            .with_context(|| {
                format!(
                    "world {} actor {} release navigation",
                    self.world, actor.index
                )
            })
    }

    /// Complete the navigation stage of sound maintenance across every loaded map.
    pub fn clear_walk_node_reservations(&mut self) -> Result<()> {
        self.walk_nodes
            .as_mut()
            .context("walk-node navigation not prepared")?
            .reset();
        Ok(())
    }

    /// Consume movement registrations before regional selection, in first-registration order.
    /// View coordinates use Q8 units. The caller supplies the original VM map-exclusion state.
    /// Excluded passes consume registrations without changing cell membership.
    pub fn flush_map_updates(&mut self, view: [i32; 3], excluded: bool) -> Result<()> {
        if self.map_updates.is_empty() {
            return Ok(());
        }
        if self.actor_lists.is_none() {
            self.actor_lists = Some(super::order::ActorLists::new(&self.actors)?);
        }
        let pending = std::mem::take(&mut self.map_updates);
        if excluded {
            return Ok(());
        }
        let spatial = self
            .spatial
            .as_ref()
            .context("deferred movement spatial state unavailable")?;
        let lists = self.actor_lists.as_mut().expect("actor lists prepared");
        for index in pending {
            let actor = &self.actors[index];
            if actor
                .placement()
                .is_none_or(|placement| placement.active != 0)
            {
                continue;
            }
            let pose = spatial.actor(index)?;
            lists
                .relocate(index, pose.local.position, pose.linked_position, view)
                .with_context(|| {
                    format!(
                        "world {} instance {index} actor {} deferred relocation",
                        self.world,
                        actor.name()
                    )
                })?;
        }
        Ok(())
    }

    /// Normal fresh-world update-list membership, before runtime actor exclusions.
    /// View coordinates use integer map units. Cell and persistent lists retain
    /// reverse creation order; the 18-cell window omits its corners and excludes
    /// attribute-113 actors on its outer border. The selection is retained until
    /// the starting cell changes, following preservation's `engine/animation.md`.
    /// Dynamic allocation, removal and cell migration require separate lifecycle support.
    pub fn ordinary_actors(&mut self, view: [i32; 2]) -> Result<&[ScriptActorId]> {
        if self.actor_lists.is_none() {
            self.actor_lists = Some(
                super::order::ActorLists::new(&self.actors)
                    .with_context(|| format!("world {} ordinary actor lists", self.world))?,
            );
        }
        self.actor_lists
            .as_mut()
            .expect("actor lists prepared")
            .select(&self.actors, self.generation, view)
            .with_context(|| format!("world {} ordinary actor list", self.world))
    }

    /// Prepare ordinary map poses and unique attachment families transactionally.
    /// Separate map records retain position with cleared temporary orientation;
    /// standalone roles still require their own spatial owners.
    pub fn prepare_spatial(&mut self) -> Result<()> {
        anyhow::ensure!(
            self.spatial.is_none(),
            "world {} spatial state already prepared",
            self.world
        );
        self.spatial = Some(
            super::spatial::SpatialWorld::prepare(&self.actors)
                .with_context(|| format!("world {} spatial preparation", self.world))?,
        );
        Ok(())
    }

    /// Initialize a standalone player role's authoritative pose once per world generation.
    /// The caller supplies established entry coordinates in Q8 and wrapped angles.
    /// This does not settle ground, initialize animation, or run the controller.
    pub fn initialize_player_spatial(
        &mut self,
        id: ScriptActorId,
        position: [i32; 3],
        angles: [u16; 3],
    ) -> Result<()> {
        let index = self.resolve(id)?;
        let actor = &self.actors[index];
        anyhow::ensure!(
            matches!(
                actor.source,
                ScriptActorSource::Player | ScriptActorSource::AlternatePlayer
            ),
            "world {} instance {index}: spatial player initialization requires a player role",
            self.world
        );
        self.spatial
            .as_mut()
            .context("spatial state not prepared")?
            .initialize_player(index, actor, position, angles)
            .with_context(|| {
                format!(
                    "world {} instance {index}: player spatial initialization",
                    self.world
                )
            })
    }

    /// Select an ordered combat taunt for an already admitted invocation.
    /// Supports begin-combat, hurt, death and opponent-unarmed types, following
    /// preservation config/combat-ini.md. Each matching nonzero label consumes
    /// one shared random draw. Death alone falls back to hurt when none matched.
    /// The caller owns eligibility and prepared presentation dependencies; this
    /// does not enter combat or admit the generic timed selection path.
    pub fn select_ordered_combat_taunt(
        &self,
        id: ScriptActorId,
        trigger: u8,
        random: &mut crate::random::EngineRandom,
    ) -> Result<Option<&CombatTaunt>> {
        let index = self.resolve(id)?;
        anyhow::ensure!(
            matches!(trigger, 0 | 2 | 6 | 10 | 11),
            "world {} instance {index}: combat taunt type {trigger} requires timed selection",
            self.world
        );
        Ok(select_combat_taunt(&self.actors[index], trigger, random))
    }

    /// Read stored actor surface state; unsupported object paths have no established value.
    pub fn actor_surface(&self, id: ScriptActorId) -> Result<u8> {
        let index = self.resolve(id)?;
        self.actors[index].surface.with_context(|| {
            format!(
                "world {} instance {index} actor {} surface state unavailable",
                self.world,
                self.actors[index].name()
            )
        })
    }

    /// Current selected-player item gates from the logical actor, never presentation defaults.
    /// Player switching and prompt lineup scheduling remain separate unsupported services.
    pub fn player_activation_state(&self) -> Result<(ScriptActorId, super::ActivationActor)> {
        let index = self
            .actors
            .iter()
            .position(|actor| matches!(actor.source, ScriptActorSource::Player))
            .context("selected player absent")?;
        let actor = &self.actors[index];
        let control = actor
            .control
            .as_ref()
            .context("selected player control state unavailable")?;
        let id = self.id(index)?;
        Ok((
            id,
            super::ActivationActor {
                state: control.living_state,
                on_rope: control.rope_attached,
                water_state: self.actor_surface(id)?,
                torch_burning: control.torch_burning,
                weapon_assigned: control.combat_death_default.is_some(),
                weapon_drawn: control.weapon_drawn,
                animation: actor
                    .bound_animation_group()
                    .map(|group| super::ActivationAnimation { group }),
            },
        ))
    }

    /// Commit the actor driver's stored surface, including assignments before a failed transition.
    /// SOUP water predicates consume this value without querying terrain again.
    pub fn set_actor_surface(&mut self, id: ScriptActorId, surface: u8) -> Result<()> {
        let index = self.resolve(id)?;
        self.actors[index].surface = Some(surface);
        Ok(())
    }

    /// Authoritative local and composed pose for this generation's actor.
    pub fn spatial(&self, id: ScriptActorId) -> Result<&super::ActorSpatial> {
        let index = self.resolve(id)?;
        self.spatial
            .as_ref()
            .context("spatial state not prepared")?
            .actor(index)
    }

    /// Snapshot local position and angles at the explicit ordinary actor update boundary.
    pub fn snapshot_spatial(&mut self, id: ScriptActorId) -> Result<()> {
        let index = self.resolve(id)?;
        self.spatial
            .as_mut()
            .context("spatial state not prepared")?
            .actor_mut(index)?
            .snapshot();
        Ok(())
    }

    /// Restore the current attribute-controlled axes after this actor's movement/script work.
    /// Matrix and family refresh remain separate later stages.
    pub fn restore_axis_locks(&mut self, id: ScriptActorId) -> Result<()> {
        let index = self.resolve(id)?;
        self.spatial
            .as_mut()
            .context("spatial state not prepared")?
            .actor_mut(index)?
            .restore_axis_locks(&self.actors[index].attributes)
            .with_context(|| format!("world {} instance {index} axis locks", self.world))
    }

    /// Refresh an unattached actor matrix without consuming its angle snapshot.
    pub fn refresh_transform(&mut self, id: ScriptActorId) -> Result<()> {
        let index = self.resolve(id)?;
        self.spatial
            .as_mut()
            .context("spatial state not prepared")?
            .actor_mut(index)?
            .refresh_transform()
    }

    /// Explicit family pass; does not infer timing or actor update eligibility.
    pub fn refresh_families(&mut self) -> Result<()> {
        self.spatial
            .as_mut()
            .context("spatial state not prepared")?
            .refresh_families()
            .with_context(|| format!("world {} attachment refresh", self.world))
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }
    /// Prepares one actor's animation without granting update eligibility.
    /// The default group is supplied by an established caller, not inferred from placement.
    pub fn bind_animation(
        &mut self,
        id: ScriptActorId,
        data: &Animations,
        default: u16,
    ) -> Result<()> {
        let index = self.validate_animation_binding(id, data)?;
        let playback = cyrus_world::animation::ActorAnimation::from_asset(
            data,
            self.actors[index].name(),
            default,
        )?;
        self.actors[index].animation = Some(ScriptAnimation {
            playback,
            frame: None,
        });
        Ok(())
    }

    /// Initialize an ordinary placed animation-table model using shared nominal ticks.
    /// This does not grant update eligibility or initialize standalone player roles.
    /// Sound failures are sticky because already-dispatched effects cannot be rolled back.
    pub fn initialize_map_animation(
        &mut self,
        id: ScriptActorId,
        data: &Animations,
        ticks: u32,
        sounds: &mut dyn FnMut(ScriptActorId, super::ScriptSound) -> Result<()>,
    ) -> Result<()> {
        let index = self.validate_animation_binding(id, data)?;
        let position = animation_sound_position(self.spatial.as_ref(), index);
        let actor = &mut self.actors[index];
        let placement = actor
            .placement()
            .context("animation initialization requires a map actor")?;
        anyhow::ensure!(
            placement.object_type == 1,
            "world {} instance {index}: animation initialization requires map object type 1",
            self.world
        );
        anyhow::ensure!(
            data.actor(actor.name())?["model_load_path"].as_str() == Some("animation_table"),
            "world {} instance {index}: actor does not use an animation model table",
            self.world
        );
        let result = cyrus_world::animation::ActorAnimation::initialize(
            data,
            actor.name(),
            ticks,
            &mut |sound| {
                sounds(
                    id,
                    super::ScriptSound::Animation {
                        sound,
                        surface: actor.surface,
                        position,
                    },
                )
            },
        );
        match result {
            Ok((playback, frame)) => {
                actor
                    .control
                    .as_mut()
                    .expect("type-one map actor has allocation state")
                    .combat_death_default = (actor.attributes[14] != 0).then_some(0);
                actor
                    .control
                    .as_mut()
                    .expect("allocated map control")
                    .combat = (actor.attributes[14] != 0)
                    .then(|| super::CombatControlState::from_attributes(&actor.attributes));
                actor.animation = Some(ScriptAnimation { playback, frame });
                Ok(())
            }
            Err(error) => {
                let message = format!(
                    "world {} instance {index} animation initialization: {error:#}",
                    self.world
                );
                actor.animation_failure = Some(message.clone());
                anyhow::bail!(message)
            }
        }
    }

    /// Select a nonplayer map actor's ordinary default using its owned state.
    /// Item readiness and shared nominal ticks come from the invoking game stage.
    /// This is separate from advancement and never grants update eligibility.
    pub fn select_map_animation_default(
        &mut self,
        id: ScriptActorId,
        item_system_ready: bool,
        ticks: u32,
        sounds: &mut dyn FnMut(ScriptActorId, super::ScriptSound) -> Result<()>,
    ) -> Result<()> {
        let index = self.resolve(id)?;
        let actor = &mut self.actors[index];
        if let Some(error) = actor.failure() {
            anyhow::bail!("{error}");
        }
        let control = actor.control.as_ref().with_context(|| {
            format!(
                "world {} instance {index}: ordinary map control state unavailable",
                self.world
            )
        })?;
        let animation = actor.animation.as_mut().with_context(|| {
            format!(
                "world {} instance {index}: animation not prepared",
                self.world
            )
        })?;
        let group = if control.death_selector == 8 {
            if let Some(default) = control.combat_death_default {
                default as u16
            } else if animation.playback.has_group(123) {
                123
            } else {
                124
            }
        } else if control.rope_attached {
            79
        } else if control.living_state == 15 {
            7
        } else {
            let (holding, drawing, sheathing) = super::activation::weapon_predicates(
                control.living_state,
                Some(super::ActivationAnimation {
                    group: animation.playback.current_group(),
                }),
                control.combat_death_default.is_some(),
                control.weapon_drawn,
                actor.attributes[110],
                item_system_ready,
                false,
            );
            if sheathing {
                u16::from(actor.attributes[44])
            } else if drawing || holding {
                8
            } else if control.torch_burning != 0 {
                5
            } else {
                u16::from(actor.attributes[44])
            }
        };
        let position = animation_sound_position(self.spatial.as_ref(), index);
        let surface = actor.surface;
        let result = animation
            .playback
            .select_default_with_sounds(group, ticks, &mut |sound| {
                sounds(
                    id,
                    super::ScriptSound::Animation {
                        sound,
                        surface,
                        position,
                    },
                )
            });
        match result {
            Ok(frame) => {
                animation.frame = frame.or(animation.frame);
                Ok(())
            }
            Err(error) => {
                let message = format!(
                    "world {} instance {index} default selection: {error:#}",
                    self.world
                );
                actor.animation_failure = Some(message.clone());
                anyhow::bail!(message)
            }
        }
    }

    fn validate_animation_binding(&self, id: ScriptActorId, data: &Animations) -> Result<usize> {
        let index = self.resolve(id)?;
        anyhow::ensure!(
            data.world == self.world,
            "world {} animation data belongs to world {}",
            self.world,
            data.world
        );
        anyhow::ensure!(
            data.metadata["script_content_id"].as_str() == Some(&self.content_id),
            "world {} animation/script cache generation mismatch; re-export the world",
            self.world
        );
        let actor = &self.actors[index];
        if let Some(error) = actor.failure() {
            anyhow::bail!("{error}");
        }
        anyhow::ensure!(
            actor.animation.is_none(),
            "world {} instance {index}: animation already prepared",
            self.world
        );
        Ok(index)
    }
    /// Construct a standalone player role's animation and combat association once.
    /// The caller owns entry position/surface snapshots for synchronous sounds and
    /// supplies the existing nominal ticks. This does not advance player controls.
    pub fn initialize_player_animation(
        &mut self,
        id: ScriptActorId,
        data: &Animations,
        ticks: u32,
        sounds: &mut dyn FnMut(cyrus_world::animation::AnimationSound) -> Result<()>,
    ) -> Result<()> {
        let index = self.validate_animation_binding(id, data)?;
        let actor = &mut self.actors[index];
        anyhow::ensure!(
            matches!(
                actor.source,
                ScriptActorSource::Player | ScriptActorSource::AlternatePlayer
            ),
            "world {} instance {index}: player initialization requires a standalone player role",
            self.world
        );
        let control = actor
            .control
            .as_mut()
            .expect("player role has allocation state");
        self.globals.initialize_player_controller();
        control.combat_death_default = (actor.attributes[14] != 0).then_some(0);
        control.combat = (actor.attributes[14] != 0)
            .then(|| super::CombatControlState::from_attributes(&actor.attributes));
        match crate::player::placement::initial_role_animation(data, actor.name(), ticks, sounds) {
            Ok((playback, frame)) => {
                actor.animation = Some(ScriptAnimation { playback, frame });
                if matches!(actor.source, ScriptActorSource::AlternatePlayer) {
                    actor
                        .control
                        .as_mut()
                        .expect("player role has allocation state")
                        .object_update_excluded = true;
                }
                Ok(())
            }
            Err(error) => {
                let message = format!(
                    "world {} instance {index} player initialization: {error:#}",
                    self.world
                );
                actor.animation_failure = Some(message.clone());
                anyhow::bail!(message)
            }
        }
    }

    /// The admitted player stage and SOUP share this playback and selected file frame.
    pub(crate) fn player_animation(
        &mut self,
        id: ScriptActorId,
        data: &Animations,
        ticks: u32,
        sounds: &mut dyn FnMut(cyrus_world::animation::AnimationSound) -> Result<()>,
    ) -> Result<&mut ScriptAnimation> {
        let index = self.resolve(id)?;
        anyhow::ensure!(
            matches!(self.actors[index].source, ScriptActorSource::Player),
            "world {} instance {index}: grounded update requires the logical player",
            self.world
        );
        anyhow::ensure!(
            data.world == self.world
                && data.metadata["script_content_id"].as_str() == Some(&self.content_id),
            "world {} player animation/script cache generation mismatch; re-export the world",
            self.world
        );
        if let Some(error) = self.actors[index].failure() {
            anyhow::bail!("world {} player: {error}", self.world);
        }
        if self.actors[index].animation.is_none() {
            self.initialize_player_animation(id, data, ticks, sounds)?;
        }
        Ok(self.actors[index]
            .animation
            .as_mut()
            .expect("player animation initialized"))
    }

    /// Last selected file frame; resolve through this world's animation asset for presentation.
    pub fn animation_frame(&self, id: ScriptActorId) -> Result<Option<u32>> {
        let index = self.resolve(id)?;
        Ok(self.actors[index]
            .animation
            .as_ref()
            .and_then(|animation| animation.frame))
    }
    /// Deterministic inspection of one body. This does not establish actor eligibility.
    pub fn step(&mut self, id: ScriptActorId, thread: Thread, budget: usize) -> Result<()> {
        self.advance(
            id,
            ScriptStage::Body(thread),
            ScriptInputs::default(),
            None,
            None,
            budget,
        )
    }
    /// Explicit task timing only. Missing scale fails before changing any actor state.
    pub fn step_tasks(&mut self, id: ScriptActorId, frame_scale: Option<u32>) -> Result<()> {
        self.resolve(id)?;
        let frame_scale = frame_scale.with_context(|| {
            format!(
                "world {} instance {} task stage requires frame_scale",
                self.world, id.index
            )
        })?;
        self.advance(
            id,
            ScriptStage::Tasks,
            ScriptInputs {
                frame_scale: Some(frame_scale),
                ..Default::default()
            },
            None,
            None,
            0,
        )
    }
    /// Invokes one explicit stage against the actor's shared native services.
    /// Worlds with any animation-bound actor require nominal ticks, including zero. Sound effects
    /// are dispatched synchronously with the receiving actor's identity.
    /// Random functions require the caller's shared engine stream; they never seed actor state.
    pub fn advance(
        &mut self,
        id: ScriptActorId,
        stage: ScriptStage,
        inputs: ScriptInputs<'_>,
        sounds: Option<&mut dyn FnMut(ScriptActorId, super::ScriptSound) -> Result<()>>,
        random: Option<&mut crate::random::EngineRandom>,
        budget: usize,
    ) -> Result<()> {
        let index = self.resolve(id)?;
        if let Some(activation) = inputs.activation {
            self.validate_activation(activation)?;
        }
        if matches!(stage, ScriptStage::SeparateMain | ScriptStage::SeparateHook) {
            anyhow::ensure!(
                self.actors[index]
                    .placement()
                    .is_some_and(|placement| matches!(placement.object_type, 2 | 6))
                    && self.actors[index].control.is_some(),
                "separate stage requires admitted type-two or type-six scratch state"
            );
        }
        let nominal_ticks = inputs.nominal_ticks;
        if matches!(
            stage,
            ScriptStage::Tasks | ScriptStage::OrdinaryMain | ScriptStage::SeparateMain
        ) {
            anyhow::ensure!(
                inputs.frame_scale.is_some(),
                "task stage requires frame_scale"
            );
        }
        if self.spatial.is_some() {
            anyhow::ensure!(
                inputs.frame_scale.is_some() && nominal_ticks.is_some(),
                "spatial script stages require frame_scale and nominal_ticks"
            );
        }
        if let Some(query) = inputs.player_query {
            anyhow::ensure!(
                query.linked_position.is_none(),
                "script query linked-position ownership must be established per receiver"
            );
        }
        if let Some(error) = self.actors[index].failure() {
            anyhow::bail!("{error}");
        }
        if self.actors.iter().any(|actor| actor.animation.is_some())
            || matches!(stage, ScriptStage::Animation)
        {
            anyhow::ensure!(
                nominal_ticks.is_some(),
                "world {} instance {index}: animation requires nominal_ticks",
                self.world
            );
        }
        if matches!(stage, ScriptStage::OrdinaryMain) && !self.updates.task_system {
            return self.finish_ordinary_script_response(id, nominal_ticks);
        }
        if matches!(stage, ScriptStage::OrdinaryHook) {
            let actor = &self.actors[index];
            let control = actor.control.as_ref().with_context(|| {
                format!(
                    "world {} instance {index}: script-control lifecycle unavailable",
                    self.world
                )
            })?;
            if control.script_control & 2 != 0
                || (self.updates.task_system
                    && actor
                        .runtime
                        .as_ref()
                        .is_some_and(Runtime::has_unpaused_task))
            {
                return Ok(());
            }
        }
        if matches!(stage, ScriptStage::SeparateHook)
            && self.updates.task_system
            && self.actors[index]
                .runtime
                .as_ref()
                .is_some_and(Runtime::has_unpaused_task)
        {
            return Ok(());
        }
        let bypass_tasks = if matches!(stage, ScriptStage::OrdinaryMain) {
            let control = self.actors[index].control.as_ref().with_context(|| {
                format!(
                    "world {} instance {index}: script-control lifecycle unavailable",
                    self.world
                )
            })?;
            if control.script_control & 1 != 0 {
                return self.finish_ordinary_script_response(id, nominal_ticks);
            }
            control.script_control & 4 != 0
        } else {
            false
        };
        let mut host = GameHost {
            world: self.world,
            animation_system: self.updates.animation_system,
            index,
            generation: self.generation,
            actors: &mut self.actors,
            task_calls: &self.task_calls,
            reference_names: &self.reference_names,
            ordinary_actors: self
                .actor_lists
                .as_ref()
                .and_then(|lists| lists.retained_selected()),
            in_combat_threshold: self.in_combat_threshold,
            names: &self.names,
            flag_state: &mut self.flag_state,
            sounds,
            random,
            spatial: self.spatial.as_mut(),
            markers: self.markers.as_ref(),
            walk_map_count: self.walk_nodes.as_ref().map(|nodes| nodes.map_count()),
            task_system: self.updates.task_system,
            map_updates: &mut self.map_updates,
            activation: &mut self.activation,
            globals: &mut self.globals,
            inputs,
        };
        if matches!(stage, ScriptStage::Animation) {
            anyhow::ensure!(
                host.actors[index].animation.is_some(),
                "world {} instance {index}: actor animation not prepared",
                self.world
            );
            return host.advance_animation().map_err(|error| {
                let message = format!(
                    "world {} instance {index} animation stage: {error:#}",
                    self.world
                );
                host.actors[index].animation_failure = Some(message.clone());
                anyhow::anyhow!(message)
            });
        }
        let native_hook = host.actors[index].native_hook.clone();
        let actor = &mut host.actors[index];
        let mut runtime = actor.runtime.take().with_context(|| {
            actor
                .unavailable
                .clone()
                .unwrap_or_else(|| "program absent".into())
        })?;
        let result = match stage {
            ScriptStage::OrdinaryMain | ScriptStage::SeparateMain => runtime.invoke_main_stage(
                &mut host,
                &mut self.flags,
                inputs.frame_scale.unwrap(),
                budget,
                bypass_tasks,
            ),
            ScriptStage::NativeHook | ScriptStage::OrdinaryHook | ScriptStage::SeparateHook => {
                match native_hook {
                    Some(call) => runtime
                        .invoke_native_hook(&call, &mut host, &mut self.flags)
                        .map(|_| ()),
                    None => Ok(()),
                }
            }
            ScriptStage::Entry => runtime.invoke_entry(&mut host, &mut self.flags, budget),
            ScriptStage::Body(thread) => {
                runtime.invoke_body(thread, &mut host, &mut self.flags, budget)
            }
            ScriptStage::Tasks => {
                runtime.invoke_tasks_timed(&mut host, &mut self.flags, inputs.frame_scale.unwrap())
            }
            ScriptStage::Animation => unreachable!(),
        };
        host.actors[index].runtime = Some(runtime);
        result?;
        if matches!(stage, ScriptStage::OrdinaryMain) {
            self.finish_ordinary_script_response(id, nominal_ticks)?;
        }
        Ok(())
    }
    /// Gameplay-global state shared by native calls and activation presentation.
    pub fn globals(&self) -> &super::GameplayGlobals {
        &self.globals
    }
    /// Borrow the topic-selection owner state without changing actor identity.
    pub fn globals_mut(&mut self) -> &mut super::GameplayGlobals {
        &mut self.globals
    }

    /// Shared ownership and pending text; fresh world preparation resets both.
    pub fn activation(&self) -> &super::ActivationState {
        &self.activation
    }

    fn validate_activation(&self, inputs: &super::ActivationInputs) -> Result<()> {
        let player = self.resolve(inputs.player)?;
        anyhow::ensure!(
            matches!(self.actors[player].source, ScriptActorSource::Player),
            "activation selected-player switching unsupported"
        );
        for &index in inputs.actors.keys() {
            self.id(index)?;
        }
        Ok(())
    }

    /// Advance one explicit overlay update using the same world-owned prompt as SOUP.
    /// The caller supplies nominal ticks and the selected player's current conditions.
    pub fn activation_overlay(
        &mut self,
        inputs: &super::ActivationInputs,
        ticks: u32,
        cycle_held: bool,
        override_ticks: u32,
    ) -> Result<super::ActivationOverlay> {
        self.validate_activation(inputs)?;
        self.activation.overlay(
            inputs,
            &self.actors[inputs.player.index],
            ticks,
            cycle_held,
            override_ticks,
            self.globals.lockout(),
        )
    }

    fn resolve(&self, actor: ScriptActorId) -> Result<usize> {
        anyhow::ensure!(
            actor.generation == self.generation,
            "stale script actor generation {} in world {} generation {}",
            actor.generation,
            self.world,
            self.generation
        );
        anyhow::ensure!(
            actor.index < self.actors.len(),
            "world {} instance {} absent",
            self.world,
            actor.index
        );
        Ok(actor.index)
    }
    /// Bounded diagnostics carry logical state, never installed bytecode.
    pub fn inspect(&self, offset: usize, limit: usize) -> serde_json::Value {
        serde_json::json!({
            "world": self.world, "generation": self.generation, "content_id": self.content_id, "total": self.actors.len(),
            "activation": self.activation, "globals": self.globals,
            "actors": self.actors.iter().enumerate().skip(offset).take(limit.min(64)).map(|(index, actor)| serde_json::json!({
                "id": ScriptActorId { generation: self.generation, index }, "name": actor.name(),
                "source": actor.source.label(), "runtime_identifier": actor.runtime_identifier,
                "object_id": actor.placement().map(|placement| placement.object_id), "unavailable": actor.unavailable,
                "main": actor.runtime.as_ref().and_then(|runtime| runtime.program_counter(Thread::Main)),
                "interrupt": actor.runtime.as_ref().and_then(|runtime| runtime.program_counter(Thread::Interrupt)),
                "locals": actor.runtime.as_ref().map(|runtime| runtime.variables()),
                "attributes": actor.attributes,
                "control": actor.control, "surface": actor.surface,
                "spatial": self.spatial.as_ref().and_then(|spatial| spatial.actor(index).ok()),
                "animation": actor.animation.as_ref().map(|animation| serde_json::json!({
                    "current_group": animation.playback.current_group(),
                    "pending_group": animation.playback.pending_group(),
                    "frame": animation.frame,
                })),
                "tasks": actor.runtime.as_ref().map(|runtime| runtime.active_tasks()),
                "failure": actor.failure(),
            })).collect::<Vec<_>>()
        })
    }
    pub fn flags(&self) -> &[i32] {
        &self.flags
    }
    /// Capture ordered global values and latches without running scripts.
    /// The snapshot includes installed definitions for compatibility validation.
    pub fn snapshot_flags(&self) -> super::GlobalFlagsSnapshot {
        self.flag_state.snapshot(&self.flags)
    }

    /// Validate all flag definitions before replacing values and latches together.
    /// No native operations, initialization or effects run during restoration.
    /// World/actor restoration and cross-world retention remain host-owned.
    pub fn restore_flags(&mut self, snapshot: super::GlobalFlagsSnapshot) -> Result<()> {
        self.flag_state
            .restore(&mut self.flags, snapshot)
            .with_context(|| format!("world {} global flag restoration", self.world))
    }

    /// Read the global flag's latch without applying its native set/reset operation.
    pub fn global_flag_is_set(&self, index: usize) -> Result<bool> {
        anyhow::ensure!(
            index < self.flags.len(),
            "world {}: global flag {index} absent",
            self.world
        );
        Ok(self.flag_state.is_set(index))
    }
    /// Assign a raw value and optionally mark the latch, as the console does.
    /// A false `mark_set` preserves the latch; it does not clear it.
    pub fn assign_global_flag(&mut self, index: usize, value: i32, mark_set: bool) -> Result<()> {
        let slot = self
            .flags
            .get_mut(index)
            .with_context(|| format!("world {}: global flag {index} absent", self.world))?;
        *slot = value;
        if mark_set {
            self.flag_state.mark_set(index);
        }
        Ok(())
    }
    pub fn actors(&self) -> &[ScriptActor] {
        &self.actors
    }
    pub fn id(&self, index: usize) -> Result<ScriptActorId> {
        anyhow::ensure!(
            index < self.actors.len(),
            "world {} instance {index} absent",
            self.world
        );
        Ok(ScriptActorId {
            generation: self.generation,
            index,
        })
    }
    pub fn invoke(
        &mut self,
        actor: ScriptActorId,
        thread: Thread,
        host: &mut dyn Host,
        budget: usize,
    ) -> Result<()> {
        self.resolve(actor)?;
        let actor = self
            .actors
            .get_mut(actor.index)
            .context("script instance absent")?;
        let runtime = actor.runtime.as_mut().with_context(|| {
            actor
                .unavailable
                .clone()
                .unwrap_or_else(|| "program absent".into())
        })?;
        runtime.invoke(thread, host, &mut self.flags, budget)
    }
}

/// Session owner. Default construction establishes fresh-session globals;
/// replacing its current world preserves them rather than starting a new session.
#[derive(Resource)]
pub struct ScriptActors {
    pub current: Option<ScriptWorld>,
    visited: BTreeMap<u32, ScriptWorld>,
    discard_current_on_release: bool,
    pub(crate) restored_generation: Option<u64>,
    generation: u64,
    requested: Option<Handle<Scripts>>,
    globals: super::GameplayGlobals,
    pub(super) error: Option<String>,
}
impl Default for ScriptActors {
    fn default() -> Self {
        Self {
            current: None,
            visited: BTreeMap::new(),
            discard_current_on_release: false,
            restored_generation: None,
            generation: 0,
            requested: None,
            globals: super::GameplayGlobals::new_session(),
            error: None,
        }
    }
}
/// Animation commands capture the receiver's own local Q8 position.
/// Unlike script Sound, they do not substitute linked or attached positions.
fn animation_sound_position(
    spatial: Option<&super::spatial::SpatialWorld>,
    index: usize,
) -> Option<[i32; 3]> {
    Some(spatial?.actor(index).ok()?.local.position)
}

fn apply_player_lockout(
    actors: &mut [ScriptActor],
    activation: &mut super::ActivationState,
    player: Option<usize>,
    lockout: u16,
) {
    if let Some(player) = player {
        actors[player].attributes[117] = u8::from(lockout == 1);
    }
    if lockout != 1 {
        for actor in actors
            .iter_mut()
            .filter(|actor| actor.runtime_identifier.is_some())
        {
            actor.attributes[117] = 0;
        }
        activation.clear_prompt_override();
    }
}

impl ScriptActors {
    /// Apply New Game's lockout cleanup and combat-queue reset before world replacement.
    /// Ordinary world reloads retain globals and must not call this operation.
    pub fn begin_new_game(&mut self) {
        if let Some(world) = self.current.as_mut() {
            let player = world
                .actors
                .iter()
                .position(|actor| matches!(actor.source, ScriptActorSource::Player));
            let lockout = world.globals.lockout_player(0);
            apply_player_lockout(&mut world.actors, &mut world.activation, player, lockout);
            world.globals.combat_messages.reset();
        } else {
            self.globals.lockout_player(0);
            self.globals.combat_messages.reset();
        }
        self.discard_current_on_release = self.current.is_some();
        self.visited.clear();
    }

    fn release_world(&mut self) {
        self.restored_generation = None;
        if let Some(mut world) = self.current.take() {
            self.globals = std::mem::take(&mut world.globals);
            if !self.discard_current_on_release {
                // An unfinished revisit must not overwrite its retained actor state.
                self.visited.entry(world.world).or_insert(world);
            }
            self.discard_current_on_release = false;
        }
    }
    /// Record a host failure without replacing the first failure in this world.
    /// World preparation clears it alongside the logical registry.
    pub fn report_runtime_error(&mut self, error: String) {
        self.error.get_or_insert(error);
    }
}

pub(super) fn prepare(
    prepared: Res<PreparedWorld>,
    assets: Res<Assets<Scripts>>,
    animations: Res<Assets<Animations>>,
    mut actors: ResMut<ScriptActors>,
    clock: Option<Res<crate::clock::SimulationClock>>,
    mut commands: Commands,
) {
    // A replacement registry adopts its already validated world without entry work.
    if actors.restored_generation.is_some() && actors.requested.is_none() {
        let (Some(scripts), Some(animation_data)) = (
            assets.get(&prepared.scripts),
            animations.get(&prepared.animations),
        ) else {
            return;
        };
        let compatible = actors.current.as_ref().is_some_and(|world| {
            world.world == scripts.world
                && world.content_id == scripts.content_id
                && prepared.world.is_none_or(|id| id == world.world)
                && animation_data.world == world.world
                && animation_data.metadata["script_content_id"].as_str()
                    == Some(world.content_id.as_str())
        });
        if compatible {
            actors.requested = Some(prepared.scripts.clone());
        } else {
            actors.report_runtime_error(
                "restored registry does not match prepared world assets".into(),
            );
        }
        return;
    }
    if prepared.is_changed() {
        actors.release_world();
        actors.error = None;
        actors.requested = Some(prepared.scripts.clone());
        let Some(generation) = actors.generation.checked_add(1) else {
            actors.error = Some("script world generation exhausted".into());
            actors.requested = None;
            return;
        };
        actors.generation = generation;
    }
    if actors.current.is_some() || actors.requested.as_ref() != Some(&prepared.scripts) {
        return;
    }
    let Some(scripts) = assets.get(&prepared.scripts) else {
        return;
    };
    let Some(animation_data) = animations.get(&prepared.animations) else {
        return;
    };
    let expected_world = prepared.world.unwrap_or(animation_data.world);
    let result = (|| {
        anyhow::ensure!(
            expected_world == scripts.world && animation_data.world == scripts.world,
            "world {} script identity differs from prepared world {}",
            scripts.world,
            expected_world
        );
        anyhow::ensure!(
            animation_data.metadata["script_content_id"].as_str() == Some(&scripts.content_id),
            "world {} animation/script cache generation mismatch; re-export the world",
            scripts.world
        );
        if let Some(clock) = &clock {
            anyhow::ensure!(
                clock.settings() == scripts.clock,
                "world {} clock settings differ from the active game's installed settings",
                scripts.world
            );
        }
        actors.globals.prepare_dialogue(&scripts.dialogue)?;
        let mut world = ScriptWorld::prepare(scripts, actors.generation)?;
        world.prepare_markers(animation_data)?;
        world.prepare_navigation(animation_data)?;
        if clock.is_none() {
            commands.insert_resource(crate::clock::SimulationClock::new(scripts.clock)?);
        }
        Ok(world)
    })();
    match result {
        Ok(mut world) => {
            world.globals = std::mem::take(&mut actors.globals);
            world.globals.sound_maintenance.enter_world();
            actors.current = Some(world);
        }
        Err(error) => {
            actors.error = Some(format!(
                "world {} script preparation: {error:#}",
                scripts.world
            ));
            actors.requested = None;
        }
    }
}

struct GameHost<'a, 'sound, 'input, 'random> {
    world: u32,
    animation_system: bool,
    task_system: bool,
    map_updates: &'a mut Vec<usize>,
    activation: &'a mut super::ActivationState,
    globals: &'a mut super::GameplayGlobals,
    spatial: Option<&'a mut super::spatial::SpatialWorld>,
    markers: Option<&'a super::markers::Markers>,
    walk_map_count: Option<usize>,
    inputs: ScriptInputs<'input>,
    index: usize,
    generation: u64,
    sounds: Option<&'sound mut dyn FnMut(ScriptActorId, super::ScriptSound) -> Result<()>>,
    random: Option<&'random mut crate::random::EngineRandom>,
    actors: &'a mut [ScriptActor],
    ordinary_actors: Option<&'a [ScriptActorId]>,
    in_combat_threshold: Option<i32>,
    task_calls: &'a BTreeMap<(CodeOrigin, u32), Call>,
    reference_names: &'a [String],
    names: &'a BTreeMap<i16, usize>,
    flag_state: &'a mut super::flags::Flags,
}
impl<'input> GameHost<'_, '_, 'input, '_> {
    fn actor_is_dead(&self, index: usize) -> Result<bool> {
        if self.selected_player()
            == Some(ScriptActorId {
                generation: self.generation,
                index,
            })
            && self
                .globals
                .player_death_override()
                .context("IsDead: selected-player override unavailable")?
        {
            return Ok(true);
        }
        Ok(self.actors[index]
            .control
            .as_ref()
            .context("IsDead: actor death lifecycle unavailable")?
            .death_selector
            == 8)
    }

    fn fighting_position(&self, index: usize) -> Result<[i32; 3]> {
        let actor = self
            .spatial
            .as_deref()
            .context("isFighting: spatial owner unavailable")?
            .actor(index)?;
        if actor.parent.is_some() {
            Ok(actor.query_pose()?.position)
        } else {
            Ok(actor.linked_position.unwrap_or(actor.local.position))
        }
    }

    fn is_fighting(&self) -> Result<bool> {
        let traversal = self
            .ordinary_actors
            .context("isFighting: ordinary traversal unavailable")?;
        let receiver = ScriptActorId {
            generation: self.generation,
            index: self.index,
        };
        let mut nearest = None;
        let mut bound = 65535;
        for &candidate in traversal {
            anyhow::ensure!(
                candidate.generation == self.generation && candidate.index < self.actors.len(),
                "isFighting: stale ordinary actor {candidate:?}"
            );
            if candidate == receiver {
                continue;
            }
            let Some(combat) = self.actors[candidate.index]
                .control
                .as_ref()
                .and_then(|control| control.combat.as_ref())
            else {
                continue;
            };
            if combat.target != Some(receiver)
                || combat.attacker_selection_state != 1
                || self.actor_is_dead(candidate.index)?
            {
                continue;
            }
            let origin = self.fighting_position(self.index)?;
            let position = self.fighting_position(candidate.index)?;
            let differences = std::array::from_fn(|axis| position[axis].wrapping_sub(origin[axis]));
            let distance = cyrus_collision::distance::rounded_distance(differences)?;
            if distance < bound {
                bound = distance;
                nearest = Some(differences);
            }
        }
        let Some([x, _, z]) = nearest else {
            return Ok(false);
        };
        let threshold = self
            .in_combat_threshold
            .context("isFighting: installed combat threshold unavailable")?;
        Ok(cyrus_collision::distance::rounded_distance([x, 0, z])? < threshold)
    }

    fn ambient_call(&self) -> super::tasks::AmbientCall<'input> {
        let actor = &self.actors[self.index];
        let position = self.spatial.as_deref().and_then(|world| {
            let spatial = world.actor(self.index).ok()?;
            if spatial.parent.is_some() {
                Some(spatial.effective?.position)
            } else {
                Some(spatial.linked_position.unwrap_or(spatial.local.position))
            }
        });
        super::tasks::AmbientCall {
            driver: self.inputs.ambient_sound,
            reset: self.inputs.sound_reset,
            position,
            excluded: actor
                .control
                .as_ref()
                .map(|control| control.object_update_excluded),
            generation: self.generation,
            actor_index: self.index,
        }
    }

    fn selected_player(&self) -> Option<ScriptActorId> {
        self.inputs
            .activation
            .map(|inputs| inputs.player)
            .or_else(|| {
                self.actors
                    .iter()
                    .position(|actor| matches!(actor.source, ScriptActorSource::Player))
                    .map(|index| ScriptActorId {
                        generation: self.generation,
                        index,
                    })
            })
    }

    /// Receiver state for locomotion tasks; fallible reads surface only when a task uses them.
    fn locomotion_state(&self) -> Result<super::locomotion::BackwardState, String> {
        let actor = &self.actors[self.index];
        let control = actor
            .control
            .as_ref()
            .ok_or("locomotion control state unavailable")?;
        let (holding_weapon, _, _) = self
            .receiver_weapon_predicates("MoveBackward")
            .map_err(|error| format!("{error:#}"))?;
        Ok(super::locomotion::BackwardState {
            rope_attached: control.rope_attached,
            sliding_state: control.sliding_state,
            falling_state: control.falling_state,
            queued_motion: control.queued_motion,
            living_state: control.living_state,
            holding_weapon,
            combat_mode: control
                .combat
                .as_ref()
                .is_some_and(|combat| combat.flags & 4 != 0),
            surface: actor.surface.unwrap_or(0),
            torch_burning: control.torch_burning != 0,
            step_attribute: actor.attributes[9],
            combat_step_attribute: actor.attributes[18],
        })
    }

    fn frame_wait_call(&self) -> super::tasks::FrameWaitCall {
        let sample = |index: usize| -> Result<(i32, i32, u8)> {
            self.actors[index]
                .animation
                .as_ref()
                .with_context(|| {
                    format!("actor {index}: animation frame wait requires prepared playback")
                })?
                .playback
                .frame_wait_state()
        };
        let player = (|| -> Result<Option<(i32, i32, u8)>> {
            let Some(player) = self.selected_player() else {
                return Ok(None);
            };
            anyhow::ensure!(
                player.generation == self.generation && player.index < self.actors.len(),
                "animation frame wait selected player identity is stale"
            );
            sample(player.index).map(Some)
        })();
        super::tasks::FrameWaitCall {
            receiver: sample(self.index).map_err(|error| format!("{error:#}")),
            player: player.map_err(|error| format!("{error:#}")),
        }
    }

    fn player_dialogue_call(&self) -> super::tasks::PlayerDialogueCall<'input> {
        super::tasks::PlayerDialogueCall {
            source: self.inputs.dialogue_source,
            player: self.selected_player(),
            reset: self.inputs.sound_reset,
        }
    }

    fn advance_animation(&mut self) -> Result<()> {
        let id = ScriptActorId {
            generation: self.generation,
            index: self.index,
        };
        let ticks = self
            .inputs
            .nominal_ticks
            .context("animation requires nominal_ticks")?;
        let position = animation_sound_position(self.spatial.as_deref(), self.index);
        let surface = self.actors[self.index].surface;
        let mut emit = |sound| match &mut self.sounds {
            Some(sounds) => sounds(
                id,
                super::ScriptSound::Animation {
                    sound,
                    surface,
                    position,
                },
            ),
            None => anyhow::bail!("world actor {}: animation sound handler absent", id.index),
        };
        let animation = self.actors[self.index]
            .animation
            .as_mut()
            .context("actor animation not prepared")?;
        let frame = animation.playback.advance_with_sounds(ticks, &mut emit)?;
        animation.frame = frame.or(animation.frame);
        let frame = animation.playback.request_with_sounds(
            animation.playback.default_group(),
            0,
            ticks,
            &mut emit,
        )?;
        animation.frame = frame.or(animation.frame);
        Ok(())
    }

    /// Receiver weapon predicates as (holding, drawing, sheathing); all false while the
    /// inventory or the receiver's animation is not ready.
    /// `IsCombatCapable`, and `PlayerArmed` when `armed`, which also needs the drawn weapon
    /// not to be sheathing. The original's transition lock and world-active globals are
    /// **Inferred** clear and set respectively; cyrus has no transition state yet.
    fn combat_capable(&self, index: usize, armed: bool) -> Result<bool> {
        let actor = &self.actors[index];
        if actor.attributes[110] != 0 {
            return Ok(true);
        }
        let (Some(control), Some(animation)) = (actor.control.as_ref(), actor.animation.as_ref())
        else {
            return Ok(false);
        };
        if control.combat.is_none() {
            return Ok(false);
        }
        if !armed {
            return Ok(true);
        }
        let sheathing =
            control.living_state == 21 || animation.playback.current_group() == Some(129);
        Ok(control.weapon_drawn && !sheathing)
    }

    fn receiver_weapon_predicates(&self, function: &str) -> Result<(bool, bool, bool)> {
        let actor = &self.actors[self.index];
        if function.eq_ignore_ascii_case("IsHoldingWeapon") && actor.attributes[110] != 0 {
            return Ok((true, false, false));
        }
        let ready = self
            .inputs
            .hud
            .with_context(|| format!("{function}: inventory owner unavailable"))?
            .inventory_ready()?;
        if !ready || actor.animation.is_none() {
            return Ok((false, false, false));
        }
        let shell = self
            .inputs
            .shell_execution
            .with_context(|| format!("{function}: shell context unavailable"))?;
        let control = actor
            .control
            .as_ref()
            .with_context(|| format!("{function}: receiver control unavailable"))?;
        Ok(super::activation::weapon_predicates(
            control.living_state,
            actor
                .animation
                .as_ref()
                .map(|animation| super::ActivationAnimation {
                    group: animation.playback.current_group(),
                }),
            control.combat.is_some(),
            control.weapon_drawn,
            actor.attributes[110],
            ready,
            shell,
        ))
    }

    fn property_name(&self, target: &Target, reference: u16) -> Result<&str> {
        self.reference_names
            .get(usize::from(reference))
            .map(String::as_str)
            .with_context(|| format!("property {target:?}.{reference}: installed reference absent"))
    }

    fn property_receiver(&self, target: &Target) -> Result<usize> {
        if !matches!(target, Target::Player) {
            return self.target_index(target);
        }
        let player = self
            .selected_player()
            .context("Player property selected player unavailable")?;
        anyhow::ensure!(
            player.generation == self.generation && player.index < self.actors.len(),
            "Player property selected player identity is stale"
        );
        Ok(player.index)
    }

    fn target_index(&self, target: &Target) -> Result<usize> {
        let index = match target {
            Target::Me => return Ok(self.index),
            Target::Named(name) => self.names.values().copied().find(|&index| {
                self.actors[index]
                    .namespace
                    .as_ref()
                    .is_some_and(|namespace| namespace.eq_ignore_ascii_case(name))
            }),
            Target::Player => self
                .actors
                .iter()
                .position(|actor| matches!(actor.source, ScriptActorSource::Player)),
            Target::Camera => self
                .actors
                .iter()
                .position(|actor| matches!(actor.source, ScriptActorSource::Camera)),
            _ => anyhow::bail!("object target {target:?} unsupported"),
        };
        index.with_context(|| format!("object target {target:?}: receiver absent"))
    }
}
enum PoseProperty {
    Position(usize),
    Angle(usize),
}

fn pose_property(name: &str) -> Option<PoseProperty> {
    let find = |names: [&str; 3]| names.iter().position(|n| name.eq_ignore_ascii_case(n));
    find(["pos.vx", "pos.vy", "pos.vz"])
        .map(PoseProperty::Position)
        .or_else(|| find(["angle.vx", "angle.vy", "angle.vz"]).map(PoseProperty::Angle))
}

impl Host for GameHost<'_, '_, '_, '_> {
    fn read_property(&mut self, target: &Target, reference: u16) -> Result<i32> {
        let name = self.property_name(target, reference)?;
        if name.eq_ignore_ascii_case("anim.current") {
            let index = self.property_receiver(target)?;
            let animation = self.actors[index]
                .animation
                .as_ref()
                .context("anim.current animation owner unavailable")?;
            return Ok(animation
                .playback
                .current_group()
                .map_or(-1, |group| i32::from(group as i16)));
        }
        if name.eq_ignore_ascii_case("torch_timer") {
            let index = self.property_receiver(target)?;
            let control = self.actors[index]
                .control
                .as_ref()
                .context("torch_timer control owner unavailable")?;
            return Ok(i32::from(control.torch_burning));
        }
        let pose = pose_property(name)
            .with_context(|| format!("property {target:?}.{name} unsupported"))?;
        let index = self.property_receiver(target)?;
        let spatial = self
            .spatial
            .as_deref()
            .with_context(|| format!("property {target:?}.{name}: spatial owner unavailable"))?
            .actor(index)?;
        Ok(match pose {
            PoseProperty::Position(axis) => spatial.local.position[axis],
            PoseProperty::Angle(axis) => i32::from(spatial.local.angles[axis]),
        })
    }

    fn write_property(&mut self, target: &Target, reference: u16, value: i32) -> Result<()> {
        let name = self.property_name(target, reference)?;
        if name.eq_ignore_ascii_case("torch_timer") {
            let index = self.property_receiver(target)?;
            let control = self.actors[index]
                .control
                .as_mut()
                .context("torch_timer control owner unavailable")?;
            control.torch_burning = value as u8;
            return Ok(());
        }
        let name = name.to_owned();
        let pose = pose_property(&name)
            .with_context(|| format!("property {target:?}.{name} write unsupported"))?;
        let index = self.property_receiver(target)?;
        let local = &mut self
            .spatial
            .as_deref_mut()
            .with_context(|| format!("property {target:?}.{reference}: spatial owner unavailable"))?
            .actor_mut(index)?
            .local;
        match pose {
            PoseProperty::Position(axis) => local.position[axis] = value,
            // The local angle field is stored wrapped; a wider assigned value has no owner yet.
            PoseProperty::Angle(axis) => {
                local.angles[axis] = u16::try_from(value)
                    .ok()
                    .filter(|angle| *angle < 2048)
                    .with_context(|| {
                        format!(
                            "property {target:?}.{name}: angle {value} outside 0..2048 unsupported"
                        )
                    })?;
            }
        }
        Ok(())
    }

    fn call(
        &mut self,
        target: &Target,
        kind: CallKind,
        function: &str,
        arguments: &[Argument],
        invocation: cyrus_script::Invocation,
        flags: &mut [i32],
    ) -> Result<CallResult> {
        anyhow::ensure!(
            target == &Target::Me,
            "inspection target {target:?} unsupported"
        );
        let id = ScriptActorId {
            generation: self.generation,
            index: self.index,
        };
        if function.eq_ignore_ascii_case("PlayerLineUp") {
            self.globals.lineup_result = None;
            anyhow::bail!("PlayerLineUp alignment execution unavailable; shared result unknown");
        }
        if kind == CallKind::Function
            && [
                "Light",
                "LightRadius",
                "LightIntensity",
                "LightOffset",
                "LightFlicker",
                "FlickerLight",
                "LightFlickerOff",
            ]
            .iter()
            .any(|name| function.eq_ignore_ascii_case(name))
        {
            let actor = &mut self.actors[self.index];
            let placement = actor.placement();
            anyhow::ensure!(
                actor.light.is_some()
                    || placement.is_none_or(|placement| placement.object_type != 4),
                "{function}: type-four light initialization unavailable"
            );
            let position = placement
                .map(|placement| placement.map_position.map(|value| value as i32))
                .unwrap_or_default();
            let light = actor
                .light
                .get_or_insert_with(|| light::LightState::dynamic(position));
            let values = arguments
                .iter()
                .map(|argument| argument.integer())
                .collect::<Result<Vec<_>>>()?;
            let name = function.to_ascii_lowercase();
            let expected = match name.as_str() {
                "light" | "lightflicker" => 2,
                "lightoffset" => 3,
                "lightflickeroff" => 0,
                _ => 1,
            };
            anyhow::ensure!(values.len() == expected, "{function}: wrong argument count");
            // Stored values keep their low sixteen bits, as the original's word stores do.
            match name.as_str() {
                "light" => {
                    light.enabled = true;
                    light.starting_radius = values[0] as i16;
                    light.radius = values[0] as i16;
                    light.starting_intensity = values[1] as i16;
                    light.intensity = values[1] as i16;
                }
                "lightradius" => {
                    light.starting_radius = values[0] as i16;
                    light.radius = values[0] as i16;
                    light.enabled = true;
                    light.inactive_ticks = 0;
                }
                "lightintensity" => {
                    light.starting_intensity = values[0] as i16;
                    light.intensity = values[0] as i16;
                    light.enabled = true;
                    light.inactive_ticks = 0;
                }
                "lightoffset" => {
                    light.offset = [values[0] as i16, values[1] as i16, values[2] as i16]
                }
                "lightflicker" => light.flicker = [values[0] as u16, 0, values[1] as u16],
                "flickerlight" => light.flicker = [values[0] as u16, 0, 1],
                _ => light.flicker[0] = 0,
            }
            return Ok(CallResult::Complete(0));
        }
        if kind == CallKind::Function && function.eq_ignore_ascii_case("LightOff") {
            anyhow::ensure!(arguments.is_empty(), "LightOff requires no arguments");
            let actor = &mut self.actors[self.index];
            anyhow::ensure!(
                actor
                    .placement()
                    .is_none_or(|placement| placement.object_type != 4),
                "LightOff type-four light initialization unavailable"
            );
            if let Some(light) = &mut actor.light {
                light.locked = false;
                light.enabled = false;
                light.inactive_ticks = 0;
            }
            return Ok(CallResult::Complete(0));
        }
        if kind == CallKind::Function && function.eq_ignore_ascii_case("beginCombat") {
            anyhow::ensure!(arguments.is_empty(), "beginCombat requires no arguments");
            let inputs = match self.inputs.combat_entry {
                Some(inputs) => inputs,
                None => {
                    let selected_player = self
                        .selected_player()
                        .context("beginCombat selected player unavailable")?;
                    anyhow::ensure!(
                        id != selected_player,
                        "beginCombat selected-player death override unavailable"
                    );
                    let actor = &self.actors[self.index];
                    anyhow::ensure!(
                        actor.model.prepared,
                        "beginCombat receiver model state unavailable"
                    );
                    CombatEntryInputs {
                        weapon: WeaponInputs {
                            selected_player,
                            item_system_ready: self
                                .inputs
                                .hud
                                .context("beginCombat inventory owner unavailable")?
                                .inventory_ready()?,
                            shell_execution: self
                                .inputs
                                .shell_execution
                                .context("beginCombat shell context unavailable")?,
                        },
                        nominal_ticks: self
                            .inputs
                            .nominal_ticks
                            .context("beginCombat nominal ticks unavailable")?,
                        taunt_animation_admitted: self.animation_system
                            && actor.animation.is_some()
                            && actor.model.static_radius.is_none(),
                        selected_player_death_override: false,
                    }
                }
            };
            anyhow::ensure!(
                inputs.weapon.selected_player.generation == self.generation
                    && inputs.weapon.selected_player.index < self.actors.len(),
                "beginCombat selected actor belongs to an expired or absent world"
            );
            let position = animation_sound_position(self.spatial.as_deref(), self.index);
            let mut receiver = weapon::CombatReceiver::new(
                &mut self.actors[self.index],
                self.globals,
                self.world,
                id,
                position,
            )?;
            let mut emit = |actor, sound| {
                self.sounds
                    .as_deref_mut()
                    .context("beginCombat sound emitter unavailable")?(actor, sound)
            };
            receiver.begin(
                inputs,
                self.random.as_deref_mut(),
                self.inputs.combat_taunts,
                &mut emit,
            )?;
            return Ok(CallResult::CompleteWithoutValue);
        }
        if kind == CallKind::Function && function.eq_ignore_ascii_case("AddItem") {
            let [item, amount] = arguments else {
                anyhow::bail!("AddItem requires two arguments")
            };
            let hud = self
                .inputs
                .hud
                .context("AddItem inventory host unavailable")?;
            if !hud.inventory_ready()? {
                return Ok(CallResult::Complete(0));
            }
            let item =
                u16::try_from(item.integer()?).context("AddItem item ID outside u16 range")?;
            let player = self.selected_player();
            let generation = self.generation;
            let actors = &mut self.actors;
            hud.add_item(item, amount.integer()?, &mut |weapon| {
                let player = player.context("AddItem fallback selected player unavailable")?;
                anyhow::ensure!(
                    player.generation == generation && player.index < actors.len(),
                    "AddItem fallback selected player identity is stale"
                );
                actors[player.index].attributes[23] = weapon;
                Ok(())
            })?;
            return Ok(CallResult::Complete(0));
        }
        if kind == CallKind::Function && function.eq_ignore_ascii_case("ActiveItem") {
            anyhow::ensure!(arguments.is_empty(), "ActiveItem requires no arguments");
            let selected = self
                .inputs
                .hud
                .context("ActiveItem inventory host unavailable")?
                .active_item()?;
            return Ok(CallResult::Complete(i32::from(selected)));
        }
        if kind == CallKind::Function && function.eq_ignore_ascii_case("HandItem") {
            let [item] = arguments else {
                anyhow::bail!("HandItem requires one argument")
            };
            let hud = self
                .inputs
                .hud
                .context("HandItem inventory host unavailable")?;
            if hud.inventory_ready()? {
                self.actors[self.index].hand_item(
                    item.integer()?,
                    hud,
                    self.inputs.shell_execution,
                )?;
            }
            return Ok(CallResult::Complete(0));
        }
        if kind == CallKind::Function && super::ScriptHud::handles(function) {
            let hud = self.inputs.hud.context("script HUD host unavailable")?;
            return Ok(CallResult::Complete(hud.call(function, arguments)?));
        }
        if kind == CallKind::Function && function.eq_ignore_ascii_case("LockoutPlayer") {
            let [value] = arguments else {
                anyhow::bail!("LockoutPlayer requires one argument")
            };
            let value = value.integer()?;
            let player = self
                .inputs
                .activation
                .map(|inputs| inputs.player.index)
                .or_else(|| {
                    self.actors
                        .iter()
                        .position(|actor| matches!(actor.source, ScriptActorSource::Player))
                })
                .context("LockoutPlayer selected player unavailable")?;
            let lockout = self.globals.lockout_player(value);
            apply_player_lockout(self.actors, self.activation, Some(player), lockout);
            return Ok(CallResult::Complete(0));
        }
        if kind == CallKind::Function {
            let automatic;
            let activation = if self.inputs.activation.is_none()
                && (function.eq_ignore_ascii_case("Activate")
                    || function.eq_ignore_ascii_case("TorchActivate"))
                && let Some(use_held) = self.inputs.activation_use_held
            {
                anyhow::ensure!(
                    self.inputs.shell_execution == Some(false),
                    "ordinary activation requires an established non-shell context"
                );
                let hud = self
                    .inputs
                    .hud
                    .context("activation HUD owner unavailable")?;
                automatic = activation::ordinary_inputs(
                    self.actors,
                    self.generation,
                    self.globals,
                    hud,
                    use_held,
                    Some((self.index, arguments)),
                )?;
                Some(&automatic)
            } else {
                self.inputs.activation
            };
            let selected_player = activation.map(|inputs| {
                (
                    &self.actors[inputs.player.index],
                    self.globals.lockout().unwrap_or(inputs.lockout),
                )
            });
            if let Some(value) = self.activation.call(
                id,
                function,
                arguments,
                activation,
                &self.actors[self.index].attributes,
                selected_player,
            )? {
                return Ok(CallResult::Complete(value));
            }
        }
        let ambient = self.ambient_call();
        let player_dialogue = self.player_dialogue_call();
        let frame_wait = self.frame_wait_call();
        let locomotion_state = self.locomotion_state();
        let position = animation_sound_position(self.spatial.as_deref(), self.index);
        let surface = self.actors[self.index].surface;
        let mut emit = |sound| match &mut self.sounds {
            Some(sounds) => sounds(
                id,
                super::ScriptSound::Animation {
                    sound,
                    surface,
                    position,
                },
            ),
            None => anyhow::bail!("world actor {}: animation sound handler absent", id.index),
        };
        let actor = &mut self.actors[self.index];
        let mut animation =
            actor
                .animation
                .as_mut()
                .zip(self.inputs.nominal_ticks)
                .map(|(animation, ticks)| super::tasks::AnimationCall {
                    animation: &mut animation.playback,
                    ticks,
                    frame: &mut animation.frame,
                    sounds: Some(&mut emit),
                });
        let player_position = self
            .inputs
            .player_query
            .as_ref()
            .and_then(|query| query.player.as_ref())
            .map(|player| player.position);
        let mut spatial_call = self
            .spatial
            .as_deref_mut()
            .and_then(|spatial| spatial.actor_mut(self.index).ok())
            .zip(self.inputs.frame_scale.zip(self.inputs.nominal_ticks))
            .map(
                |(spatial, (frame_scale, nominal_ticks))| super::tasks::SpatialCall {
                    matrix: spatial.local.matrix,
                    locations: &spatial.locations,
                    effective_position: spatial.effective.map(|pose| pose.position),
                    player_position,
                    slave: actor.attributes[26] != 0,
                    actor_index: self.index,
                    map_updates: self.map_updates,
                    position: &mut spatial.local.position,
                    angles: &mut spatial.local.angles,
                    frame_scale,
                    nominal_ticks,
                },
            );
        if let Some(result) = actor.tasks.call(
            (function, kind),
            arguments,
            invocation,
            &mut animation,
            &mut spatial_call,
            super::tasks::NativeServices {
                ambient: Some(&ambient),
                dialogue: self.globals.dialogue.as_mut(),
                player_dialogue: Some(player_dialogue),
                frame_wait: Some(frame_wait),
                locomotion: Some(super::tasks::LocomotionCall {
                    state: locomotion_state,
                    blocked_ahead_30: self.inputs.blocked_ahead_30,
                    animation_system: self.animation_system,
                    motion_direction: actor
                        .control
                        .as_mut()
                        .map(|control| &mut control.motion_direction),
                }),
            },
        )? {
            return Ok(result);
        }
        anyhow::ensure!(
            kind == CallKind::Function,
            "native {:?} {function} unsupported by inspection host",
            kind
        );
        if function.eq_ignore_ascii_case("KillMe") {
            anyhow::ensure!(arguments.is_empty(), "KillMe requires no arguments");
            // Walk-node release needs actor-side reservation indices; navigation maps stay
            // unsupported, so no actor can hold a reservation here (shortcut: revisit with maps).
            let control = self.actors[self.index]
                .control
                .as_mut()
                .context("KillMe: receiver lifecycle unavailable")?;
            control.killed = true;
            control.death_selector = 8;
            self.actors[self.index].model.current = None;
            self.sounds
                .as_mut()
                .context("script sound handler absent")?(
                id, super::ScriptSound::StopOwned
            )?;
            return Ok(CallResult::Complete(0));
        }
        if function.eq_ignore_ascii_case("HideMe") || function.eq_ignore_ascii_case("ShowMe") {
            anyhow::ensure!(arguments.is_empty(), "{function} requires no arguments");
            let control = self.actors[self.index].control.as_mut().with_context(|| {
                format!("{function}: receiver visibility lifecycle unavailable")
            })?;
            control.hidden = function.eq_ignore_ascii_case("HideMe");
            if control.hidden {
                self.actors[self.index].model.current = None;
            } else if self.actors[self.index].model.static_radius.is_some() {
                let spatial = self
                    .spatial
                    .as_deref_mut()
                    .context("ShowMe: model spatial state unavailable")?;
                let state = spatial.actor_mut(self.index)?;
                if state.parent.is_none() {
                    state.refresh_transform()?;
                }
                self.actors[self.index]
                    .model
                    .show_static(state.query_pose()?);
            }
            return Ok(CallResult::Complete(0));
        }
        if [
            "IsHoldingWeapon",
            "IsDrawingSword",
            "IsSheathingSword",
            "IsDrawingOrSheathing",
        ]
        .iter()
        .any(|name| function.eq_ignore_ascii_case(name))
        {
            anyhow::ensure!(arguments.is_empty(), "{function} requires no arguments");
            let (holding, drawing, sheathing) = self.receiver_weapon_predicates(function)?;
            let result = if function.eq_ignore_ascii_case("IsHoldingWeapon") {
                holding
            } else if function.eq_ignore_ascii_case("IsDrawingSword") {
                drawing
            } else if function.eq_ignore_ascii_case("IsDrawingOrSheathing") {
                drawing || sheathing
            } else {
                sheathing
            };
            return Ok(CallResult::Complete(i32::from(result)));
        }
        if function.eq_ignore_ascii_case("SetHealth")
            || function.eq_ignore_ascii_case("AdjustHealth")
        {
            let set = function.eq_ignore_ascii_case("SetHealth");
            anyhow::ensure!(
                arguments.len() == if set { 1 } else { 2 },
                "{function}: wrong argument count"
            );
            let selected = self
                .selected_player()
                .is_some_and(|player| player.index == self.index);
            let attributes = &mut self.actors[self.index].attributes;
            // Attribute 120 (nonzero) ignores health changes. Attribute 97 gates on a combat
            // record bit, the selected player updates the HUD, and death starts a sequence;
            // none of those owners exist, so they stay unavailable instead of guessed.
            if attributes[120] != 0 {
                return Ok(CallResult::Complete(0));
            }
            anyhow::ensure!(
                attributes[97] == 0 && !selected,
                "{function}: invulnerability gate and selected-player health unavailable"
            );
            let value = arguments[0].integer()?;
            let delta = if set {
                value.wrapping_sub(i32::from(attributes[3]))
            } else {
                value
            };
            let health = i32::from(attributes[3]).wrapping_add(delta);
            let damage = set || arguments[1].integer()? == 1;
            anyhow::ensure!(
                health > 0 || !damage,
                "{function}: death sequence unavailable"
            );
            attributes[3] = health.clamp(0, 255) as u8;
            return Ok(CallResult::Complete(0));
        }
        // PlayerMoved compares the selected player's position with its snapshot taken at the
        // ordinary actor-update boundary.
        if function.eq_ignore_ascii_case("PlayerMoved") {
            anyhow::ensure!(arguments.is_empty(), "PlayerMoved requires no arguments");
            let player = self
                .selected_player()
                .context("PlayerMoved: selected player unavailable")?;
            let spatial = self
                .spatial
                .as_deref()
                .context("spatial state not prepared")?
                .actor(player.index)?;
            return Ok(CallResult::Complete(i32::from(
                spatial.local.position != spatial.previous_local_position,
            )));
        }
        // IsMaster and IsSlave test attributes 25 and 26; TrueFunction ignores both arguments.
        if function.eq_ignore_ascii_case("IsMaster") || function.eq_ignore_ascii_case("IsSlave") {
            anyhow::ensure!(arguments.is_empty(), "{function} requires no arguments");
            let attribute = if function.eq_ignore_ascii_case("IsMaster") {
                25
            } else {
                26
            };
            return Ok(CallResult::Complete(i32::from(
                self.actors[self.index].attributes[attribute] != 0,
            )));
        }
        if function.eq_ignore_ascii_case("TrueFunction") {
            anyhow::ensure!(arguments.len() == 2, "TrueFunction: wrong argument count");
            return Ok(CallResult::Complete(1));
        }
        // SetAIType and SetAIMode store the low byte of their argument in the bytes that
        // MyAIType and MyAIMode read back.
        if function.eq_ignore_ascii_case("SetAIType") || function.eq_ignore_ascii_case("SetAIMode")
        {
            anyhow::ensure!(arguments.len() == 1, "{function}: wrong argument count");
            let value = arguments[0].integer()? as u8;
            let control = self.actors[self.index]
                .control
                .as_mut()
                .with_context(|| format!("{function}: receiver control state unavailable"))?;
            if function.eq_ignore_ascii_case("SetAIType") {
                control.death_selector = value;
            } else {
                control.death_response = value;
            }
            return Ok(CallResult::Complete(0));
        }
        // `attr_health` is attribute 3; Strength and Armor read the combat record.
        if [
            "Health",
            "PlayerHealth",
            "Strength",
            "Armor",
            "ReplenishHealth",
        ]
        .iter()
        .any(|name| function.eq_ignore_ascii_case(name))
        {
            anyhow::ensure!(arguments.is_empty(), "{function} requires no arguments");
            let lower = function.to_ascii_lowercase();
            let index = if lower == "playerhealth" {
                self.selected_player()
                    .context("PlayerHealth: selected player unavailable")?
                    .index
            } else {
                self.index
            };
            let selected = self
                .selected_player()
                .is_some_and(|player| player.index == index);
            let actor = &mut self.actors[index];
            let value = match lower.as_str() {
                "health" | "playerhealth" => i32::from(actor.attributes[3]),
                "replenishhealth" => {
                    anyhow::ensure!(
                        !selected,
                        "ReplenishHealth selected-player HUD health timer unavailable"
                    );
                    i32::from(
                        actor.attributes[3] < 100 && {
                            actor.attributes[3] = 100;
                            true
                        },
                    )
                }
                _ => {
                    let combat = actor
                        .control
                        .as_ref()
                        .and_then(|control| control.combat.as_ref())
                        .with_context(|| {
                            format!("{function}: receiver combat record unavailable")
                        })?;
                    if lower == "strength" {
                        combat.strength
                    } else {
                        combat.armor
                    }
                }
            };
            return Ok(CallResult::Complete(value));
        }
        if function.eq_ignore_ascii_case("IsCombatCapable")
            || function.eq_ignore_ascii_case("PlayerArmed")
        {
            anyhow::ensure!(arguments.is_empty(), "{function} requires no arguments");
            let armed = function.eq_ignore_ascii_case("PlayerArmed");
            let index = if armed {
                self.selected_player()
                    .context("PlayerArmed: selected player unavailable")?
                    .index
            } else {
                self.index
            };
            return Ok(CallResult::Complete(i32::from(
                self.combat_capable(index, armed)?,
            )));
        }
        if function.eq_ignore_ascii_case("DisableHook")
            || function.eq_ignore_ascii_case("EnableHook")
        {
            anyhow::ensure!(arguments.is_empty(), "{function} requires no arguments");
            let control = self.actors[self.index]
                .control
                .as_mut()
                .with_context(|| format!("{function}: receiver script-control unavailable"))?;
            // Bit 0x02 of the script-control byte suppresses the native hook.
            if function.eq_ignore_ascii_case("DisableHook") {
                control.script_control |= 2;
            } else {
                control.script_control &= !2;
            }
            return Ok(CallResult::Complete(0));
        }
        if function.eq_ignore_ascii_case("SetPotion") || function.eq_ignore_ascii_case("GetPotion")
        {
            let set = function.eq_ignore_ascii_case("SetPotion");
            anyhow::ensure!(
                arguments.len() == 1 + usize::from(set),
                "{function}: wrong argument count"
            );
            let slot = usize::try_from(arguments[0].integer()?)
                .ok()
                .filter(|slot| *slot < self.globals.potions.len())
                .with_context(|| format!("{function}: potion index outside 0..4 unsupported"))?;
            if set {
                self.globals.potions[slot] = arguments[1].integer()?;
                return Ok(CallResult::Complete(1));
            }
            return Ok(CallResult::Complete(self.globals.potions[slot]));
        }
        if function.eq_ignore_ascii_case("AdjustStrength")
            || function.eq_ignore_ascii_case("AdjustArmor")
        {
            anyhow::ensure!(arguments.len() == 1, "{function}: wrong argument count");
            let delta = arguments[0].integer()?;
            let combat = self.actors[self.index]
                .control
                .as_mut()
                .and_then(|control| control.combat.as_mut())
                .with_context(|| format!("{function}: receiver combat record unavailable"))?;
            let value = if function.eq_ignore_ascii_case("AdjustStrength") {
                &mut combat.strength
            } else {
                &mut combat.armor
            };
            *value = value.wrapping_add(delta);
            return Ok(CallResult::Complete(0));
        }
        if function.eq_ignore_ascii_case("setStrength") || function.eq_ignore_ascii_case("setArmor")
        {
            anyhow::ensure!(arguments.len() == 1, "{function}: wrong argument count");
            let value = arguments[0].integer()?;
            let combat = self.actors[self.index]
                .control
                .as_mut()
                .and_then(|control| control.combat.as_mut())
                .with_context(|| format!("{function}: receiver combat record unavailable"))?;
            if function.eq_ignore_ascii_case("setStrength") {
                combat.strength = value;
            } else {
                combat.armor = value;
            }
            return Ok(CallResult::Complete(0));
        }
        if kind == CallKind::Function && function.eq_ignore_ascii_case("PlayAnimation") {
            let [group, modifier] = arguments else {
                anyhow::bail!("PlayAnimation requires two arguments")
            };
            let group = u16::try_from(group.integer()?)
                .context("PlayAnimation group outside supported range")?;
            anyhow::ensure!(
                group <= i16::MAX as u16,
                "PlayAnimation signed group unsupported"
            );
            // The modifier is stored as one byte.
            let modifier = modifier.integer()? as u8;
            let excluded = self.actors[self.index]
                .control
                .as_ref()
                .is_some_and(|control| control.object_update_excluded);
            if !self.animation_system || excluded || self.actors[self.index].animation.is_none() {
                return Ok(CallResult::Complete(0));
            }
            let ticks = self
                .inputs
                .nominal_ticks
                .context("PlayAnimation requires nominal_ticks")?;
            let position = animation_sound_position(self.spatial.as_deref(), self.index);
            let surface = self.actors[self.index].surface;
            let mut emit = |sound| match &mut self.sounds {
                Some(sounds) => sounds(
                    id,
                    super::ScriptSound::Animation {
                        sound,
                        surface,
                        position,
                    },
                ),
                None => anyhow::bail!("world actor {}: animation sound handler absent", id.index),
            };
            let animation = self.actors[self.index]
                .animation
                .as_mut()
                .expect("checked animation");
            let frame = animation
                .playback
                .request_with_sounds(group, modifier, ticks, &mut emit)?;
            animation.frame = frame.or(animation.frame);
            return Ok(CallResult::Complete(0));
        }
        if function.eq_ignore_ascii_case("GetFramePassed") {
            anyhow::ensure!(arguments.is_empty(), "GetFramePassed requires no arguments");
            let ticks = self
                .inputs
                .nominal_ticks
                .context("GetFramePassed requires nominal_ticks input")?;
            return Ok(CallResult::Complete(
                i32::try_from(ticks).context("GetFramePassed ticks exceed the signed range")?,
            ));
        }
        if function.eq_ignore_ascii_case("InRectangle") {
            let [x1, z1, x2, z2] = arguments else {
                anyhow::bail!("InRectangle requires four arguments")
            };
            let (x1, z1, x2, z2) = (x1.integer()?, z1.integer()?, x2.integer()?, z2.integer()?);
            let position = self.fighting_position(self.index)?;
            let inside = |value: i32, a: i32, b: i32| (a.min(b)..=a.max(b)).contains(&value);
            return Ok(CallResult::Complete(i32::from(
                inside(position[0], x1, x2) && inside(position[2], z1, z2),
            )));
        }
        if function.eq_ignore_ascii_case("SheathSword") {
            anyhow::ensure!(arguments.is_empty(), "SheathSword requires no arguments");
            let selected = self.selected_player() == Some(id);
            let (holding, drawing, sheathing) = self.receiver_weapon_predicates(function)?;
            let actor = &mut self.actors[self.index];
            let Some(control) = actor.control.as_mut() else {
                return Ok(CallResult::Complete(0));
            };
            let eligible = self.task_system
                && actor.animation.is_some()
                && super::combat::sheath_eligible(control, holding, drawing, sheathing);
            if !eligible {
                return Ok(CallResult::Complete(0));
            }
            // The selected player also resets the HUD health-display timer and selects
            // camera slot zero; neither owner is wired here.
            anyhow::ensure!(
                !selected,
                "SheathSword selected-player HUD timer and camera selection unavailable"
            );
            super::combat::begin_sheathing(control);
            return Ok(CallResult::Complete(0));
        }
        if function.eq_ignore_ascii_case("isFighting") {
            anyhow::ensure!(arguments.is_empty(), "{function} requires no arguments");
            return Ok(CallResult::Complete(i32::from(self.is_fighting()?)));
        }
        if function.eq_ignore_ascii_case("isDead") {
            anyhow::ensure!(arguments.is_empty(), "{function} requires no arguments");
            return Ok(CallResult::Complete(i32::from(
                self.actor_is_dead(self.index)?,
            )));
        }
        if function.eq_ignore_ascii_case("Rnd") || function.eq_ignore_ascii_case("QuickRnd") {
            let [operand] = arguments else {
                anyhow::bail!("{function} requires one argument")
            };
            let operand = operand.integer()?;
            let random = self
                .random
                .as_deref_mut()
                .with_context(|| format!("{function}: shared engine random stream unavailable"))?;
            let value = if function.eq_ignore_ascii_case("Rnd") {
                random.rnd(operand)?
            } else {
                random.quick_rnd(operand)
            };
            return Ok(CallResult::Complete(value));
        }
        // Enable*/Disable* clear and set one bit of the script-control byte: 0x01 script,
        // 0x04 tasks, 0x08 object, 0x10 combat (0x02 is the hook, handled above).
        let control_bit = [
            ("Script", 0x01),
            ("Tasks", 0x04),
            ("Object", 0x08),
            ("Combat", 0x10),
        ]
        .into_iter()
        .find_map(|(suffix, bit)| {
            let enable = function.eq_ignore_ascii_case(&format!("Enable{suffix}"));
            (enable || function.eq_ignore_ascii_case(&format!("Disable{suffix}")))
                .then_some((enable, bit))
        });
        if let Some((enable, bit)) = control_bit {
            anyhow::ensure!(arguments.is_empty(), "{function}: wrong argument count");
            let control = self.actors[self.index]
                .control
                .as_mut()
                .with_context(|| format!("{function}: script-control lifecycle unavailable"))?;
            if enable {
                control.script_control &= !bit;
            } else {
                control.script_control |= bit;
            }
            return Ok(CallResult::Complete(0));
        }
        if function.eq_ignore_ascii_case("Sound") {
            let [effect, volume, rate] = arguments else {
                anyhow::bail!("Sound requires three arguments")
            };
            let spatial = self
                .spatial
                .as_deref()
                .context("Sound receiver pose unavailable")?
                .actor(self.index)?;
            let position = if spatial.parent.is_some() {
                spatial
                    .effective
                    .context("Sound attached receiver pose unavailable")?
                    .position
            } else {
                spatial.linked_position.unwrap_or(spatial.local.position)
            };
            self.sounds
                .as_mut()
                .context("script sound handler absent")?(
                id,
                super::ScriptSound::Positional {
                    effect: effect.integer()?,
                    volume: volume.integer()?,
                    rate_offset_hz: rate.integer()?,
                    position,
                },
            )?;
            return Ok(CallResult::Complete(0));
        }
        if let Some(sound) = super::sound::command(function, arguments)? {
            self.sounds
                .as_mut()
                .context("script sound handler absent")?(id, sound)?;
            return Ok(CallResult::Complete(0));
        }
        if self.actors[self.index].runtime_identifier.is_none()
            && [
                "Wander",
                "MoveByAxis",
                "sMoveByAxis",
                "MoveObjectAxis",
                "MoveToLocation",
                "RotateByAxis",
                "sRotateByAxis",
                "RotateToAxis",
            ]
            .iter()
            .any(|name| function.eq_ignore_ascii_case(name))
        {
            anyhow::bail!("{function}: separate movement/relocation owner unavailable");
        }
        if function.eq_ignore_ascii_case("Wander") {
            let [radius, tolerance] = arguments else {
                anyhow::bail!("Wander requires two arguments")
            };
            let radius = radius.integer()?;
            let _tolerance = tolerance.integer()?;
            anyhow::ensure!(
                self.walk_map_count
                    .context("Wander navigation not prepared")?
                    == 0,
                "Wander movement through walk-node maps unsupported"
            );
            let home = self.actors[self.index]
                .placement()
                .context("Wander home unavailable for standalone actor")?
                .map_position;
            let [x, y, z] = home.map(i32::try_from);
            let home = [x?, y?, z?];
            let frame_scale = self
                .inputs
                .frame_scale
                .context("Wander requires frame_scale")?;
            let spatial = self
                .spatial
                .as_deref_mut()
                .context("Wander spatial state not prepared")?
                .actor_mut(self.index)?;
            let random = self
                .random
                .as_deref_mut()
                .context("Wander shared engine random stream unavailable")?;
            let result =
                super::navigation::wander_without_maps(home, radius, frame_scale, spatial, random)?;
            self.actors[self.index].attributes[94] = 1;
            return Ok(CallResult::Complete(result));
        }
        if function.eq_ignore_ascii_case("AtNodeMarker") {
            let [marker, distance] = arguments else {
                anyhow::bail!("AtNodeMarker requires two arguments")
            };
            let position = self
                .spatial
                .as_deref()
                .context("AtNodeMarker spatial state not prepared")?
                .actor(self.index)?
                .local
                .position;
            let result = self
                .markers
                .context("AtNodeMarker markers not prepared")?
                .contains(marker.integer()?, position, distance.integer()?)?;
            return Ok(CallResult::Complete(i32::from(result)));
        }
        if function.eq_ignore_ascii_case("PlayerStand") {
            anyhow::ensure!(arguments.is_empty(), "PlayerStand requires no arguments");
            let player = self
                .selected_player()
                .context("PlayerStand selected player unavailable")?;
            anyhow::ensure!(
                player.generation == self.generation && player.index < self.actors.len(),
                "PlayerStand selected player identity is stale"
            );
            let receiver = ScriptActorId {
                generation: self.generation,
                index: self.index,
            };
            return Ok(CallResult::Complete(i32::from(
                self.actors[player.index].model.support.actor == Some(receiver),
            )));
        }
        // Surface classes 1..=4 are the water classes, 2 and 4 the deep ones, 5 lava.
        let surface_test: Option<fn(u8) -> bool> = match function.to_ascii_lowercase().as_str() {
            "isindeepwater" => Some(|surface| matches!(surface, 2 | 4)),
            "isinwater" => Some(|surface| (1..=4).contains(&surface)),
            "isinlava" => Some(|surface| surface == 5),
            _ => None,
        };
        if let Some(test) = surface_test {
            anyhow::ensure!(arguments.is_empty(), "{function} requires no arguments");
            let surface = self.actors[self.index]
                .surface
                .with_context(|| format!("{function} actor surface state unavailable"))?;
            return Ok(CallResult::Complete(i32::from(test(surface))));
        }
        // Receiver lifecycle bytes: MyAIType is the lifecycle selector, MyAIMode the death
        // notification byte; IsOnEdge holds for living states 10 through 19.
        let control_test: Option<fn(&ActorControlState) -> i32> =
            match function.to_ascii_lowercase().as_str() {
                "myaitype" => Some(|control| i32::from(control.death_selector)),
                "myaimode" => Some(|control| i32::from(control.death_response)),
                "isonedge" => Some(|control| i32::from((10..=19).contains(&control.living_state))),
                // The original also answers 0 while its transition lock (global word read by
                // dozens of routines) is set; cyrus has no transition state, so it is clear.
                "isinair" => Some(|control| {
                    i32::from(
                        control.sliding_state != 0
                            || control.falling_state != 0
                            || control.queued_motion != 0
                            || (27..=29).contains(&control.living_state),
                    )
                }),
                _ => None,
            };
        if let Some(test) = control_test {
            anyhow::ensure!(arguments.is_empty(), "{function} requires no arguments");
            let control = self.actors[self.index]
                .control
                .as_ref()
                .with_context(|| format!("{function}: receiver control state unavailable"))?;
            return Ok(CallResult::Complete(test(control)));
        }
        if function.eq_ignore_ascii_case("PlayerDistance")
            || function.eq_ignore_ascii_case("PlayerFacing")
            || function.eq_ignore_ascii_case("PlayerLooking")
            || function.eq_ignore_ascii_case("PlayerArc")
        {
            let mut inputs = self
                .inputs
                .player_query
                .context("player query inputs unavailable")?;
            let spatial = self
                .spatial
                .as_deref()
                .context("spatial state not prepared")?
                .actor(self.index)?;
            inputs.linked_position = Some(spatial.linked_position);
            return Ok(CallResult::Complete(
                spatial
                    .query(function, arguments, inputs)?
                    .context("spatial query absent")?,
            ));
        }
        if let Some(value) = self.flag_state.call(function, arguments, flags)? {
            return Ok(CallResult::Complete(value));
        }
        if self.spatial.is_some()
            && (function.eq_ignore_ascii_case("SetAttribute")
                || function.eq_ignore_ascii_case("SetMyAttr"))
            && let Some(index) = arguments.first()
        {
            anyhow::ensure!(
                !matches!(index.integer()?, 25..=28 | 33),
                "{function}: attachment/rotation attribute lifecycle unsupported after spatial preparation"
            );
        }
        let actor = &mut self.actors[self.index];
        if let Some(value) = super::attributes::call(
            function,
            arguments,
            &mut actor.attributes,
            actor.control.as_mut(),
        )? {
            return Ok(CallResult::Complete(value));
        }
        anyhow::bail!(
            "native {:?} {function} unsupported by inspection host",
            kind
        )
    }
    fn poll(
        &mut self,
        token: u64,
        arguments: &[Argument],
        invocation: cyrus_script::Invocation,
        _: &mut [i32],
    ) -> Result<bool> {
        let id = ScriptActorId {
            generation: self.generation,
            index: self.index,
        };
        let ambient = self.ambient_call();
        let player_dialogue = self.player_dialogue_call();
        let frame_wait = self.frame_wait_call();
        let locomotion_state = self.locomotion_state();
        let position = animation_sound_position(self.spatial.as_deref(), self.index);
        let surface = self.actors[self.index].surface;
        let mut emit = |sound| match &mut self.sounds {
            Some(sounds) => sounds(
                id,
                super::ScriptSound::Animation {
                    sound,
                    surface,
                    position,
                },
            ),
            None => anyhow::bail!("world actor {}: animation sound handler absent", id.index),
        };
        let actor = &mut self.actors[self.index];
        let mut animation =
            actor
                .animation
                .as_mut()
                .zip(self.inputs.nominal_ticks)
                .map(|(animation, ticks)| super::tasks::AnimationCall {
                    animation: &mut animation.playback,
                    ticks,
                    frame: &mut animation.frame,
                    sounds: Some(&mut emit),
                });
        let player_position = self
            .inputs
            .player_query
            .as_ref()
            .and_then(|query| query.player.as_ref())
            .map(|player| player.position);
        let mut spatial_call = self
            .spatial
            .as_deref_mut()
            .and_then(|spatial| spatial.actor_mut(self.index).ok())
            .zip(self.inputs.frame_scale.zip(self.inputs.nominal_ticks))
            .map(
                |(spatial, (frame_scale, nominal_ticks))| super::tasks::SpatialCall {
                    matrix: spatial.local.matrix,
                    locations: &spatial.locations,
                    effective_position: spatial.effective.map(|pose| pose.position),
                    player_position,
                    slave: actor.attributes[26] != 0,
                    actor_index: self.index,
                    map_updates: self.map_updates,
                    position: &mut spatial.local.position,
                    angles: &mut spatial.local.angles,
                    frame_scale,
                    nominal_ticks,
                },
            );
        actor.tasks.poll(
            token,
            arguments,
            invocation,
            &mut animation,
            &mut spatial_call,
            super::tasks::NativeServices {
                ambient: Some(&ambient),
                dialogue: self.globals.dialogue.as_mut(),
                player_dialogue: Some(player_dialogue),
                frame_wait: Some(frame_wait),
                locomotion: Some(super::tasks::LocomotionCall {
                    state: locomotion_state,
                    blocked_ahead_30: self.inputs.blocked_ahead_30,
                    animation_system: self.animation_system,
                    motion_direction: actor
                        .control
                        .as_mut()
                        .map(|control| &mut control.motion_direction),
                }),
            },
        )
    }
    fn cancel(&mut self, token: u64) -> Result<()> {
        self.actors[self.index].tasks.cancel(token)
    }
    fn route(
        &mut self,
        target: &Target,
        call: &Call,
        strings: &[String],
        source: Option<TaskSource>,
        flags: &mut [i32],
    ) -> Result<Route> {
        let index = self.target_index(target)?;
        if let Some(error) = self.actors[index].failure() {
            anyhow::bail!("{error}");
        }
        anyhow::ensure!(
            self.actors[index].animation.is_none() || self.inputs.nominal_ticks.is_some(),
            "receiver instance {index}: animation requires nominal_ticks"
        );
        if index == self.index {
            return Ok(Route::Here);
        }
        let actor = &mut self.actors[index];
        let mut runtime = actor.runtime.take().with_context(|| {
            format!(
                "receiver instance {index} {}: {}",
                actor.name(),
                actor
                    .unavailable
                    .as_deref()
                    .unwrap_or("script runtime already active")
            )
        })?;
        let caller = std::mem::replace(&mut self.index, index);
        let result = runtime.receive(call, strings, source, self, flags);
        self.index = caller;
        self.actors[index].runtime = Some(runtime);
        result.map(Route::Complete)
    }
    fn task_call(&mut self, origin: CodeOrigin, instruction: u32) -> Result<Call> {
        self.task_calls
            .get(&(origin, instruction))
            .cloned()
            .with_context(|| format!("task source {} instruction {instruction} absent", origin.0))
    }
    fn end(&mut self, _: Thread) -> Result<()> {
        anyhow::ensure!(
            self.actors[self.index].attributes[29] == 0
                && self.actors[self.index].attributes[30] == 0,
            "synchronization channels unsupported"
        );
        Ok(())
    }
}

pub(super) fn clear_unprepared(
    prepared: Option<Res<PreparedWorld>>,
    mut actors: ResMut<ScriptActors>,
    mut status: ResMut<super::ScriptStatus>,
) {
    if prepared.is_some() {
        return;
    }
    actors.release_world();
    actors.requested = None;
    actors.error = None;
    *status = super::ScriptStatus::NotRequested;
}

fn select_combat_taunt<'a>(
    actor: &'a ScriptActor,
    trigger: u8,
    random: &mut crate::random::EngineRandom,
) -> Option<&'a CombatTaunt> {
    let mut requested = trigger;
    loop {
        let mut selected = None;
        for taunt in &actor.combat_taunts {
            if taunt.trigger != requested || taunt.label == [0; 4] {
                continue;
            }
            selected = Some(taunt);
            if 4 * random.draw() / 32767 == 1 {
                break;
            }
        }
        if selected.is_some() || requested != 6 {
            return selected;
        }
        requested = 2;
    }
}

#[cfg(test)]
mod fighting_tests {
    use super::*;

    fn query(
        positions: &[[i32; 3]],
        order: &[usize],
        threshold: i32,
        state: i32,
        dead: bool,
    ) -> bool {
        let receiver = ScriptActorId {
            generation: 1,
            index: 0,
        };
        let mut actors: Vec<_> = (0..=positions.len())
            .map(|index| ScriptActor {
                light: None,
                message_read: 0,
                source: ScriptActorSource::Map(Instance {
                    object_id: index as u32,
                    actor: format!("ACTOR{index}"),
                    object_type: 0,
                    active: 0,
                    model_name: String::new(),
                    model_mode: 0,
                    light_intensity: 0,
                    light_radius: 0,
                    light_rgb: [0; 3],
                    map_position: [0; 3],
                    angles: [0; 3],
                }),
                runtime_identifier: None,
                namespace: None,
                native_hook: None,
                combat_taunts: vec![],
                runtime: None,
                unavailable: None,
                attributes: vec![0; 256],
                control: Some(ActorControlState {
                    combat: (index != 0).then_some(super::super::CombatControlState {
                        target: Some(receiver),
                        attacker_selection_state: state,
                        ..Default::default()
                    }),
                    death_selector: if dead { 8 } else { 0 },
                    ..Default::default()
                }),
                surface: Some(0),
                model: Default::default(),
                collision: None,
                player_movement: None,
                location_offsets: vec![],
                tasks: Default::default(),
                animation: None,
                animation_failure: None,
                update_failure: None,
            })
            .collect();
        let mut spatial = super::super::spatial::SpatialWorld::prepare(&actors).unwrap();
        for (index, &position) in positions.iter().enumerate() {
            spatial.actor_mut(index + 1).unwrap().local.position = position;
        }
        let traversal: Vec<_> = order
            .iter()
            .map(|&index| ScriptActorId {
                generation: 1,
                index,
            })
            .collect();
        let definition = cyrus_script::program::Definition {
            functions: vec![],
            flags: vec![],
            references: vec![],
            attributes: vec![],
        };
        let host = GameHost {
            world: 1,
            animation_system: true,
            task_system: true,
            map_updates: &mut vec![],
            activation: &mut Default::default(),
            globals: &mut Default::default(),
            spatial: Some(&mut spatial),
            markers: None,
            walk_map_count: None,
            inputs: Default::default(),
            index: 0,
            generation: 1,
            sounds: None,
            random: None,
            actors: &mut actors,
            ordinary_actors: Some(&traversal),
            in_combat_threshold: Some(threshold),
            task_calls: &BTreeMap::new(),
            reference_names: &definition.references,
            names: &BTreeMap::new(),
            flag_state: &mut super::super::flags::Flags::prepare(&definition).unwrap(),
        };
        host.is_fighting().unwrap()
    }

    #[test]
    fn fighting_selection_preserves_order_and_uses_three_dimensions_before_horizontal_test() {
        let positions = [[0, 10, 0], [10, 0, 0]];
        assert!(query(&positions, &[1, 2], 5, 1, false));
        assert!(!query(&positions, &[2, 1], 5, 1, false));
        assert!(!query(&[[0, 11, 0], [10, 0, 0]], &[1, 2], 5, 1, false));
        assert!(query(&[[0, 9, 0], [10, 0, 0]], &[2, 1], 5, 1, false));
    }

    #[test]
    fn fighting_selection_checks_strict_bounds_exact_state_and_death() {
        assert!(!query(&[[10, 0, 0]], &[1], 10, 1, false));
        assert!(query(&[[10, 0, 0]], &[1], 11, 1, false));
        assert!(!query(&[[65535, 0, 0]], &[1], 100000, 1, false));
        assert!(query(&[[65534, 0, 0]], &[1], 100000, 1, false));
        for state in [0, 2, -1] {
            assert!(!query(&[[1, 0, 0]], &[1], 10, state, false));
        }
        assert!(!query(&[[1, 0, 0]], &[1], 10, 1, true));
        assert!(!query(&[[1, 0, 0]], &[], 10, 1, false));
    }
}
