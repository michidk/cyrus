//! Complete actor and world replacements are assembled away from live execution.
use super::*;
use crate::script::JumpMotion;
use crate::script::LivingState;
use anyhow::ensure;
use cyrus_assets::script::PlacementType;
use serde::{Deserialize, Serialize};

/// The AI record fields beyond the location word.
#[derive(Clone, Copy, serde::Serialize, serde::Deserialize)]
struct AiRecordSnapshot {
    var2: i16,
    length: i32,
}

#[derive(Clone, Serialize, Deserialize)]
struct ControlSnapshot {
    #[serde(default)]
    navigation: super::super::ActorNavigation,
    point_command: super::super::PointCommandState,
    script_control: u8,
    death_selector: u8,
    death_response: u8,
    #[serde(default)]
    ai_delay: u8,
    #[serde(default)]
    ai_location: i16,
    /// The remaining AI record fields; older saves restore the allocation defaults.
    #[serde(default)]
    ai_record: Option<AiRecordSnapshot>,
    living_state: LivingState,
    motion_companion: u8,
    #[serde(default)]
    extra_time: u8,
    sliding_state: u8,
    falling_state: JumpMotion,
    queued_motion: JumpMotion,
    motion_direction: crate::script::MotionDirection,
    motion_classification: crate::script::MotionDirection,
    directional_state: [u32; 2],
    action_transition_counter: u8,
    #[serde(default)]
    jump_window: u8,
    movement_flags: u8,
    motion_vector: [i32; 3],
    /// The held ledge point; saves written before it lived in the actor kept it in the session globals.
    #[serde(default)]
    ledge_point: [i32; 3],
    #[serde(default)]
    hand_point: [i32; 3],
    #[serde(default)]
    hand_angles: [i32; 3],
    fall_origin_y: i32,
    fall_timer: u8,
    jump_selector: u8,
    stable_position_valid: bool,
    stable_local_position: [i32; 3],
    sliding_distance: i32,
    body_response_flags: u8,
    collision_transition_flags: u8,
    collision_response_auxiliary: [u8; 2],
    collision_response_age: u8,
    overlap_contact_identifier: u16,
    rope_attached: bool,
    /// Saves written before the rope speed shared the `move` triple kept it here.
    #[serde(default, skip_serializing)]
    rope_speed: i32,
    torch_burning: u8,
    #[serde(default)]
    torch_saved: u8,
    weapon_drawn: bool,
    held_item: Option<u8>,
    object_update_excluded: bool,
    hidden: bool,
    #[serde(default)]
    scabbard_override: [Option<i16>; 2],
    #[serde(default)]
    killed: bool,
    #[serde(default)]
    creator: Option<usize>,
    combat_death_default: Option<i32>,
    combat: Option<CombatSnapshot>,
}

#[derive(Clone, Serialize, Deserialize)]
struct CombatSnapshot {
    contact_profile: cyrus_assets::animation::CombatContactProfile,
    attack_indices: [Option<u8>; 9],
    defense_indices: [Option<u8>; 4],
    target: Option<(u64, usize)>,
    #[serde(default)]
    retained_target_position: [i32; 3],
    #[serde(default)]
    close_distance_passes: i32,
    lost_sight_ticks: i32,
    lost_sight_routing: i32,
    flags: u8,
    action: i32,
    cooldown: u32,
    allocation_index: i32,
    attacker_selection_state: i32,
    retained_action_indices: [i32; 2],
    taunt_cache: Option<usize>,
    taunt_dialogue: Option<usize>,
    taunt_active: i32,
    taunt_period: i32,
    spacing_ranges: [i32; 3],
    spacing_period: i32,
    #[serde(default)]
    strength: i32,
    #[serde(default)]
    armor: i32,
    #[serde(default)]
    decision: super::super::combat::CombatDecision,
}

impl ControlSnapshot {
    fn capture(control: &ActorControlState) -> Self {
        let last_collide = control.last_collide.bits().to_le_bytes();
        Self {
            navigation: control.navigation,
            point_command: control.point_command,
            script_control: control.script_control,
            death_selector: control.ai_type.byte(),
            death_response: control.ai_mode,
            ai_delay: control.ai_delay,
            ai_location: control.ai.var1,
            ai_record: Some(AiRecordSnapshot {
                var2: control.ai.var2,
                length: control.ai.length,
            }),
            living_state: control.living_state,
            motion_companion: control.motion_companion,
            extra_time: control.extra_time,
            sliding_state: control.sliding_state,
            falling_state: control.falling_state,
            queued_motion: control.queued_motion,
            motion_direction: control.motion_direction,
            motion_classification: control.motion_classification,
            directional_state: control.actions.into(),
            action_transition_counter: control.action_transition_counter,
            jump_window: control.jump_window,
            movement_flags: control.move_counter,
            motion_vector: control.motion_vector,
            ledge_point: control.ledge_point,
            hand_point: control.hand_point,
            hand_angles: control.hand_angles,
            fall_origin_y: control.fall_origin_y,
            fall_timer: control.fall_timer,
            jump_selector: control.jump_selector,
            stable_position_valid: control.stable_position_valid,
            stable_local_position: control.stable_local_position,
            sliding_distance: control.sliding_distance,
            // Snapshots keep the word's bytes in their earlier fields.
            body_response_flags: last_collide[1],
            collision_transition_flags: last_collide[2],
            collision_response_auxiliary: [last_collide[0], last_collide[3]],
            collision_response_age: control.last_collide_count,
            overlap_contact_identifier: control.overlap_contact_identifier,
            rope_attached: control.rope_attached,
            rope_speed: 0,
            torch_burning: control.torch_burning,
            torch_saved: control.torch_saved,
            weapon_drawn: control.weapon_drawn,
            held_item: control.held_item,
            object_update_excluded: control.object_update_excluded,
            hidden: control.hidden,
            scabbard_override: control.scabbard_override,
            killed: control.killed,
            creator: control.creator,
            combat_death_default: control.combat_death_default,
            combat: control.combat.as_ref().map(CombatSnapshot::capture),
        }
    }

    fn restore(
        self,
        rebind: &impl Fn(u64, usize) -> Result<ScriptActorId>,
        saved_generation: u64,
    ) -> Result<ActorControlState> {
        // Created-object parents are stored as indices because they are only used inside
        // the enclosing world. Validate that cross-actor reference against the saved
        // generation before restoring it, just like combat targets.
        let creator = self
            .creator
            .map(|index| {
                rebind(saved_generation, index)
                    .map(|_| index)
                    .with_context(|| format!("saved creator actor {index} is stale or absent"))
            })
            .transpose()?;
        Ok(ActorControlState {
            navigation: self.navigation,
            point_command: self.point_command,
            script_control: self.script_control,
            ai_type: crate::script::AiType::from_byte(self.death_selector),
            ai_mode: self.death_response,
            ai_delay: self.ai_delay,
            ai: {
                let defaults = super::AiRecord::default();
                let saved = self.ai_record;
                super::AiRecord {
                    var1: self.ai_location,
                    var2: saved.map_or(defaults.var2, |saved| saved.var2),
                    length: saved.map_or(defaults.length, |saved| saved.length),
                }
            },
            living_state: self.living_state,
            motion_companion: self.motion_companion,
            extra_time: self.extra_time,
            sliding_state: self.sliding_state,
            falling_state: self.falling_state,
            queued_motion: self.queued_motion,
            motion_direction: self.motion_direction,
            motion_classification: self.motion_classification,
            actions: self.directional_state.into(),
            action_transition_counter: self.action_transition_counter,
            jump_window: self.jump_window,
            move_counter: self.movement_flags,
            ledge_point: self.ledge_point,
            hand_point: self.hand_point,
            hand_angles: self.hand_angles,
            motion_vector: match self.rope_speed {
                0 => self.motion_vector,
                speed => [self.motion_vector[0], self.motion_vector[1], speed],
            },
            fall_origin_y: self.fall_origin_y,
            fall_timer: self.fall_timer,
            jump_selector: self.jump_selector,
            stable_position_valid: self.stable_position_valid,
            stable_local_position: self.stable_local_position,
            sliding_distance: self.sliding_distance,
            last_collide: crate::script::CollisionFlags::from_bits(u32::from_le_bytes([
                self.collision_response_auxiliary[0],
                self.body_response_flags,
                self.collision_transition_flags,
                self.collision_response_auxiliary[1],
            ])),
            last_collide_count: self.collision_response_age,
            overlap_contact_identifier: self.overlap_contact_identifier,
            ground_kind: Default::default(),
            rope_attached: self.rope_attached,
            torch_burning: self.torch_burning,
            torch_saved: self.torch_saved,
            weapon_drawn: self.weapon_drawn,
            held_item: self.held_item,
            object_update_excluded: self.object_update_excluded,
            hidden: self.hidden,
            scabbard_override: self.scabbard_override,
            killed: self.killed,
            creator,
            combat_death_default: self.combat_death_default,
            combat: self.combat.map(|saved| saved.restore(rebind)).transpose()?,
        })
    }
}
impl CombatSnapshot {
    fn capture(combat: &super::super::CombatControlState) -> Self {
        Self {
            contact_profile: combat.contact_profile,
            attack_indices: combat.attack_indices,
            defense_indices: combat.defense_indices,
            target: combat.target.map(|id| (id.generation, id.index)),
            retained_target_position: combat.retained_target_position,
            close_distance_passes: combat.close_distance_passes,
            lost_sight_ticks: combat.lost_sight_ticks,
            lost_sight_routing: combat.lost_sight_routing,
            flags: combat.flags.bits(),
            action: combat.action,
            cooldown: combat.cooldown,
            allocation_index: combat.allocation_index,
            attacker_selection_state: combat.attacker_selection_state,
            retained_action_indices: combat.retained_action_indices,
            taunt_cache: combat.taunt_cache,
            taunt_dialogue: combat.taunt_dialogue,
            taunt_active: combat.taunt_active,
            taunt_period: combat.taunt_period,
            spacing_ranges: combat.spacing_ranges,
            spacing_period: combat.spacing_period,
            strength: combat.strength,
            armor: combat.armor,
            decision: combat.decision,
        }
    }

    fn restore(
        self,
        rebind: &impl Fn(u64, usize) -> Result<ScriptActorId>,
    ) -> Result<super::super::CombatControlState> {
        Ok(super::super::CombatControlState {
            contact_profile: self.contact_profile,
            attack_indices: self.attack_indices,
            defense_indices: self.defense_indices,
            target: self
                .target
                .map(|(generation, index)| rebind(generation, index))
                .transpose()?,
            retained_target_position: self.retained_target_position,
            close_distance_passes: self.close_distance_passes,
            lost_sight_ticks: self.lost_sight_ticks,
            lost_sight_routing: self.lost_sight_routing,
            flags: crate::script::CombatFlags::from_bits(self.flags),
            action: self.action,
            cooldown: self.cooldown,
            allocation_index: self.allocation_index,
            attacker_selection_state: self.attacker_selection_state,
            retained_action_indices: self.retained_action_indices,
            taunt_cache: self.taunt_cache,
            taunt_dialogue: self.taunt_dialogue,
            taunt_active: self.taunt_active,
            taunt_period: self.taunt_period,
            spacing_ranges: self.spacing_ranges,
            spacing_period: self.spacing_period,
            strength: self.strength,
            armor: self.armor,
            decision: self.decision,
        })
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct CollisionSnapshot {
    positive_extent: [i32; 3],
    negative_extent: [i32; 3],
    #[serde(default)]
    center_offset: [i32; 3],
    body: Vec<([i32; 3], i32)>,
    additional: Option<([i32; 3], i32)>,
}
impl CollisionSnapshot {
    fn capture(shape: &ground::OrdinaryCollisionShape) -> Self {
        Self {
            positive_extent: shape.positive_extent,
            negative_extent: shape.negative_extent,
            center_offset: shape.center_offset,
            body: shape
                .body
                .iter()
                .map(|sphere| (sphere.center, sphere.radius))
                .collect(),
            additional: shape
                .additional
                .map(|sphere| (sphere.center, sphere.radius)),
        }
    }

    fn restore(self) -> Result<ground::OrdinaryCollisionShape> {
        let sphere = |(center, radius)| -> Result<_> {
            ensure!(radius >= 0, "saved collision sphere has negative radius");
            Ok(cyrus_collision::actor_shape::BodySphere { center, radius })
        };
        Ok(ground::OrdinaryCollisionShape {
            positive_extent: self.positive_extent,
            negative_extent: self.negative_extent,
            center_offset: self.center_offset,
            body: self.body.into_iter().map(sphere).collect::<Result<_>>()?,
            additional: self.additional.map(sphere).transpose()?,
        })
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct AnimationSnapshot {
    playback: cyrus_world::animation::ActorAnimationSnapshot,
    frame: Option<u32>,
}

#[derive(Clone, Serialize, Deserialize)]
struct ActorSnapshot {
    name: String,
    source: String,
    runtime_identifier: Option<i16>,
    message_read: u8,
    #[serde(default)]
    broadcast_index: u8,
    runtime: Option<cyrus_soup::persistence::Snapshot>,
    unavailable: Option<String>,
    attributes: cyrus_soup::Attributes,
    control: Option<ControlSnapshot>,
    surface: Option<cyrus_assets::surface::Surface>,
    model: model::ModelSnapshot,
    collision: Option<CollisionSnapshot>,
    player_movement: Option<player::PlayerMovementSnapshot>,
    light: Option<super::light::LightState>,
    /// Whether the actor is a type-six map placement, the only kind projected as a map light.
    /// Absent in older documents, which projected every enabled light.
    #[serde(default)]
    map_light: Option<bool>,
    #[serde(default)]
    flat: Option<super::flat::FlatState>,
    tasks: super::super::NativeTasksSnapshot,
    native_commands: [super::super::tasks::NativeTaskCommandState; 6],

    #[serde(default)]
    unprepared_animation_default: i16,
    animation: Option<AnimationSnapshot>,
    animation_failure: Option<String>,
    update_failure: Option<String>,
}
impl ScriptWorld {
    /// Recreate the shells and created objects that were started after preparation, in saved
    /// order, so the saved actor census matches. A created object is placed at the origin with
    /// its own saved spatial state restored afterwards; anything else beyond the installed census
    /// stays a mismatch.
    fn restore_started_shells(&mut self, saved: &[ActorSnapshot]) -> Result<()> {
        for extra in saved.iter().skip(self.actors.len()) {
            match extra.source.as_str() {
                "shell" => {
                    self.spawn_shell(&extra.name)?;
                }
                "created" => {
                    self.restore_created_placeholder(&extra.name)?;
                }
                _ => anyhow::bail!("world {}: saved actor census differs", self.world),
            }
        }
        Ok(())
    }
}

impl ActorSnapshot {
    fn capture(
        actor: &ScriptActor,
        sources: &BTreeMap<CodeOrigin, Arc<cyrus_soup::program::Program>>,
    ) -> Result<Self> {
        Ok(Self {
            name: actor.name().to_owned(),
            source: actor.source.label().to_owned(),
            runtime_identifier: actor.runtime_identifier,
            message_read: actor.message_read,
            broadcast_index: actor.broadcast_index,
            runtime: actor
                .runtime
                .as_ref()
                .map(|runtime| runtime.snapshot(sources))
                .transpose()?,
            unavailable: actor.unavailable.clone(),
            attributes: actor.attributes.clone(),
            control: actor.control.as_ref().map(ControlSnapshot::capture),
            surface: actor.surface,
            light: actor.light.clone(),
            map_light: Some(
                actor.placement().is_some_and(|placement| {
                    placement.object_type == PlacementType::COLOURED_LIGHT
                }),
            ),
            flat: actor.flat.clone(),
            model: actor.model.snapshot(),
            collision: actor.collision.as_ref().map(CollisionSnapshot::capture),
            player_movement: actor
                .player_movement
                .as_ref()
                .map(player::PlayerMovement::snapshot),
            tasks: actor.tasks.snapshot(),
            native_commands: actor.native_commands,
            unprepared_animation_default: actor.unprepared_animation_default,
            animation: actor.animation.as_ref().map(|animation| AnimationSnapshot {
                playback: animation.playback.snapshot(),
                frame: animation.frame,
            }),
            animation_failure: actor.animation_failure.clone(),
            update_failure: actor.update_failure.clone(),
        })
    }
}

struct ActorRestoreContext<'a, F> {
    index: usize,
    saved_generation: u64,
    scripts: &'a Scripts,
    animations: &'a Animations,
    sources: &'a BTreeMap<CodeOrigin, Arc<cyrus_soup::program::Program>>,
    static_count: Option<usize>,
    rebind: &'a F,
    dialogue: Option<&'a super::super::DialogueState>,
    samples: &'a BTreeMap<u8, u8>,
}
impl ActorSnapshot {
    fn restore(
        self,
        mut actor: ScriptActor,
        context: &ActorRestoreContext<'_, impl Fn(u64, usize) -> Result<ScriptActorId>>,
    ) -> Result<ScriptActor> {
        ensure!(
            self.name == actor.name()
                && self.source == actor.source.label()
                && self.runtime_identifier == actor.runtime_identifier,
            "saved actor {}: installed allocation identity differs",
            self.name
        );
        ensure!(
            self.control.is_some() == actor.control.is_some(),
            "saved actor {}: control allocation differs",
            self.name
        );
        // A shell or created object that ended with `KillMe` keeps no executable.
        let finished_separate = self.runtime.is_none()
            && matches!(
                actor.source,
                ScriptActorSource::Shell(_) | ScriptActorSource::Created(_)
            );
        ensure!(
            !finished_separate
                || actor.runtime.is_none()
                || !matches!(actor.source, ScriptActorSource::Created(_))
                || self.control.as_ref().is_some_and(|control| control.killed),
            "saved actor {}: created actor executable absent without killed cleanup",
            self.name
        );
        ensure!(
            (self.runtime.is_some() == actor.runtime.is_some() || finished_separate)
                && self.unavailable == actor.unavailable,
            "saved actor {}: executable availability differs",
            self.name
        );
        ensure!(
            self.player_movement.is_none()
                || matches!(
                    actor.source,
                    ScriptActorSource::Player | ScriptActorSource::AlternatePlayer
                ),
            "saved actor {}: player movement on another role",
            self.name
        );
        ensure!(
            self.collision.is_none() || actor.ordinary_collision_role(),
            "saved actor {}: collision shape on unsupported role",
            self.name
        );
        if let Some(light) = &self.light {
            if let Some(base) = light.starting_rgb {
                let placement = actor
                    .placement()
                    .filter(|placement| placement.object_type == PlacementType::COLOURED_LIGHT)
                    .with_context(|| {
                        format!(
                            "saved actor {}: light starting RGB has no authored source",
                            self.name
                        )
                    })?;
                ensure!(
                    base == placement.light_rgb,
                    "saved actor {}: light starting RGB source differs",
                    self.name
                );
            }
            // Retained source coordinates identify the authored record; drawing uses the
            // current spatial pose and offset.
            if let Some(placement) = actor.placement().filter(|placement| {
                matches!(
                    placement.object_type,
                    PlacementType::LIGHT | PlacementType::COLOURED_LIGHT
                )
            }) {
                ensure!(
                    light.position == placement.map_position.map(|value| value as i32),
                    "saved actor {}: object light source position differs",
                    self.name
                );
            }
        }
        let animation = self
            .animation
            .map(|saved| -> Result<_> {
                if let Some(reference) = saved.frame {
                    match &actor.source {
                        ScriptActorSource::Map(_) => {
                            context
                                .animations
                                .resolve_map_frame(context.index, reference)?;
                        }
                        _ => {
                            context.animations.resolve_frame(actor.name(), reference)?;
                        }
                    }
                }
                Ok(ScriptAnimation {
                    playback: cyrus_world::animation::ActorAnimation::from_snapshot(
                        context.animations,
                        actor.name(),
                        saved.playback,
                    )?,
                    frame: saved.frame,
                })
            })
            .transpose()?;
        let (runtime, tasks) = if let Some(mut vm) = self.runtime {
            let tokens: BTreeMap<_, _> = vm.task_tokens().map(|token| (token, token)).collect();
            let tasks = super::super::NativeTasks::from_snapshot(
                self.tasks,
                &vm,
                &tokens,
                super::super::NativeTaskBindings {
                    animation: animation.as_ref().map(|animation| &animation.playback),
                    dialogue: context.dialogue,
                    ambient_channels: Some(context.samples),
                },
            )?;
            let definition = context
                .scripts
                .actors
                .get(actor.name())
                .context("saved actor program absent")?;
            vm.rebind_actor_references(|reference| {
                (context.rebind)(reference.generation, reference.index)
            })?;
            let runtime = Runtime::from_snapshot(
                CodeOrigin(definition.code_origin),
                vm,
                context.sources,
                &tokens,
                format!("world {} actor {}", context.scripts.world, actor.name()),
            )?;
            (Some(runtime), tasks)
        } else {
            (
                None,
                super::super::NativeTasks::from_snapshot_without_vm(self.tasks)?,
            )
        };
        actor.message_read = self.message_read;
        actor.broadcast_index = self.broadcast_index;
        actor.runtime = runtime;
        actor.tasks = tasks;
        actor.native_commands = self.native_commands;
        actor.attributes = self.attributes;
        actor.control = self
            .control
            .map(|saved| saved.restore(context.rebind, context.saved_generation))
            .transpose()?;
        actor.surface = self.surface;
        actor.light = self.light;
        actor.flat = self.flat;
        actor.model = model::ModelState::from_snapshot(
            self.model,
            context.animations,
            context.static_count,
            context.rebind,
        )?;
        actor.collision = self.collision.map(CollisionSnapshot::restore).transpose()?;
        actor.player_movement = self
            .player_movement
            .map(player::PlayerMovement::from_snapshot)
            .transpose()?;
        actor.unprepared_animation_default = self.unprepared_animation_default;
        actor.animation = animation;
        actor.animation_failure = self.animation_failure;
        actor.update_failure = self.update_failure;
        // Snapshots are taken outside invocation, so absent runtime identifies completed cleanup.
        actor.created_retired = matches!(actor.source, ScriptActorSource::Created(_))
            && actor.runtime.is_none()
            && actor.control.as_ref().is_some_and(|control| control.killed);
        Ok(actor)
    }
}

/// Immutable inputs required by a world candidate. The enclosing session owns them.
pub struct WorldSnapshotSources<'a> {
    /// Exact installed effect images and pool settings, independent of live slot state.
    pub effects: Option<&'a cyrus_assets::effects::Effects>,
    /// Decoded player inputs when this world has a prepared player scene.
    /// A complete gameplay session supplies this for every retained world.
    pub player: Option<&'a crate::player::data::PlayerScene>,
    pub scripts: &'a Scripts,
    pub animations: &'a Animations,
    pub collision: &'a cyrus_assets::collision::CollisionScene,
    /// `identity()` of exactly these inputs when the owner already computed it. Sources are
    /// immutable once decoded, so an owner may reuse it; leave `None` to compute on demand.
    pub identity_memo: Option<String>,
}
impl WorldSnapshotSources<'_> {
    /// Validate and fingerprint the complete decoded source set used by registry restore.
    /// Includes collision, optional player/effect content, scripts and animation.
    /// Absent effect inputs preserve the identity used by older pool-free documents.
    pub fn identity(&self) -> Result<String> {
        if let Some(identity) = &self.identity_memo {
            return Ok(identity.clone());
        }
        struct Digest(blake3::Hasher);
        impl std::io::Write for Digest {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.0.update(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        self.scripts.validate()?;
        self.animations.validate()?;
        ensure!(
            self.scripts.world == self.animations.world
                && self.animations.metadata["script_content_id"].as_str()
                    == Some(&self.scripts.content_id),
            "world {}: save source generations differ",
            self.scripts.world
        );
        if let Some(effects) = self.effects {
            effects.validate()?;
        }
        let player_identity = self
            .player
            .map(|player| -> Result<&str> {
                ensure!(
                    player.world == self.scripts.world,
                    "world {}: player source world differs",
                    self.scripts.world
                );
                player.source_identity.as_deref().with_context(|| {
                    format!(
                        "world {}: player source identity unavailable",
                        self.scripts.world
                    )
                })
            })
            .transpose()?;
        let mut digest = Digest(blake3::Hasher::new());
        let mut buffered = std::io::BufWriter::new(&mut digest);
        serde_json::to_writer(
            &mut buffered,
            &(
                self.scripts,
                self.animations,
                self.collision,
                player_identity,
            ),
        )?;
        if let Some(effects) = self.effects {
            serde_json::to_writer(&mut buffered, effects)?;
        }
        std::io::Write::flush(&mut buffered)?;
        drop(buffered);
        Ok(digest.0.finalize().to_hex().to_string())
    }

    fn programs(&self) -> Result<BTreeMap<CodeOrigin, Arc<cyrus_soup::program::Program>>> {
        let mut sources = BTreeMap::new();
        for actor in self.scripts.actors.values() {
            if let Some(previous) = sources.insert(
                CodeOrigin(actor.code_origin),
                Arc::new(actor.program.clone()),
            ) {
                ensure!(
                    *previous == actor.program,
                    "world {}: aliased program content differs",
                    self.scripts.world
                );
            }
        }
        Ok(sources)
    }
}

/// Ordered logical world continuation, excluding the separately owned session globals.
/// A complete save also needs visited worlds, HUD, clock/RNG and audio state.
#[derive(Clone, Serialize, Deserialize)]
pub struct WorldSnapshot {
    version: u32,
    world: u32,
    content_id: String,
    source_identity: String,
    generation: u64,
    flags: super::super::GlobalFlagsSnapshot,
    actors: Vec<ActorSnapshot>,
    spatial: Option<super::super::spatial::SpatialSnapshot>,
    activation: super::super::activation::ActivationSnapshot,
    actor_lists: Option<super::super::order::ActorListsSnapshot>,
    markers_prepared: bool,
    walk_nodes: Option<super::super::navigation::WalkNodeSnapshot>,
    next_combat_index: i32,
    ordinary_prepared: bool,
    #[serde(default)]
    actor_prelighting: Option<BTreeMap<usize, super::ActorPrelight>>,
    model_admission: Option<model::ModelAdmissionSnapshot>,
    map_updates: Vec<usize>,
    surface_veto: Option<bool>,
    camera: Option<super::super::camera::CameraSnapshot>,
    #[serde(default)]
    effects: Option<super::effects::EffectsSnapshot>,
    /// Dynamic state of each rope in construction order; older saves restore ropes at rest.
    #[serde(default)]
    ropes: Vec<cyrus_game::rope::RopeState>,
}
impl WorldSnapshot {
    fn capture(world: &ScriptWorld, sources: &WorldSnapshotSources<'_>) -> Result<Self> {
        ensure!(
            world.world == sources.scripts.world && world.content_id == sources.scripts.content_id,
            "world {}: snapshot source identity differs",
            world.world
        );
        let source_identity = sources.identity()?;
        let programs = sources.programs()?;
        Ok(Self {
            effects: world
                .effects
                .as_ref()
                .map(|effects| {
                    effects.snapshot(
                        sources
                            .effects
                            .context("world effect snapshot source unavailable")?,
                    )
                })
                .transpose()
                .with_context(|| format!("world {} environmental effect capture", world.world))?,
            ropes: world
                .ropes
                .iter()
                .map(cyrus_game::rope::Rope::state)
                .collect(),
            version: 1,
            world: world.world,
            content_id: world.content_id.clone(),
            source_identity,
            generation: world.generation,
            flags: world.snapshot_flags(),
            actors: world
                .actors
                .iter()
                .map(|actor| ActorSnapshot::capture(actor, &programs))
                .collect::<Result<_>>()?,
            spatial: world
                .spatial
                .as_ref()
                .map(super::super::spatial::SpatialWorld::snapshot),
            activation: world.activation.snapshot(),
            actor_lists: world
                .actor_lists
                .as_ref()
                .map(super::super::order::ActorLists::snapshot),
            markers_prepared: world.markers.is_some(),
            walk_nodes: world
                .walk_nodes
                .as_ref()
                .map(super::super::navigation::WalkNodeReservations::snapshot),
            next_combat_index: world.next_combat_index,
            ordinary_prepared: world.ordinary_prepared,
            actor_prelighting: world
                .actor_prelighting
                .as_ref()
                .map(|_| world.actor_prelighting_inputs())
                .transpose()?,
            model_admission: world
                .model_admission
                .as_ref()
                .map(model::AdmissionState::snapshot),
            map_updates: world.map_updates.as_slice().to_vec(),
            surface_veto: world.surface_veto,
            camera: world
                .camera
                .as_ref()
                .map(super::super::camera::CameraState::snapshot),
        })
    }

    fn restore(
        self,
        sources: &WorldSnapshotSources<'_>,
        generation: u64,
        globals: &super::super::GameplayGlobals,
        samples: &BTreeMap<u8, u8>,
    ) -> Result<ScriptWorld> {
        ensure!(
            self.source_identity == sources.identity()?,
            "world {}: saved source content differs",
            self.world
        );
        self.restore_validated_source(sources, generation, globals, samples)
    }

    // Registry plans retain immutable source borrows after checking their full identities.
    fn restore_validated_source(
        self,
        sources: &WorldSnapshotSources<'_>,
        generation: u64,
        globals: &super::super::GameplayGlobals,
        samples: &BTreeMap<u8, u8>,
    ) -> Result<ScriptWorld> {
        ensure!(
            self.version == 1,
            "world snapshot version {} unsupported",
            self.version
        );
        ensure!(
            self.world == sources.scripts.world && self.content_id == sources.scripts.content_id,
            "world {}: saved source content differs",
            self.world
        );
        ensure!(
            generation != self.generation,
            "world {}: replacement must use a fresh actor generation",
            self.world
        );
        let mut candidate = ScriptWorld::prepare(sources.scripts, generation)?;
        candidate.restore_started_shells(&self.actors)?;
        ensure!(
            self.actors.len() == candidate.actors.len(),
            "world {}: saved actor census differs",
            self.world
        );
        let count = candidate.actors.len();
        let rebind = |old_generation, index| -> Result<ScriptActorId> {
            ensure!(
                old_generation == self.generation && index < count,
                "world {} actor {old_generation}/{index}: stale or absent saved owner",
                self.world
            );
            Ok(ScriptActorId { generation, index })
        };
        candidate.spatial = self
            .spatial
            .map(|saved| {
                super::super::spatial::SpatialWorld::from_snapshot(saved, &candidate.actors)
            })
            .transpose()?;
        candidate.actor_lists = self
            .actor_lists
            .map(|saved| {
                super::super::order::ActorLists::from_snapshot(
                    saved,
                    &candidate.actors,
                    self.generation,
                    generation,
                )
            })
            .transpose()?;
        candidate.model_admission = self
            .model_admission
            .map(|saved| {
                let visibility = sources
                    .collision
                    .model_visibility
                    .as_ref()
                    .context("saved model admission: installed visibility absent")?;
                model::AdmissionState::from_snapshot(
                    saved,
                    visibility,
                    sources.collision.placements.len(),
                    &rebind,
                )
            })
            .transpose()?;
        candidate.markers = self
            .markers_prepared
            .then(|| super::super::markers::Markers::prepare(&sources.animations.metadata))
            .transpose()?;
        candidate.walk_nodes = self
            .walk_nodes
            .map(|saved| {
                super::super::navigation::WalkNodeReservations::from_snapshot(
                    saved,
                    &sources.animations.metadata["walk_node_maps"],
                    self.world,
                )
            })
            .transpose()?;
        let programs = sources.programs()?;
        let static_count = candidate
            .model_admission
            .as_ref()
            .map(|_| sources.collision.placements.len());
        candidate.actors = self
            .actors
            .into_iter()
            .zip(candidate.actors)
            .enumerate()
            .map(|(index, (saved, actor))| {
                saved
                    .restore(
                        actor,
                        &ActorRestoreContext {
                            index,
                            saved_generation: self.generation,
                            scripts: sources.scripts,
                            animations: sources.animations,
                            sources: &programs,
                            static_count,
                            rebind: &rebind,
                            dialogue: globals.dialogue.as_ref(),
                            samples,
                        },
                    )
                    .with_context(|| format!("world {} actor {index} replacement", self.world))
            })
            .collect::<Result<_>>()?;
        let selected_player = super::selected_player_index(&candidate.actors, globals);
        // Saves written before the held ledge point lived in the actor kept it in the session globals.
        if let (Some((point, heading)), Some(index)) =
            (globals.legacy_ledge_anchor, selected_player)
            && let Some(control) = candidate.actors[index].control.as_mut()
            && control.living_state.is_ledge()
            && control.ledge_point == [0; 3]
        {
            control.ledge_point = point;
            control.motion_vector[1] = i32::from(heading);
        }
        for (index, actor) in candidate.actors.iter().enumerate() {
            actor
                .tasks
                .validate_actor_references(|reference| {
                    candidate.resolve(rebind(self.generation, reference as usize)?)?;
                    ensure!(
                        Some(reference as usize) != selected_player,
                        "lineup reference is the selected player"
                    );
                    Ok(())
                })
                .with_context(|| {
                    format!("world {} actor {index} saved task references", self.world)
                })?;
            if let Some(control) = &actor.control {
                if let Some(nodes) = &candidate.walk_nodes {
                    nodes.check_state(&control.navigation).with_context(|| {
                        format!("world {} actor {index} saved navigation", self.world)
                    })?;
                } else {
                    ensure!(
                        control.navigation == super::super::ActorNavigation::default(),
                        "world {} actor {index}: saved navigation without prepared maps",
                        self.world
                    );
                }
            }
        }
        candidate.restore_flags(self.flags)?;
        candidate.activation =
            super::super::ActivationState::from_snapshot(self.activation, &rebind)?;
        let mut seen = std::collections::BTreeSet::new();
        for &index in &self.map_updates {
            ensure!(
                index < count && seen.insert(index),
                "world {}: invalid pending map update {index}",
                self.world
            );
        }
        candidate.prepare_ropes(sources.animations)?;
        ensure!(
            self.ropes.is_empty() || self.ropes.len() == candidate.ropes.len(),
            "world {}: saved ropes differ from the installed records",
            self.world
        );
        for (rope, saved) in candidate.ropes.iter_mut().zip(&self.ropes) {
            rope.restore_state(saved);
        }
        candidate.map_updates.restore(self.map_updates);
        candidate.next_combat_index = self.next_combat_index;
        candidate.ordinary_prepared = self.ordinary_prepared;
        if let Some(saved) = self.actor_prelighting {
            ensure!(
                saved == candidate.actor_prelighting_inputs()?,
                "world {}: saved actor prelight checkpoint differs from restored owners",
                self.world
            );
            for (&index, input) in &saved {
                ensure!(
                    sources
                        .collision
                        .actor_models
                        .as_ref()
                        .and_then(|models| models.instances.get(&index))
                        .is_some_and(|model| model == input.model()),
                    "world {} actor {index}: saved prelight model differs from installed source",
                    self.world
                );
            }
            candidate.actor_prelighting = Some(saved);
        }
        candidate.surface_veto = self.surface_veto;
        ensure!(
            self.camera.is_some() == candidate.camera.is_some(),
            "world {}: saved camera session differs from the actor census",
            self.world
        );
        candidate.camera = self
            .camera
            .map(|saved| super::super::camera::CameraState::from_snapshot(saved, &rebind))
            .transpose()
            .with_context(|| format!("world {} camera replacement", self.world))?;
        candidate.effects = self
            .effects
            .map(|saved| {
                EnvironmentalEffects::from_snapshot(
                    saved,
                    sources
                        .effects
                        .context("saved environmental effect source unavailable")?,
                )
            })
            .transpose()
            .with_context(|| format!("world {} environmental effect replacement", self.world))?;
        candidate.globals = super::super::GameplayGlobals::default();
        Ok(candidate)
    }
}

/// The registry portion of a session document; globals occur once for all worlds.
#[derive(Clone, Serialize, Deserialize)]
struct RegistrySnapshot {
    version: u32,
    current: u32,
    worlds: Vec<WorldSnapshot>,
    globals: super::super::activation::GameplayGlobalsSnapshot,
    error: Option<String>,
}

struct RegistryCandidate {
    current: ScriptWorld,
    visited: BTreeMap<u32, ScriptWorld>,
    generation: u64,
    error: Option<String>,
}
impl RegistryCandidate {
    fn into_registry(self) -> ScriptActors {
        let restored_generation = Some(self.current.generation());
        ScriptActors {
            world_loop_outcome: None,
            current: Some(self.current),
            visited: self.visited,
            discard_current_on_release: false,
            retained_flags: None,
            retained_health: None,
            restored_generation,
            generation: self.generation,
            requested: None,
            globals: super::super::GameplayGlobals::default(),
            error: self.error,
        }
    }
}

impl RegistrySnapshot {
    fn capture(
        current: &ScriptWorld,
        visited: &BTreeMap<u32, ScriptWorld>,
        sources: &BTreeMap<u32, WorldSnapshotSources<'_>>,
        error: Option<String>,
    ) -> Result<Self> {
        ensure!(
            !visited.contains_key(&current.world),
            "current world also appears in visited worlds"
        );
        let mut generations = std::collections::BTreeSet::new();
        let mut worlds = Vec::with_capacity(visited.len() + 1);
        for world in std::iter::once(current).chain(visited.values()) {
            ensure!(
                generations.insert(world.generation),
                "world {}: duplicate retained generation",
                world.world
            );
            let source = sources
                .get(&world.world)
                .with_context(|| format!("world {}: save sources absent", world.world))?;
            worlds.push(WorldSnapshot::capture(world, source)?);
        }
        for (&key, world) in visited {
            ensure!(
                key == world.world,
                "visited world key {key} differs from retained identity"
            );
        }
        Ok(Self {
            version: 1,
            current: current.world,
            worlds,
            globals: current.globals.snapshot(),
            error,
        })
    }

    fn plan<'a>(
        self,
        sources: &'a BTreeMap<u32, WorldSnapshotSources<'a>>,
        last_generation: u64,
    ) -> Result<RegistryRestorePlan<'a>> {
        ensure!(
            self.version == 1,
            "registry snapshot version {} unsupported",
            self.version
        );
        let mut identities = std::collections::BTreeSet::new();
        let mut generations = std::collections::BTreeSet::new();
        let mut bindings = Vec::new();
        let mut replacements = BTreeMap::new();
        let mut generation = self
            .worlds
            .iter()
            .map(|world| world.generation)
            .fold(last_generation, u64::max);
        for saved in &self.worlds {
            ensure!(
                identities.insert(saved.world),
                "duplicate saved world {}",
                saved.world
            );
            ensure!(
                generations.insert(saved.generation),
                "duplicate saved world generation {}",
                saved.generation
            );
            let source = sources
                .get(&saved.world)
                .with_context(|| format!("world {}: restore sources absent", saved.world))?;
            ensure!(
                saved.content_id == source.scripts.content_id
                    && saved.source_identity == source.identity()?,
                "world {}: saved content differs",
                saved.world
            );
            let mut installed = ScriptWorld::prepare(source.scripts, saved.generation)?;
            installed.restore_started_shells(&saved.actors)?;
            ensure!(
                saved.actors.len() == installed.actors.len(),
                "world {}: saved actor census differs",
                saved.world
            );
            for (index, (actor, expected)) in saved.actors.iter().zip(&installed.actors).enumerate()
            {
                ensure!(
                    actor.name == expected.name()
                        && actor.source == expected.source.label()
                        && actor.runtime_identifier == expected.runtime_identifier,
                    "world {} actor {index}: saved allocation identity differs",
                    saved.world
                );
            }
            // Reserve every destination identity before resolving session references.
            generation = generation
                .checked_add(1)
                .context("session actor generation exhausted")?;
            replacements.insert(saved.world, generation);
            for index in 0..saved.actors.len() {
                bindings.push((
                    ScriptActorId {
                        generation: saved.generation,
                        index,
                    },
                    ScriptActorId { generation, index },
                ));
            }
        }
        ensure!(
            identities.contains(&self.current),
            "saved current world {} absent",
            self.current
        );
        let current_source = &sources[&self.current];
        for &world in &identities {
            ensure!(
                sources[&world].scripts.dialogue == current_source.scripts.dialogue,
                "world {world}: session dialogue settings differ"
            );
        }
        Ok(RegistryRestorePlan {
            saved: self,
            sources,
            bindings,
            replacements,
            generation,
        })
    }
}

/// Owns a checked snapshot and all reserved identities while services are prepared.
struct RegistryRestorePlan<'a> {
    saved: RegistrySnapshot,
    sources: &'a BTreeMap<u32, WorldSnapshotSources<'a>>,
    bindings: Vec<(ScriptActorId, ScriptActorId)>,
    replacements: BTreeMap<u32, u64>,
    generation: u64,
}
impl RegistryRestorePlan<'_> {
    fn actor_bindings(&self) -> &[(ScriptActorId, ScriptActorId)] {
        &self.bindings
    }

    fn restore(self, samples: &BTreeMap<u8, u8>) -> Result<RegistryCandidate> {
        let Self {
            saved,
            sources,
            bindings,
            replacements,
            generation,
        } = self;
        let current_source = &sources[&saved.current];
        let globals = super::super::GameplayGlobals::from_snapshot(
            saved.globals,
            &current_source.scripts.dialogue,
            &bindings,
            samples,
        )?;
        let mut current = None;
        let mut visited = BTreeMap::new();
        for world_snapshot in saved.worlds {
            let world = world_snapshot.world;
            let candidate = world_snapshot.restore_validated_source(
                &sources[&world],
                replacements[&world],
                &globals,
                samples,
            )?;
            if world == saved.current {
                current = Some(candidate);
            } else {
                visited.insert(world, candidate);
            }
        }
        let mut current = current.context("restored current world absent")?;
        current.globals = globals;
        // Backfill membership for snapshots written before channel registration was retained.
        // Existing entries win so restored actors keep their creation-time channel.
        current.globals.group_sync.register_actors(&current.actors);
        Ok(RegistryCandidate {
            current,
            visited,
            generation,
            error: saved.error,
        })
    }
}

#[derive(Clone, Serialize, Deserialize)]
/// Logical session document. Audio, presentation and host lifecycle are separate owners.
/// Retains the selected rope jump latch independently of temporary world snapshots.
/// Legacy documents without that owner are accepted only while the selected player is detached.
pub struct LogicalSessionSnapshot {
    version: u32,
    registry: RegistrySnapshot,
    hud: super::super::ScriptHudSnapshot,
    clock: crate::clock::ClockSnapshot,
    random: u32,
    toggles: crate::player::input::PlayerToggles,
    #[serde(default)]
    selected_rope_jump_latch: Option<bool>,
}
/// Fully validated logical resources, ready for the host's coordinated session commit.
/// Constructing this value neither executes scripts nor replaces live resources.
pub struct LogicalSessionCandidate {
    pub actors: ScriptActors,
    pub hud: super::super::ScriptHud,
    pub clock: crate::clock::SimulationClock,
    pub random: cyrus_game::random::EngineRandom,
    pub toggles: crate::player::input::PlayerToggles,
}
/// Installed sources and current host epoch for logical replacement validation.
pub struct LogicalSessionRestoreInputs<'a> {
    pub sources: &'a BTreeMap<u32, WorldSnapshotSources<'a>>,
    pub last_generation: u64,
    pub hud: &'a cyrus_assets::hud::HudAsset,
    pub preferences: &'a cyrus_game::preferences::Preferences,
    pub bios_tick: u32,
}
impl LogicalSessionSnapshot {
    /// Destination actor-light inputs for hidden material preparation. These were
    /// captured from saved model owners, never from the currently displayed world.
    /// Older snapshots with eligible static actors require a new save.
    pub fn actor_prelighting(&self) -> Result<BTreeMap<usize, super::ActorPrelight>> {
        let current = self
            .registry
            .worlds
            .iter()
            .find(|world| world.world == self.registry.current)
            .context("saved actor prelight world absent")?;
        if let Some(inputs) = &current.actor_prelighting {
            return Ok(inputs.clone());
        }
        ensure!(
            !current.actors.iter().any(|actor| {
                actor.attributes[cyrus_soup::Attribute::NonMoving] != 0
                    && actor.model.has_static_model()
            }),
            "world {}: legacy actor prelight checkpoint unavailable; save again",
            current.world
        );
        Ok(BTreeMap::new())
    }
    /// Saved enabled map lights (type-six placements, as `ScriptWorld::object_lights` projects)
    /// for off-live material preparation. Source and identity validation still belongs to the
    /// complete replacement plan.
    pub fn object_lights(&self) -> Result<Vec<cyrus_assets::environment::LocalLight>> {
        let current = self
            .registry
            .worlds
            .iter()
            .find(|world| world.world == self.registry.current)
            .context("saved light world absent")?;
        let mut lights = Vec::new();
        for (index, actor) in current.actors.iter().enumerate() {
            let Some(light) = &actor.light else {
                continue;
            };
            if actor.map_light == Some(false) {
                continue;
            }
            let pose = current
                .spatial
                .as_ref()
                .map(|spatial| spatial.actor_pose(index))
                .transpose()?
                .flatten();
            if let Some(rendered) = match pose {
                Some(pose) => light.rendered_pose(pose),
                None => light.rendered(),
            } {
                lights.push(rendered);
            }
        }
        // The saved type-two effect records submit through the colour the last object light left.
        let rgb = lights.last().map_or([0; 3], |light| light.rgb);
        lights.extend(
            current
                .effects
                .iter()
                .flat_map(|effects| effects.light_effects())
                .filter(|light| light.radius.current > 0)
                .map(|light| cyrus_assets::environment::LocalLight {
                    position: light.position,
                    radius: light.radius.current as i16,
                    rgb,
                }),
        );
        Ok(lights)
    }

    /// World assets the host must prepare before validating a replacement.
    pub fn world_ids(&self) -> impl ExactSizeIterator<Item = u32> + '_ {
        self.registry.worlds.iter().map(|world| world.world)
    }

    /// Worlds whose retained effect pools require installed environment inputs.
    /// These are preparation hints until the complete document has been validated.
    pub fn environmental_effect_worlds(&self) -> impl Iterator<Item = u32> + '_ {
        self.registry
            .worlds
            .iter()
            .filter(|world| world.effects.is_some())
            .map(|world| world.world)
    }

    /// Saved active world. This is an unvalidated document value until `plan` succeeds.
    pub fn current_world(&self) -> u32 {
        self.registry.current
    }

    fn capture(
        registry: RegistrySnapshot,
        selected_rope_jump_latch: bool,
        hud: &super::super::ScriptHud,
        clock: &crate::clock::SimulationClock,
        random: &cyrus_game::random::EngineRandom,
        toggles: crate::player::input::PlayerToggles,
    ) -> Result<Self> {
        Ok(Self {
            version: 1,
            registry,
            selected_rope_jump_latch: Some(selected_rope_jump_latch),
            hud: hud.snapshot()?,
            clock: clock.snapshot(),
            random: random.state(),
            toggles,
        })
    }

    /// Validate immutable owners and reserve actor identities before audio rebinding.
    pub fn plan<'a>(
        self,
        inputs: LogicalSessionRestoreInputs<'a>,
    ) -> Result<LogicalSessionRestorePlan<'a>> {
        let LogicalSessionRestoreInputs {
            sources,
            last_generation,
            hud,
            preferences,
            bios_tick,
        } = inputs;
        ensure!(
            self.version == 1,
            "session snapshot version {} unsupported",
            self.version
        );
        // No resource is replaced until every owner has produced its candidate.
        let hud = super::super::ScriptHud::from_snapshot(hud, preferences, self.hud)?;
        let clock = crate::clock::SimulationClock::from_snapshot(self.clock, bios_tick)?;
        let random = cyrus_game::random::EngineRandom::from_state(self.random);
        let registry = self.registry.plan(sources, last_generation)?;
        Ok(LogicalSessionRestorePlan {
            selected_rope_jump_latch: self.selected_rope_jump_latch,
            registry,
            hud,
            clock,
            random,
            toggles: self.toggles,
        })
    }
}

/// Off-live logical assembly while the host prepares audio and presentation owners.
pub struct LogicalSessionRestorePlan<'a> {
    selected_rope_jump_latch: Option<bool>,
    registry: RegistryRestorePlan<'a>,
    hud: super::super::ScriptHud,
    clock: crate::clock::SimulationClock,
    random: cyrus_game::random::EngineRandom,
    toggles: crate::player::input::PlayerToggles,
}
impl LogicalSessionRestorePlan<'_> {
    /// Complete supplied source identities, reusing checked world fingerprints while
    /// the plan retains immutable source borrows. Unrepresented extra sources are
    /// fingerprinted here; callers still validate independently mutable live assets.
    pub fn source_identities(&self) -> Result<BTreeMap<u32, String>> {
        let mut identities: BTreeMap<_, _> = self
            .registry
            .saved
            .worlds
            .iter()
            .map(|world| (world.world, world.source_identity.clone()))
            .collect();
        for (&world, source) in self.registry.sources {
            if let std::collections::btree_map::Entry::Vacant(entry) = identities.entry(world) {
                entry.insert(source.identity()?);
            }
        }
        Ok(identities)
    }

    /// Complete saved-to-replacement actor identities for service reconstruction.
    pub fn actor_bindings(&self) -> &[(ScriptActorId, ScriptActorId)] {
        self.registry.actor_bindings()
    }

    /// Consume bindings only after the owning audio service has prepared its replacement.
    pub fn finish(self, samples: &BTreeMap<u8, u8>) -> Result<LogicalSessionCandidate> {
        let mut registry = self.registry.restore(samples)?;
        registry
            .current
            .restore_session_rope_jump_latch(self.selected_rope_jump_latch)?;
        Ok(LogicalSessionCandidate {
            actors: registry.into_registry(),
            hud: self.hud,
            clock: self.clock,
            random: self.random,
            toggles: self.toggles,
        })
    }
}

impl ScriptActors {
    /// The loaded entry still needs its retained actor state and service cleanup.
    pub fn has_pending_revisit(&self) -> bool {
        self.current
            .as_ref()
            .is_some_and(|world| self.visited.contains_key(&world.world))
    }

    /// Highest actor generation allocated by this registry, including released worlds.
    pub fn generation_high_water(&self) -> u64 {
        self.generation
            .max(self.current.as_ref().map_or(0, ScriptWorld::generation))
            .max(
                self.visited
                    .values()
                    .map(ScriptWorld::generation)
                    .max()
                    .unwrap_or(0),
            )
    }

    /// Detached logical worlds that must be included in the same session document.
    pub fn visited_worlds(&self) -> &BTreeMap<u32, ScriptWorld> {
        &self.visited
    }

    /// Scheduler-boundary capture; world loading must have finished.
    pub fn snapshot_session(
        &self,
        sources: &BTreeMap<u32, WorldSnapshotSources<'_>>,
        hud: &super::super::ScriptHud,
        clock: &crate::clock::SimulationClock,
        random: &cyrus_game::random::EngineRandom,
        toggles: crate::player::input::PlayerToggles,
    ) -> Result<LogicalSessionSnapshot> {
        ensure!(
            !self.discard_current_on_release,
            "session snapshot: new-game transition in progress"
        );
        let current = self
            .current
            .as_ref()
            .context("session snapshot: no current world or world loading incomplete")?;
        LogicalSessionSnapshot::capture(
            RegistrySnapshot::capture(current, &self.visited, sources, self.error.clone())?,
            current.session_rope_jump_latch()?,
            hud,
            clock,
            random,
            toggles,
        )
    }
}

/// Host-prepared session ownership for off-live world reconstruction.
/// Sample bindings must refer to already validated replacement audio slots.
pub struct WorldRestoreContext<'a> {
    pub generation: u64,
    pub globals: super::super::GameplayGlobals,
    pub samples: &'a BTreeMap<u8, u8>,
}
impl ScriptWorld {
    /// Capture at a completed scheduler boundary without scripts, cues or initialization.
    /// Session globals are captured separately once across current and visited worlds.
    pub fn snapshot(&self, sources: &WorldSnapshotSources<'_>) -> Result<WorldSnapshot> {
        WorldSnapshot::capture(self, sources)
    }

    /// Validate a complete logical world replacement without modifying a live owner.
    /// The host must commit it together with the corresponding session and audio owners.
    pub fn from_snapshot(
        snapshot: WorldSnapshot,
        sources: &WorldSnapshotSources<'_>,
        context: WorldRestoreContext<'_>,
    ) -> Result<Self> {
        let mut candidate = snapshot.restore(
            sources,
            context.generation,
            &context.globals,
            context.samples,
        )?;
        candidate.globals = context.globals;
        Ok(candidate)
    }
}

impl ScriptActors {
    /// Replace a freshly initialized entry with its retained logical world.
    /// The host clears shared RTX, dialogue and sample services synchronously;
    /// saved task counters and remembered slot numbers remain unchanged.
    /// Player records use the freshly initialized destination pose after restoration.
    /// No dormant transport or dialogue owner is reconstructed.
    pub fn restore_visited(
        &mut self,
        sources: &WorldSnapshotSources<'_>,
        cleanup: &mut dyn FnMut(
            super::super::SoundResetStage,
            &mut super::super::GameplayGlobals,
        ) -> Result<()>,
    ) -> Result<bool> {
        ensure!(
            self.error.is_none(),
            "revisit registry has a retained failure"
        );
        let current = self
            .current
            .as_ref()
            .context("revisit current world absent")?;
        let world_id = current.world;
        let Some(retained) = self.visited.get(&world_id) else {
            return Ok(false);
        };
        let snapshot = retained.snapshot(sources)?;
        let samples = (0..16).map(|slot| (slot, slot)).collect();
        let destination = current
            .selected_player()
            .map(|player| current.spatial(player).map(|spatial| spatial.local))
            .transpose()?;
        let mut replacement =
            snapshot.restore(sources, current.generation(), &current.globals, &samples)?;
        // Temporary world restoration precedes destination placement (preservation player entry).
        if let Some(destination) = destination {
            let spatial = replacement
                .spatial
                .as_mut()
                .context("revisit restored spatial state absent")?;
            for (index, actor) in replacement.actors.iter().enumerate() {
                if matches!(
                    actor.source,
                    ScriptActorSource::Player | ScriptActorSource::AlternatePlayer
                ) {
                    spatial
                        .place_player(
                            index,
                            actor,
                            destination.position,
                            destination.wrapped_angles(),
                            true,
                        )
                        .with_context(|| {
                            format!(
                                "world {world_id} instance {index}: revisit destination placement"
                            )
                        })?;
                }
            }
        }
        if replacement.model_resources_prepared() {
            replacement.actor_prelighting = None;
            replacement.prepare_actor_prelighting()?;
        }
        let mut globals = std::mem::take(&mut self.current.as_mut().unwrap().globals);
        let mut maintenance = std::mem::take(&mut globals.sound_maintenance);
        let result = maintenance.restore_world(&mut |stage| cleanup(stage, &mut globals), || {
            self.world_loop_outcome = None;
            self.current = Some(replacement);
            Ok(())
        });
        globals.sound_maintenance = maintenance;
        self.current.as_mut().unwrap().globals = globals;
        result?;
        self.visited.remove(&world_id);
        Ok(true)
    }
}
