//! Logical script instances retain authored object order and reject stale world identities.
use anyhow::{Context, Result};
use bevy::prelude::{Assets, Commands, DetectChanges, Handle, Res, ResMut, Resource};
use cyrus_assets::{
    animation::Animations,
    script::{Call, CallKind, Instance, Operation, Scripts},
};
use cyrus_script::{
    Argument, CallResult, CodeOrigin, Host, Route, Runtime, Target, TaskSource, Thread,
};
use cyrus_world::lifecycle::PreparedWorld;
use std::collections::BTreeMap;
use std::sync::Arc;

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
    pub activation: Option<&'a super::ActivationInputs>,
    pub nominal_ticks: Option<u32>,
    pub frame_scale: Option<u32>,
    pub player_query: Option<super::QueryInputs>,
}

pub(crate) struct ScriptAnimation {
    pub(crate) playback: cyrus_world::animation::ActorAnimation,
    pub(crate) frame: Option<u32>,
}

/// Allocated ordinary-map control state, separate from animation playback.
/// Allocation and combat construction follow preservation's engine/animation.md.
/// Standalone role setup and death/removal transitions remain unsupported.
#[derive(Debug, serde::Serialize)]
pub struct ActorControlState {
    /// Runtime script flags, separate from installed attribute bytes.
    pub script_control: u8,
    pub death_selector: u8,
    pub living_state: u8,
    pub rope_attached: bool,
    pub torch_burning: u8,
    pub weapon_drawn: bool,
    pub object_update_excluded: bool,
    pub combat_death_default: Option<i32>,
}

pub struct ScriptActor {
    pub source: ScriptActorSource,
    /// Signed low word used by namespace lookup; absent for separate map object paths.
    pub runtime_identifier: Option<i16>,
    pub namespace: Option<String>,
    native_hook: Option<Call>,
    runtime: Option<Runtime>,
    unavailable: Option<String>,
    attributes: Vec<u8>,
    control: Option<ActorControlState>,
    tasks: super::tasks::NativeTasks,
    animation: Option<ScriptAnimation>,
    animation_failure: Option<String>,
}
impl ScriptActor {
    pub fn name(&self) -> &str {
        self.source.name()
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
    /// Known ordinary-map allocation state; standalone roles have no established state.
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
        self.animation_failure
            .as_deref()
            .or_else(|| {
                self.animation
                    .as_ref()
                    .and_then(|animation| animation.playback.failure())
            })
            .or_else(|| self.runtime.as_ref().and_then(Runtime::failure))
    }
    pub fn unavailable(&self) -> Option<&str> {
        self.unavailable.as_deref()
    }
}

/// Explicit inspection state. Preparation does not grant ordinary update eligibility.
pub struct ScriptWorld {
    pub world: u32,
    pub content_id: String,
    generation: u64,
    flags: Vec<i32>,
    flag_state: super::flags::Flags,
    updates: cyrus_assets::script::UpdateSettings,
    actors: Vec<ScriptActor>,
    spatial: Option<super::spatial::SpatialWorld>,
    activation: super::ActivationState,
    task_calls: BTreeMap<(CodeOrigin, u32), Call>,
    names: BTreeMap<i16, usize>,
    actor_lists: Option<super::order::ActorLists>,
    markers: Option<super::markers::Markers>,
}
impl ScriptWorld {
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
                anyhow::ensure!(
                    runtime_identifier.is_some(),
                    "separate map object path unsupported"
                );
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
                    Some(ActorControlState {
                        script_control: 0,
                        death_selector: 0,
                        living_state: 0,
                        rope_attached: false,
                        torch_burning: 0,
                        weapon_drawn: false,
                        object_update_excluded: false,
                        combat_death_default: None,
                    })
                }
                _ => None,
            };
            ScriptActor {
                source,
                control,
                runtime_identifier,
                namespace: metadata.and_then(|actor| actor.namespace.clone()),
                native_hook: metadata.and_then(|actor| actor.native_hook.clone()),
                runtime,
                unavailable,
                tasks: Default::default(),
                animation: None,
                animation_failure: None,
                attributes: (0..256)
                    .map(|index| {
                        metadata
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
        Ok(Self {
            updates: scripts.updates,
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
            names,
            actor_lists: None,
            markers: None,
            spatial: None,
            activation: Default::default(),
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
    /// Separate map paths and standalone roles remain unavailable for spatial services.
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
            &mut |sound| sounds(id, super::ScriptSound::Animation(sound)),
        );
        match result {
            Ok((playback, frame)) => {
                actor
                    .control
                    .as_mut()
                    .expect("type-one map actor has allocation state")
                    .combat_death_default = (actor.attributes[14] != 0).then_some(0);
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
        let result = animation
            .playback
            .select_default_with_sounds(group, ticks, &mut |sound| {
                sounds(id, super::ScriptSound::Animation(sound))
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
        let actor = &mut self.actors[index];
        if let Some(error) = actor.failure() {
            anyhow::bail!("world {} player: {error}", self.world);
        }
        if actor.animation.is_none() {
            match crate::player::placement::initial_animation(data, ticks, sounds) {
                Ok((playback, frame)) => {
                    actor.animation = Some(ScriptAnimation { playback, frame })
                }
                Err(error) => {
                    actor.animation_failure = Some(format!("{error:#}"));
                    return Err(error);
                }
            }
        }
        Ok(actor
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
            0,
        )
    }
    /// Invokes one explicit stage against the actor's shared native services.
    /// Worlds with any animation-bound actor require nominal ticks, including zero. Sound effects
    /// are dispatched synchronously with the receiving actor's identity.
    pub fn advance(
        &mut self,
        id: ScriptActorId,
        stage: ScriptStage,
        inputs: ScriptInputs<'_>,
        sounds: Option<&mut dyn FnMut(ScriptActorId, super::ScriptSound) -> Result<()>>,
        budget: usize,
    ) -> Result<()> {
        let index = self.resolve(id)?;
        if let Some(activation) = inputs.activation {
            self.validate_activation(activation)?;
        }
        let nominal_ticks = inputs.nominal_ticks;
        if matches!(stage, ScriptStage::Tasks | ScriptStage::OrdinaryMain) {
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
            return Ok(());
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
        let bypass_tasks = if matches!(stage, ScriptStage::OrdinaryMain) {
            let control = self.actors[index].control.as_ref().with_context(|| {
                format!(
                    "world {} instance {index}: script-control lifecycle unavailable",
                    self.world
                )
            })?;
            if control.script_control & 1 != 0 {
                return Ok(());
            }
            control.script_control & 4 != 0
        } else {
            false
        };
        let mut host = GameHost {
            index,
            generation: self.generation,
            actors: &mut self.actors,
            task_calls: &self.task_calls,
            names: &self.names,
            flag_state: &mut self.flag_state,
            sounds,
            spatial: self.spatial.as_mut(),
            markers: self.markers.as_ref(),
            activation: &mut self.activation,
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
            ScriptStage::OrdinaryMain => runtime.invoke_main_stage(
                &mut host,
                &mut self.flags,
                inputs.frame_scale.unwrap(),
                budget,
                bypass_tasks,
            ),
            ScriptStage::NativeHook | ScriptStage::OrdinaryHook => match native_hook {
                Some(call) => runtime
                    .invoke_native_hook(&call, &mut host, &mut self.flags)
                    .map(|_| ()),
                None => Ok(()),
            },
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
        result
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
            "activation": self.activation,
            "actors": self.actors.iter().enumerate().skip(offset).take(limit.min(64)).map(|(index, actor)| serde_json::json!({
                "id": ScriptActorId { generation: self.generation, index }, "name": actor.name(),
                "source": actor.source.label(), "runtime_identifier": actor.runtime_identifier,
                "object_id": actor.placement().map(|placement| placement.object_id), "unavailable": actor.unavailable,
                "main": actor.runtime.as_ref().and_then(|runtime| runtime.program_counter(Thread::Main)),
                "interrupt": actor.runtime.as_ref().and_then(|runtime| runtime.program_counter(Thread::Interrupt)),
                "locals": actor.runtime.as_ref().map(|runtime| runtime.variables()),
                "attributes": actor.attributes,
                "control": actor.control,
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

#[derive(Resource, Default)]
pub struct ScriptActors {
    pub current: Option<ScriptWorld>,
    generation: u64,
    requested: Option<Handle<Scripts>>,
    pub(super) error: Option<String>,
}
pub(super) fn prepare(
    prepared: Res<PreparedWorld>,
    assets: Res<Assets<Scripts>>,
    animations: Res<Assets<Animations>>,
    mut actors: ResMut<ScriptActors>,
    clock: Option<Res<crate::clock::SimulationClock>>,
    mut commands: Commands,
) {
    if prepared.is_changed() {
        actors.current = None;
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
        let mut world = ScriptWorld::prepare(scripts, actors.generation)?;
        world.prepare_markers(animation_data)?;
        if clock.is_none() {
            commands.insert_resource(crate::clock::SimulationClock::new(scripts.clock)?);
        }
        Ok(world)
    })();
    match result {
        Ok(world) => actors.current = Some(world),
        Err(error) => {
            actors.error = Some(format!(
                "world {} script preparation: {error:#}",
                scripts.world
            ));
            actors.requested = None;
        }
    }
}

struct GameHost<'a, 'sound, 'input> {
    activation: &'a mut super::ActivationState,
    spatial: Option<&'a mut super::spatial::SpatialWorld>,
    markers: Option<&'a super::markers::Markers>,
    inputs: ScriptInputs<'input>,
    index: usize,
    generation: u64,
    sounds: Option<&'sound mut dyn FnMut(ScriptActorId, super::ScriptSound) -> Result<()>>,
    actors: &'a mut [ScriptActor],
    task_calls: &'a BTreeMap<(CodeOrigin, u32), Call>,
    names: &'a BTreeMap<i16, usize>,
    flag_state: &'a mut super::flags::Flags,
}
impl GameHost<'_, '_, '_> {
    fn advance_animation(&mut self) -> Result<()> {
        let id = ScriptActorId {
            generation: self.generation,
            index: self.index,
        };
        let ticks = self
            .inputs
            .nominal_ticks
            .context("animation requires nominal_ticks")?;
        let mut emit = |sound| match &mut self.sounds {
            Some(sounds) => sounds(id, super::ScriptSound::Animation(sound)),
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
impl Host for GameHost<'_, '_, '_> {
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
        if kind == CallKind::Function {
            let selected_player = self
                .inputs
                .activation
                .map(|inputs| &self.actors[inputs.player.index]);
            if let Some(value) = self.activation.call(
                id,
                function,
                arguments,
                self.inputs.activation,
                &self.actors[self.index].attributes,
                selected_player,
            )? {
                return Ok(CallResult::Complete(value));
            }
        }
        let mut emit = |sound| match &mut self.sounds {
            Some(sounds) => sounds(id, super::ScriptSound::Animation(sound)),
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
        let mut spatial_call = self
            .spatial
            .as_deref_mut()
            .and_then(|spatial| spatial.actor_mut(self.index).ok())
            .zip(self.inputs.frame_scale.zip(self.inputs.nominal_ticks))
            .map(
                |(spatial, (frame_scale, nominal_ticks))| super::tasks::SpatialCall {
                    position: &mut spatial.local.position,
                    angles: &mut spatial.local.angles,
                    frame_scale,
                    nominal_ticks,
                },
            );
        if let Some(result) = actor.tasks.call(
            function,
            kind,
            arguments,
            invocation,
            &mut animation,
            &mut spatial_call,
        )? {
            return Ok(result);
        }
        anyhow::ensure!(
            kind == CallKind::Function,
            "native {:?} {function} unsupported by inspection host",
            kind
        );
        if function.eq_ignore_ascii_case("EnableScript")
            || function.eq_ignore_ascii_case("DisableScript")
        {
            anyhow::ensure!(arguments.is_empty(), "{function}: wrong argument count");
            let control = self.actors[self.index]
                .control
                .as_mut()
                .with_context(|| format!("{function}: script-control lifecycle unavailable"))?;
            if function.eq_ignore_ascii_case("EnableScript") {
                control.script_control &= !1;
            } else {
                control.script_control |= 1;
            }
            return Ok(CallResult::Complete(0));
        }
        if let Some(sound) = super::sound::command(function, arguments)? {
            self.sounds
                .as_mut()
                .context("script sound handler absent")?(id, sound)?;
            return Ok(CallResult::Complete(0));
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
        if function.eq_ignore_ascii_case("PlayerDistance")
            || function.eq_ignore_ascii_case("PlayerFacing")
        {
            let inputs = self
                .inputs
                .player_query
                .context("player query inputs unavailable")?;
            let spatial = self
                .spatial
                .as_deref()
                .context("spatial state not prepared")?
                .actor(self.index)?;
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
        let mut emit = |sound| match &mut self.sounds {
            Some(sounds) => sounds(id, super::ScriptSound::Animation(sound)),
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
        let mut spatial_call = self
            .spatial
            .as_deref_mut()
            .and_then(|spatial| spatial.actor_mut(self.index).ok())
            .zip(self.inputs.frame_scale.zip(self.inputs.nominal_ticks))
            .map(
                |(spatial, (frame_scale, nominal_ticks))| super::tasks::SpatialCall {
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
    actors.current = None;
    actors.requested = None;
    actors.error = None;
    *status = super::ScriptStatus::NotRequested;
}
