//! Automatic ordinary map stages share the installed clock and logical actor state.
use super::{ScriptActorId, ScriptActors, ScriptInputs, ScriptStage, ScriptWorld};
use crate::clock::{ClockStep, SimulationClock, bios_tick};
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use cyrus_assets::animation::Animations;
use cyrus_world::{animation::AnimationRequest, sound::SoundCue};

/// Current presentation view in original Q8 coordinates, tied to its world asset.
/// A missing view suspends automatic traversal; explicit inspection remains available.
#[derive(Resource)]
pub struct ScriptView {
    pub source: Handle<Animations>,
    pub position: [i32; 3],
}

/// Automatic world work must precede pose presentation and follow view preparation.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OrdinaryUpdates;

/// Model selection/positioning precedes scene submission and ordinary simulation.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ModelPreparation;

/// Both phases borrow the same synchronous host services; only simulation advances actors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrdinaryPhase {
    ModelPresentation,
    Simulation,
}

/// Timing admitted for this host update, removed while ordinary world work is paused.
/// A restored or otherwise changed SimulationClock alone is not a sampled update.
#[derive(Resource)]
pub struct WorldClockSample {
    generation: u64,
    step: ClockStep,
    models_prepared: bool,
}
impl WorldClockSample {
    /// Current update timing only when it belongs to the supplied logical generation.
    /// Submission is admitted only after this sample's model services complete.
    pub fn presentation_step(&self, generation: u64) -> Option<ClockStep> {
        self.models_prepared
            .then(|| self.step(generation))
            .flatten()
    }

    pub fn step(&self, generation: u64) -> Option<ClockStep> {
        (self.generation == generation).then_some(self.step)
    }
}

/// Timing/services and matching world geometry for one ordinary traversal.
/// Missing geometry cannot admit an actor's collision stages.
pub struct OrdinaryInputs<'a> {
    pub scripts: ScriptInputs<'a>,
    pub scene: Option<&'a crate::player::data::PlayerScene>,
}

impl ScriptWorld {
    pub(super) fn prepare_ordinary_models(
        &mut self,
        data: &Animations,
        ticks: u32,
        sounds: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        if self.spatial.is_none() {
            self.prepare_spatial()?;
        }
        if !self.ordinary_prepared {
            self.ordinary_prepared = true;
            for index in 0..self.actors.len() {
                let actor = &self.actors[index];
                if actor.animation.is_some()
                    || actor.failure().is_some()
                    || !actor
                        .placement()
                        .is_some_and(|placement| placement.object_type == 1)
                {
                    continue;
                }
                let result = (|| -> anyhow::Result<()> {
                    if data.actor(self.actors[index].name())?["model_load_path"].as_str()
                        == Some("animation_table")
                    {
                        anyhow::ensure!(
                            self.updates.animation_system,
                            "animation-disabled model initialization unsupported"
                        );
                        self.initialize_map_animation(self.id(index)?, data, ticks, sounds)?;
                    }
                    Ok(())
                })();
                if let Err(error) = result {
                    self.actors[index].update_failure = Some(format!(
                        "world {} instance {index} actor {} automatic model preparation: {error:#}",
                        self.world,
                        self.actors[index].name()
                    ));
                }
            }
        }
        Ok(())
    }

    /// Execute the supported ordinary map stages in authored traversal order.
    /// The caller runs `refresh_automatic_models` first for this frame's family/group stage
    /// and retained presentation. Simulation does not refresh those captured transforms.
    /// Unsupported movement/combat and native services stop the affected actor,
    /// retaining its diagnostic instead of treating the missing stage as successful.
    pub fn update_ordinary(
        &mut self,
        data: &Animations,
        view: [i32; 2],
        inputs: OrdinaryInputs<'_>,
        random: &mut crate::random::EngineRandom,
        sounds: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> anyhow::Result<()>,
        budget: usize,
    ) -> anyhow::Result<()> {
        let OrdinaryInputs {
            scripts: inputs,
            scene,
        } = inputs;
        let inputs = ScriptInputs {
            animations: Some(super::AnimationData(data)),
            ..inputs
        };
        data.require_version()?;
        anyhow::ensure!(
            data.world == self.world
                && data.metadata["script_content_id"].as_str() == Some(&self.content_id),
            "world {} ordinary update asset identity mismatch",
            self.world
        );
        let ticks = inputs
            .nominal_ticks
            .ok_or_else(|| anyhow::anyhow!("ordinary update requires nominal_ticks"))?;
        anyhow::ensure!(
            inputs.frame_scale.is_some(),
            "ordinary update requires frame_scale"
        );
        self.globals.world_loop_frame = self.globals.world_loop_frame.wrapping_add(1);
        self.prepare_ordinary_models(data, ticks, sounds)?;
        self.update_shells(inputs, random, sounds, budget)?;
        self.update_created(inputs, random, sounds, budget)?;
        let selected = self.ordinary_actors(view)?.to_vec();
        for &id in &selected {
            if matches!(
                self.actors[id.index].source,
                super::ScriptActorSource::Camera
            ) {
                // The camera actor has no script body: its `cameraController` native hook runs the
                // camera update at its place in the list, ahead of the selected player's update.
                if let Some(service) = inputs.camera_update
                    && let Err(error) = self.run_camera_controller(service)
                {
                    self.actors[id.index].update_failure = Some(format!(
                        "world {} instance {} actor {} ordinary update: {error:#}",
                        self.world,
                        id.index,
                        self.actors[id.index].name()
                    ));
                }
                continue;
            }
            if self.selected_player_index() == Some(id.index) {
                self.globals.clear_player_contacts();
            }
            let selected_player = self.selected_player_index() == Some(id.index)
                && inputs.player_controller.is_some();
            let actor = &mut self.actors[id.index];
            actor.sync_falling_motion();
            if actor.placement().is_none() && !selected_player {
                continue;
            }
            if actor.failure().is_some() || actor.unavailable().is_some() {
                continue;
            }
            let Some(control) = actor.control.as_mut() else {
                continue;
            };
            if actor.attributes[26] != 0
                && if actor.attributes[25] == 0 {
                    self.updates.disable_slaves
                } else {
                    self.updates.disable_master_slaves
                }
            {
                control.object_update_excluded = true;
            }
            if control.object_update_excluded || control.script_control & 8 != 0 {
                continue;
            }
            self.globals.locomotion_called = 0;
            self.globals.ground_prevalidation_used = false;
            let result = (|| -> anyhow::Result<()> {
                self.advance_torch_burn(id, inputs, ticks, sounds)?;
                if selected_player {
                    if !self.actors[id.index].model.prepared {
                        self.prepare_player_model_resources(id, data)?;
                    }
                    self.begin_player_movement_update(id, ticks)?;
                }
                self.snapshot_spatial(id)?;
                self.actors[id.index].model.support = Default::default();
                self.actors[id.index]
                    .control
                    .as_mut()
                    .expect("admitted ordinary control")
                    .advance_action_transition_counter(ticks);
                if self.actors[id.index].animation.is_some() {
                    if !self.select_selected_death_animation_default(id, ticks, sounds)? {
                        let items_ready = inputs
                            .hud
                            .map(super::super::ScriptHud::inventory_ready)
                            .transpose()?
                            .unwrap_or(false);
                        self.select_map_animation_default(id, items_ready, ticks, sounds)?;
                    }
                    if self.updates.animation_system {
                        self.advance(
                            id,
                            ScriptStage::Animation,
                            inputs,
                            Some(sounds),
                            Some(random),
                            budget,
                        )?;
                    }
                }
                let rollback = self.capture_ordinary_rollback(id, sounds)?;
                let control = self.actors[id.index]
                    .control
                    .as_ref()
                    .expect("admitted ordinary control");
                anyhow::ensure!(
                    control.queued_motion == 0
                        && (control.falling_state == 0
                            || (!selected_player && self.actors[id.index].attributes[1] & 4 != 0)),
                    "ordinary queued motion stage unsupported"
                );
                let collision = if self.actors[id.index].attributes[1] != 0 {
                    let scene = scene
                        .ok_or_else(|| anyhow::anyhow!("ordinary collision scene unavailable"))?;
                    Some(self.prepare_ordinary_collision_query(id, scene, data, view, &selected)?)
                } else {
                    None
                };
                let inputs = ScriptInputs {
                    locomotion_geometry: collision
                        .as_ref()
                        .zip(scene)
                        .map(|(prepared, scene)| -> anyhow::Result<_> {
                            Ok(super::LocomotionGeometry {
                                ground: super::ground::OrdinaryGroundQuery {
                                    scene,
                                    animations: data,
                                    prepared,
                                    distance: scene.settings.post_collide_height,
                                    sidedness: false,
                                },
                                environment: super::transition::ObstacleEnvironment {
                                    selected_player: self
                                        .actors
                                        .iter()
                                        .position(|actor| {
                                            matches!(actor.source, super::ScriptActorSource::Player)
                                        })
                                        .map(|index| self.id(index))
                                        .transpose()?,
                                    auto_grab: false,
                                    item_system_ready: inputs
                                        .hud
                                        .map(super::super::ScriptHud::inventory_ready)
                                        .transpose()?
                                        .unwrap_or(false),
                                    shell_execution: inputs.shell_execution.unwrap_or(false),
                                    ticks,
                                },
                            })
                        })
                        .transpose()?,
                    player_collision: scene.map(|scene| super::PlayerCollisionSource {
                        scene,
                        animations: data,
                        view,
                        actors: &selected,
                    }),
                    ..inputs
                };
                let player_query = if selected_player {
                    Some(super::ground::OrdinaryGroundQuery {
                        scene: scene
                            .ok_or_else(|| anyhow::anyhow!("player collision scene unavailable"))?,
                        animations: data,
                        prepared: collision.as_ref().ok_or_else(|| {
                            anyhow::anyhow!("player collision candidates unavailable")
                        })?,
                        distance: scene.unwrap().settings.post_collide_height,
                        sidedness: false,
                    })
                } else {
                    None
                };
                let player_timing = crate::clock::ClockStep {
                    frame_scale: inputs.frame_scale.expect("validated frame scale"),
                    nominal_ticks: ticks,
                    fine_ticks: 0,
                };
                // The input update runs every frame, including while a script task owns the player.
                if let (Some(query), Some(controller)) = (&player_query, inputs.player_controller) {
                    let settings = &query.scene.settings;
                    self.globals.turn.update(
                        [controller.held.left, controller.held.right],
                        settings.turn_speed,
                        settings.turn_max_speed,
                    );
                }
                if let Some(query) = player_query {
                    self.advance_player_controller(
                        id,
                        query,
                        player_timing,
                        inputs,
                        random,
                        sounds,
                    )?;
                } else {
                    self.advance(
                        id,
                        ScriptStage::OrdinaryHook,
                        inputs,
                        Some(sounds),
                        Some(random),
                        budget,
                    )?;
                }
                let actor = &self.actors[id.index];
                if self.updates.task_system
                    && !actor
                        .runtime
                        .as_ref()
                        .is_some_and(cyrus_script::Runtime::has_unpaused_task)
                {
                    let shell = inputs
                        .shell_execution
                        .or_else(|| inputs.activation.map(|input| input.shell_execution))
                        .unwrap_or(false);
                    let transition = if let Some(input) = inputs.activation {
                        input.item_system_ready
                            && !shell
                            && input.actors.get(&input.player.index).is_some_and(|player| {
                                let animation = self
                                    .actors
                                    .get(input.player.index)
                                    .and_then(super::ScriptActor::bound_animation_group)
                                    .map(|group| super::super::ActivationAnimation { group })
                                    .or(player.animation);
                                let (_, drawing, sheathing) =
                                    super::super::activation::weapon_predicates(
                                        player.state,
                                        animation,
                                        player.weapon_assigned,
                                        player.weapon_drawn,
                                        0,
                                        input.item_system_ready,
                                        shell,
                                    );
                                drawing || sheathing
                            })
                    } else if !shell
                        && inputs
                            .hud
                            .map(super::super::ScriptHud::inventory_ready)
                            .transpose()?
                            .unwrap_or(false)
                    {
                        self.selected_player_index()
                            .map(|index| &self.actors[index])
                            .and_then(|actor| {
                                actor.control.as_ref().map(|control| (actor, control))
                            })
                            .is_some_and(|(actor, control)| {
                                let (_, drawing, sheathing) =
                                    super::super::activation::weapon_predicates(
                                        control.living_state,
                                        actor.bound_animation_group().map(|group| {
                                            super::super::ActivationAnimation { group }
                                        }),
                                        control.combat_death_default.is_some(),
                                        control.weapon_drawn,
                                        actor.attributes[110],
                                        true,
                                        shell,
                                    );
                                drawing || sheathing
                            })
                    } else {
                        false
                    };
                    self.maintain_map_combat(id, ticks, transition, inputs, Some(random), sounds)?;
                }
                self.advance(
                    id,
                    ScriptStage::OrdinaryMain,
                    inputs,
                    Some(sounds),
                    Some(random),
                    budget,
                )?;
                if let Some(query) = player_query {
                    self.advance_player_walking(id, query, player_timing, true, sounds)?;
                }
                if !self.actors[id.index]
                    .control
                    .as_ref()
                    .is_some_and(|control| control.hidden)
                {
                    self.actors[id.index].maintain_light(ticks);
                }
                if self.actors[id.index].attributes[1] != 0 {
                    let collision = collision.as_ref().ok_or_else(|| {
                        anyhow::anyhow!(
                            "script enabled collision without pre-script candidate preparation"
                        )
                    })?;
                    let scene = anyhow::Context::context(
                        scene,
                        "ordinary late collision scene unavailable",
                    )?;
                    anyhow::Context::context(
                        self.resolve_ordinary_late_collision(
                            id,
                            super::ground::OrdinaryGroundQuery {
                                scene,
                                animations: data,
                                prepared: collision,
                                distance: scene.settings.post_collide_height,
                                sidedness: false,
                            },
                            super::late::OrdinaryLateInputs {
                                ticks,
                                fine_ticks: inputs.fine_ticks,
                                step_fps: inputs.step_fps,
                                weapon: inputs
                                    .hud
                                    .zip(inputs.shell_execution)
                                    .zip(self.actors.iter().position(|actor| {
                                        matches!(actor.source, super::ScriptActorSource::Player)
                                    }))
                                    .map(|((hud, shell_execution), index)| {
                                        Ok::<_, anyhow::Error>(super::weapon::WeaponInputs {
                                            selected_player: self.id(index)?,
                                            item_system_ready: hud.inventory_ready()?,
                                            shell_execution,
                                        })
                                    })
                                    .transpose()?,
                            },
                            &rollback,
                            super::recovery::OrdinaryStableRecoveryInputs {
                                scene,
                                animations: data,
                                view,
                                actors: &selected,
                                random,
                            },
                            sounds,
                        ),
                        "ordinary late collision response",
                    )?;
                }
                super::super::locomotion::finish_step_counters(
                    &mut self.actors[id.index]
                        .control
                        .as_mut()
                        .expect("admitted control")
                        .movement_flags,
                    self.globals.locomotion_called,
                );
                self.follow_ordinary_support(id, None)?;
                self.restore_axis_locks(id)?;
                if self.spatial(id)?.parent.is_none() {
                    self.refresh_transform(id)?;
                }
                self.retain_ordinary_water_position(id)?;
                if selected_player {
                    self.finish_player_movement_update(id)?;
                }
                Ok(())
            })();
            if let Err(error) = result {
                self.actors[id.index].update_failure = Some(format!(
                    "world {} instance {} actor {} ordinary update: {error:#}",
                    self.world,
                    id.index,
                    self.actors[id.index].name()
                ));
            }
        }
        for &id in &selected {
            if self.actors[id.index].failure().is_none() {
                self.run_held_torch(id, inputs, ticks, sounds);
            }
        }
        // Separate records gather no collision candidates of their own, but their natives (for
        // example `PlayerInSight`) read the world's geometry and the latest gather.
        let separate_inputs = ScriptInputs {
            player_collision: scene.map(|scene| super::PlayerCollisionSource {
                scene,
                animations: data,
                view,
                actors: &selected,
            }),
            ..inputs
        };
        self.update_separate(view, separate_inputs, random, sounds, budget)?;
        // The item-use latch clears after the actors ran and before the key handler, so a key
        // press arms it for the next iteration's scripts.
        self.globals.end_item_use_attempt();
        self.run_quick_keys(inputs, random, sounds)?;
        self.advance_environment_effects(ticks);
        Ok(())
    }
}

pub(crate) fn sample_clock(
    time: Res<Time<Real>>,
    mut actors: ResMut<ScriptActors>,
    clock: Option<ResMut<SimulationClock>>,
    prepared: OrdinaryAssets,
    mut running: Local<bool>,
    mut commands: Commands,
) {
    let ready = !prepared
        .logbook
        .as_ref()
        .is_some_and(|screen| screen.suspended())
        && prepared
            .state
            .as_ref()
            .is_some_and(|state| *state.get() == crate::AppState::InGame)
        && !prepared
            .console
            .as_ref()
            .is_some_and(|console| console.open)
        && actors.current.is_some()
        && actors.error.is_none()
        && actors.world_loop_outcome.is_none()
        && prepared
            .view
            .as_ref()
            .zip(prepared.request.as_ref())
            .is_some_and(|(view, request)| view.source == request.0);
    commands.remove_resource::<WorldClockSample>();
    if let Some(mut clock) = clock {
        let tick = bios_tick(time.elapsed());
        if ready {
            if !*running {
                clock.rebase(tick);
            }
            clock.advance(tick);
            actors
                .current
                .as_mut()
                .expect("clock readiness requires a world")
                .globals
                .advance_player_idle_clock(clock.measurement_intervals());
            commands.insert_resource(WorldClockSample {
                generation: actors.current.as_ref().unwrap().generation(),
                step: clock.step(),
                models_prepared: false,
            });
        }
    }
    *running = ready;
}

#[derive(SystemParam)]
pub(crate) struct OrdinaryAssets<'w> {
    view: Option<Res<'w, ScriptView>>,
    prepared_world: Option<Res<'w, cyrus_world::lifecycle::PreparedWorld>>,
    server: Option<Res<'w, AssetServer>>,
    environments: Option<Res<'w, Assets<cyrus_assets::environment::Environment>>>,
    request: Option<Res<'w, AnimationRequest>>,
    assets: Res<'w, Assets<Animations>>,
    scripts: Option<Res<'w, Assets<cyrus_assets::script::Scripts>>>,
    scene_request: Option<Res<'w, crate::player::data::PlayerSceneRequest>>,
    scenes: Option<Res<'w, Assets<crate::player::data::PlayerScene>>>,
    sample: Option<ResMut<'w, WorldClockSample>>,
    cheats: Option<Res<'w, crate::cheats::Cheats>>,
    console: Option<Res<'w, cyrus_console::Console>>,
    logbook: Option<Res<'w, crate::logbook::LogbookScreen>>,
    state: Option<Res<'w, State<crate::AppState>>>,
}

/// Optional host execution adapter. The game retains ordinary stage ownership;
/// the host lends synchronous services for the duration of this invocation.
#[derive(Resource)]
pub struct OrdinarySoundExecutor(pub fn(&mut World, OrdinaryPhase));

/// Scheduler inputs borrowed for one ordinary invocation. Presentation hosts may
/// lend an audio driver without retaining game state or introducing another VM.
#[derive(SystemParam)]
pub struct OrdinaryContext<'w, 's> {
    prepared: OrdinaryAssets<'w>,
    hud: Option<Res<'w, super::super::ScriptHud>>,
    entry: Option<Res<'w, crate::player::placement::MarkerEntry>>,
    input: Option<Res<'w, crate::player::input::PlayerInput>>,
    quick: Option<Res<'w, super::super::QuickKeys>>,
    toggles: Option<Res<'w, crate::player::input::PlayerToggles>>,
    fidelity: Option<Res<'w, crate::fidelity::Fidelity>>,
    explicit: Option<Res<'w, crate::player::runtime::PlayerRuntime>>,
    actors: ResMut<'w, ScriptActors>,
    clock: Option<Res<'w, SimulationClock>>,
    random: ResMut<'w, crate::random::EngineRandom>,
    camera_drive: Option<Res<'w, super::CameraDrive>>,
    camera_frame: Option<ResMut<'w, super::CameraFrameState>>,
    bindings: Query<'w, 's, (Entity, &'static super::super::ScriptBinding)>,
    players:
        Query<'w, 's, &'static crate::player::placement::PlayerPose, With<crate::player::Player>>,
}

impl OrdinaryContext<'_, '_> {
    /// Whether this entry requires retained-world restoration before traversal.
    pub fn has_pending_revisit(&self) -> bool {
        self.actors.has_pending_revisit()
    }

    /// Initialize the entry roles before the host clears services and restores
    /// retained records. Pending asset/clock preparation runs no entry effects.
    pub fn prepare_revisit_entry(
        &mut self,
        emit: &mut dyn FnMut(SoundCue) -> anyhow::Result<()>,
    ) -> anyhow::Result<bool> {
        if !self.has_pending_revisit() || !self.is_running() {
            return Ok(false);
        }
        let (Some(request), Some(clock), Some(sample)) = (
            self.prepared.request.as_ref(),
            self.clock.as_ref(),
            self.prepared.sample.as_ref(),
        ) else {
            return Ok(false);
        };
        let Some(data) = self.prepared.assets.get(&request.0) else {
            return Ok(false);
        };
        let world = self.actors.current.as_mut().expect("running world");
        if sample.generation != world.generation()
            || !world.model_resources_prepared()
            || world.model_admission.is_none()
            || (world.selected_player_index().is_some() && self.players.single().is_err())
        {
            return Ok(false);
        }
        initialize_entry(
            world,
            data,
            clock.step().nominal_ticks,
            self.players.single().ok(),
            self.entry.as_deref().copied(),
            &mut |actor, sound| send_actor_sound(actor, sound, &request.0, &self.bindings, emit),
        )?;
        Ok(true)
    }

    /// Reconstruct retained actors with fresh identities and clear shared services
    /// before adopting them. The host must first complete `prepare_revisit_entry`.
    pub fn restore_visited(
        &mut self,
        cleanup: &mut dyn FnMut(
            super::super::SoundResetStage,
            &mut super::super::GameplayGlobals,
        ) -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        let request = self
            .prepared
            .request
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("revisit animation request absent"))?;
        let data = self
            .prepared
            .assets
            .get(&request.0)
            .ok_or_else(|| anyhow::anyhow!("revisit animations unavailable"))?;
        let scripts = self
            .actors
            .requested
            .as_ref()
            .zip(self.prepared.scripts.as_ref())
            .and_then(|(handle, scripts)| scripts.get(handle))
            .ok_or_else(|| anyhow::anyhow!("revisit scripts unavailable"))?;
        let scene = self
            .prepared
            .scene_request
            .as_ref()
            .zip(self.prepared.scenes.as_ref())
            .and_then(|(request, scenes)| scenes.get(&request.0))
            .ok_or_else(|| anyhow::anyhow!("revisit player/collision source unavailable"))?;
        self.actors.restore_visited(
            &super::WorldSnapshotSources {
                effects: self
                    .prepared
                    .prepared_world
                    .as_ref()
                    .zip(self.prepared.server.as_ref())
                    .and_then(|(prepared, server)| {
                        server.get_handle::<cyrus_assets::environment::Environment>(
                            prepared.environment_path.clone(),
                        )
                    })
                    .and_then(|handle| {
                        self.prepared
                            .environments
                            .as_ref()
                            .and_then(|assets| assets.get(&handle))
                    })
                    .and_then(|environment| environment.effects.as_ref()),
                scripts,
                animations: data,
                collision: &scene.collision,
                player: Some(scene),
            },
            cleanup,
        )?;
        Ok(())
    }

    /// Whether automatic world execution may request host services this update.
    pub fn is_running(&self) -> bool {
        !self
            .prepared
            .logbook
            .as_ref()
            .is_some_and(|screen| screen.suspended())
            && self
                .prepared
                .state
                .as_ref()
                .is_some_and(|state| *state.get() == crate::AppState::InGame)
            && !self
                .prepared
                .console
                .as_ref()
                .is_some_and(|console| console.open)
            && self.actors.error.is_none()
            && self.actors.current.is_some()
    }

    /// Lend the admitted world and sampled nominal ticks to its service stages.
    /// The same asset/model/clock gates as ordinary traversal apply. No host work
    /// runs while the console is open or world preparation remains incomplete.
    pub fn with_world_services<R>(
        &mut self,
        update: impl FnOnce(&mut ScriptWorld, u32) -> R,
    ) -> Option<R> {
        if !self.is_running() {
            return None;
        }
        let view = self.prepared.view.as_ref()?;
        let request = self.prepared.request.as_ref()?;
        if view.source != request.0 || self.prepared.assets.get(&request.0).is_none() {
            return None;
        }
        let world = self.actors.current.as_mut()?;
        if self
            .prepared
            .sample
            .as_ref()
            .is_none_or(|sample| sample.generation != world.generation())
            || !world.model_resources_prepared()
            || world.model_admission.is_none()
        {
            return None;
        }
        Some(update(world, self.clock.as_ref()?.step().nominal_ticks))
    }

    /// A simulation invocation cannot bypass this update's completed model phase.
    pub fn models_prepared(&self) -> bool {
        self.generation().is_some_and(|generation| {
            self.prepared
                .sample
                .as_ref()
                .and_then(|sample| sample.presentation_step(generation))
                .is_some()
        })
    }

    /// Retain a contextual host failure and stop automatic world updates.
    pub fn report_error(&mut self, error: String) {
        self.actors.report_runtime_error(error);
    }

    /// Nonzero authored taunt labels needed before synchronous ordinary calls run.
    /// Preparation must not consume random draws or mutate actor/cache ownership.
    pub fn combat_taunt_labels(&self) -> Vec<[u8; 4]> {
        self.actors
            .current
            .as_ref()
            .into_iter()
            .flat_map(|world| world.actors.iter())
            .filter(|actor| actor.control.is_some())
            .flat_map(|actor| &actor.combat_taunts)
            .filter(|taunt| taunt.label != [0; 4])
            .map(|taunt| taunt.label)
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    /// Current logical world identity, used to reject expired audio ownership.
    pub fn generation(&self) -> Option<u64> {
        self.actors.current.as_ref().map(ScriptWorld::generation)
    }
}

pub(crate) fn prepare_models(world: &mut World) {
    execute_phase(world, OrdinaryPhase::ModelPresentation);
}

pub(crate) fn step(world: &mut World) {
    execute_phase(world, OrdinaryPhase::Simulation);
}

fn execute_phase(world: &mut World, phase: OrdinaryPhase) {
    if let Some(executor) = world.get_resource::<OrdinarySoundExecutor>() {
        let execute = executor.0;
        execute(world, phase);
    } else if let Err(error) = world.run_system_cached_with(queued_step, phase) {
        world.resource_mut::<ScriptActors>().error = Some(format!("ordinary scheduler: {error}"));
    }
}

fn queued_step(
    In(phase): In<OrdinaryPhase>,
    context: OrdinaryContext,
    mut sounds: Option<ResMut<Messages<SoundCue>>>,
) {
    run_ordinary(context, phase, None, None, None, None, &mut |cue| {
        let sounds = sounds
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("ordinary sound consumer unavailable"))?;
        sounds.write(cue);
        Ok(())
    });
}

/// Decide a forward animation `Transition` command. **Inferred** from the original's decision
/// routine: masks 1 and 2 take the transition only when the actor's pending action word has a
/// jump bit, which only the `SetAction` native sets. Cyrus does not implement `SetAction` (a
/// script calling it fails), so that word is always clear and these masks continue with the next
/// command. Mask 8 acts only for the selected player, so every other actor continues; any other
/// mask continues. Mask 8 for the selected player reads a combat input flag cyrus does not model,
/// so it fails instead of guessing. Mask 4 never reaches the host: the animation decides it from
/// the falling byte.
fn transition_continues(
    actor: ScriptActorId,
    mask: u32,
    selected_player: Option<ScriptActorId>,
) -> anyhow::Result<()> {
    match mask {
        8 if Some(actor) == selected_player => {
            anyhow::bail!("selected-player combat animation transition is unsupported")
        }
        _ => Ok(()),
    }
}

fn send_actor_sound(
    actor: ScriptActorId,
    sound: super::super::ScriptSound,
    source: &Handle<Animations>,
    bindings: &Query<(Entity, &super::super::ScriptBinding)>,
    emit: &mut dyn FnMut(SoundCue) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    let cue = match sound {
        super::super::ScriptSound::Animation {
            sound:
                cyrus_world::animation::AnimationSound::Event { .. }
                | cyrus_world::animation::AnimationSound::Transition { .. },
            ..
        } => anyhow::bail!(
            "animation actor event or transition outside an ordinary update is unsupported"
        ),
        super::super::ScriptSound::Animation {
            sound,
            surface,
            position,
        } => {
            let mut nodes = bindings
                .iter()
                .filter(|(_, binding)| binding.actor == actor && binding.source == *source);
            let (entity, _) = nodes
                .next()
                .ok_or_else(|| anyhow::anyhow!("animation sound scene binding unavailable"))?;
            anyhow::ensure!(
                nodes.next().is_none(),
                "animation sound scene binding ambiguous"
            );
            SoundCue::Animation {
                actor: entity,
                sound,
                surface,
                position,
            }
        }
        sound => SoundCue::Script {
            generation: actor.generation,
            index: actor.index,
            sound,
        },
    };
    emit(cue)
}

fn initialize_entry(
    world: &mut ScriptWorld,
    data: &Animations,
    ticks: u32,
    player_pose: Option<&crate::player::placement::PlayerPose>,
    entry: Option<crate::player::placement::MarkerEntry>,
    send_sound: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    world.prepare_ordinary_models(data, ticks, send_sound)?;
    if let Some(pose) = player_pose
        && world.selected_player_index().is_some()
    {
        for index in 0..world.actors.len() {
            if !matches!(
                world.actors[index].source,
                super::ScriptActorSource::Player | super::ScriptActorSource::AlternatePlayer
            ) {
                continue;
            }
            let id = world.id(index)?;
            if world.selected_player_index() == Some(index)
                && !world
                    .spatial
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("player spatial registry not prepared"))?
                    .is_initialized(index)?
            {
                let mut position = [0; 3];
                for (axis, value) in pose.position.into_iter().enumerate() {
                    position[axis] = i32::try_from(value).map_err(|_| {
                        anyhow::anyhow!(
                            "player spatial initialization axis {axis}: Q8 position outside i32"
                        )
                    })?;
                }
                world.initialize_player_spatial(id, position, [0, pose.heading, 0])?;
            }
            if !world.actors[index].animation_initialized() {
                let entry = entry.ok_or_else(|| {
                    anyhow::anyhow!("player role initialization: marker entry unavailable")
                })?;
                let (spawn, _) = crate::player::placement::placement(data, entry)?;
                let mut position = [0; 3];
                for (axis, value) in spawn.position.into_iter().enumerate() {
                    position[axis] = i32::try_from(value).map_err(|_| {
                        anyhow::anyhow!(
                            "player role initialization axis {axis}: Q8 position outside i32"
                        )
                    })?;
                }
                let surface = world.actors[index].surface;
                world.initialize_player_animation(id, data, ticks, &mut |sound| {
                    let position =
                        matches!(sound, cyrus_world::animation::AnimationSound::Play { .. })
                            .then_some(position);
                    send_sound(
                        id,
                        super::super::ScriptSound::Animation {
                            sound,
                            surface,
                            position,
                        },
                    )
                })?;
            }
            if !world.actors[index].model.prepared
                && world.actors[index]
                    .control
                    .as_ref()
                    .is_some_and(|control| !control.hidden && !control.object_update_excluded)
            {
                world.prepare_player_model_resources(id, data)?;
            }
        }
    }
    Ok(())
}

/// Run the game-owned ordinary stages with synchronous audio feedback. Missing
/// reset state remains unsupported; a host must not substitute a guessed flag.
pub fn run_ordinary(
    context: OrdinaryContext,
    phase: OrdinaryPhase,
    ambient_sound: Option<&dyn cyrus_world::sound::AmbientSoundDriver>,
    sound_reset: Option<bool>,
    dialogue_source: Option<&dyn super::super::DialogueSource>,
    combat_taunts: Option<&dyn super::super::CombatTauntSource>,
    emit: &mut dyn FnMut(SoundCue) -> anyhow::Result<()>,
) {
    let OrdinaryContext {
        prepared,
        hud,
        entry,
        input,
        quick,
        toggles,
        fidelity,
        explicit,
        mut actors,
        clock,
        mut random,
        camera_drive,
        camera_frame,
        bindings,
        players,
    } = context;
    if hud.as_ref().is_some_and(|hud| !hud.ready()) {
        return;
    }
    let OrdinaryAssets {
        view,
        request,
        assets,
        scripts: _,
        scene_request,
        scenes,
        mut sample,
        cheats,
        console,
        logbook,
        state,
        ..
    } = prepared;
    if !state.is_some_and(|state| *state.get() == crate::AppState::InGame)
        || logbook.is_some_and(|screen| screen.suspended())
        || console.is_some_and(|console| console.open)
        || actors.error.is_some()
        || actors.world_loop_outcome.is_some()
    {
        return;
    }
    if actors.has_pending_revisit() {
        actors.error = Some("world revisit requires complete session service restoration".into());
        return;
    }
    let (Some(view), Some(request), Some(clock)) = (view, request, clock) else {
        return;
    };
    if view.source != request.0 {
        return;
    }
    let Some(data) = assets.get(&request.0) else {
        return;
    };
    let Some(world) = actors.current.as_mut() else {
        return;
    };
    if sample
        .as_ref()
        .is_none_or(|sample| sample.generation != world.generation())
    {
        return;
    }
    if !world.model_resources_prepared() || world.model_admission.is_none() {
        return;
    }
    let timing = clock.step();
    let selected_player = world
        .selected_player_index()
        .and_then(|index| world.id(index).ok());
    let mut send_sound = |actor: ScriptActorId, sound: super::super::ScriptSound| {
        if let super::super::ScriptSound::Animation {
            sound: cyrus_world::animation::AnimationSound::Transition { mask },
            ..
        } = sound
        {
            return transition_continues(actor, mask, selected_player);
        }
        send_actor_sound(actor, sound, &request.0, &bindings, emit)
    };
    if phase == OrdinaryPhase::ModelPresentation {
        // Ordinary world scheduling runs outside the separate shell-script context.
        if let Err(error) = world.flush_map_updates(view.position, false) {
            actors.error = Some(format!("automatic deferred movement: {error:#}"));
            return;
        }
        let preparation = (|| -> anyhow::Result<()> {
            initialize_entry(
                world,
                data,
                timing.nominal_ticks,
                players.single().ok(),
                entry.as_deref().copied(),
                &mut send_sound,
            )?;
            if players.single().is_ok()
                && let Some(hud) = hud.as_ref()
                && let Some(index) = world.selected_player_index()
            {
                world.check_player_weapon(
                    world.id(index)?,
                    hud,
                    false,
                    timing.nominal_ticks,
                    &mut send_sound,
                )?;
            }
            world.refresh_automatic_models(
                data,
                view.position,
                scene_request
                    .as_ref()
                    .and_then(|request| scenes.as_ref().and_then(|scenes| scenes.get(&request.0))),
                ScriptInputs {
                    hud: hud.as_deref(),
                    shell_execution: Some(false),
                    death_suppressed: cheats
                        .as_ref()
                        .and_then(|cheats| cheats.death_suppressed().ok()),
                    nominal_ticks: Some(timing.nominal_ticks),
                    frame_scale: Some(timing.frame_scale),
                    dialogue_source,
                    combat_taunts,
                    sound_reset,
                    ..Default::default()
                },
                Some(&mut random),
                &mut send_sound,
            )
        })();
        if let Err(error) = preparation {
            actors.error = Some(format!("automatic model presentation: {error:#}"));
            return;
        }
        sample
            .as_deref_mut()
            .expect("admitted clock sample")
            .models_prepared = true;
        return;
    }
    if sample.as_ref().is_none_or(|sample| !sample.models_prepared) {
        return;
    }
    let player_query = (|| -> anyhow::Result<_> {
        let Some(index) = world.selected_player_index() else {
            return Ok(None);
        };
        if players.single().is_err() {
            return Ok(None);
        }
        let pose = world.spatial(world.id(index)?)?.query_pose()?;
        Ok(Some(super::super::QueryInputs {
            player: Some(super::super::PlayerQuery {
                position: pose.position,
                heading: pose.angles[1],
            }),
            distance_override: Some(
                cheats
                    .as_ref()
                    .and_then(|cheats| cheats.distance_override().ok())
                    .unwrap_or(world.player_distance_override),
            ),
            fallback_position: None,
            linked_position: None,
        }))
    })();
    let player_query = match player_query {
        Ok(query) => query,
        Err(error) => {
            actors.error = Some(format!("automatic selected-player query: {error:#}"));
            return;
        }
    };
    let player_controller = if !explicit
        .as_ref()
        .is_some_and(|runtime| runtime.explicit_updated)
        && fidelity
            .as_ref()
            .is_some_and(|fidelity| fidelity.player_controller == crate::fidelity::Mode::Faithful)
    {
        input
            .as_deref()
            .zip(toggles.as_deref())
            .map(|(held, toggles)| super::player::PlayerControllerInput {
                held,
                walk_mode: toggles.walk_mode,
            })
    } else {
        None
    };
    let drive_scene = camera_drive
        .as_deref()
        .filter(|drive| drive.enabled)
        .and(scene_request.as_ref())
        .and_then(|request| scenes.as_ref().and_then(|scenes| scenes.get(&request.0)));
    let drive_view = [view.position[0] >> 8, view.position[2] >> 8];
    let mut camera_frame = camera_frame;
    let terrain_arc = camera_frame
        .as_deref()
        .and_then(|frame| frame.terrain.clone());
    let driven = match (drive_scene, camera_frame.as_deref_mut()) {
        (Some(scene), Some(frame)) => {
            match super::camera_drive::begin(
                world,
                scene,
                frame,
                terrain_arc.as_deref(),
                drive_view,
                timing.frame_scale as i32,
            ) {
                Ok(update) => update,
                Err(error) => {
                    actors.error = Some(format!("camera setup: {error:#}"));
                    return;
                }
            }
        }
        _ => None,
    };
    let result = world.update_ordinary(
        data,
        [view.position[0] >> 8, view.position[2] >> 8],
        OrdinaryInputs {
            scene: scene_request
                .as_ref()
                .and_then(|request| scenes.as_ref().and_then(|scenes| scenes.get(&request.0))),
            scripts: ScriptInputs {
                hud: hud.as_deref(),
                activation_use_held: input.as_deref().map(|held| held.action),
                quick_keys: quick.as_deref().copied(),
                shell_execution: Some(false),
                combat_clock_sample: Some(clock.bios_sample()),
                nominal_ticks: Some(timing.nominal_ticks),
                frame_scale: Some(timing.frame_scale),
                fine_ticks: Some(timing.fine_ticks),
                step_fps: Some(clock.fps()),
                player_query,
                player_controller,
                ambient_sound,
                dialogue_source,
                combat_taunts,
                sound_reset,
                camera_update: driven
                    .as_ref()
                    .map(|update| update as &dyn super::super::camera::CameraUpdateService),
                ..default()
            },
        },
        &mut random,
        &mut send_sound,
        100_000,
    );
    if let Err(error) = result {
        actors.error = Some(format!("automatic ordinary update: {error:#}"));
        return;
    }
    if let (Some(update), Some(frame), Some(scene), Some(world)) =
        (driven, camera_frame, drive_scene, actors.current.as_mut())
        && let Err(error) = super::camera_drive::end(
            world,
            scene,
            frame.into_inner(),
            update,
            terrain_arc.as_deref(),
            drive_view,
            timing.frame_scale as i32,
        )
    {
        actors.error = Some(format!("camera frame: {error:#}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transitions_continue_except_the_selected_players_combat_input() {
        let other = ScriptActorId {
            generation: 1,
            index: 3,
        };
        let player = ScriptActorId {
            generation: 1,
            index: 5,
        };
        for mask in [0, 1, 2, 3, 4, 5, 16, 63] {
            assert!(transition_continues(other, mask, Some(player)).is_ok());
            assert!(transition_continues(player, mask, Some(player)).is_ok());
        }
        assert!(transition_continues(other, 8, Some(player)).is_ok());
        assert!(transition_continues(other, 8, None).is_ok());
        assert!(transition_continues(player, 8, Some(player)).is_err());
    }
}
