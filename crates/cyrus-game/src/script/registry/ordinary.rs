//! Automatic ordinary map stages share the installed clock and logical actor state.
use super::{ScriptActorId, ScriptActors, ScriptInputs, ScriptStage, ScriptWorld};
use crate::clock::{SimulationClock, bios_tick};
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

#[derive(Resource)]
pub(crate) struct ClockSample(u64);

impl ScriptWorld {
    /// Execute the supported ordinary map stages in authored traversal order.
    /// Unsupported movement/combat and native services stop the affected actor,
    /// retaining its diagnostic instead of treating the missing stage as successful.
    pub fn update_ordinary(
        &mut self,
        data: &Animations,
        view: [i32; 2],
        inputs: ScriptInputs<'_>,
        random: &mut crate::random::EngineRandom,
        sounds: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> anyhow::Result<()>,
        budget: usize,
    ) -> anyhow::Result<()> {
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
        if self.spatial.is_none() {
            self.prepare_spatial()?;
            self.refresh_families()?;
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
        let selected = self.ordinary_actors(view)?.to_vec();
        for id in selected {
            let actor = &mut self.actors[id.index];
            // Standalone player/camera stages have separate, not yet connected owners.
            if actor.placement().is_none() {
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
            let result = (|| -> anyhow::Result<()> {
                self.snapshot_spatial(id)?;
                if self.actors[id.index].animation.is_some() {
                    // No item-system owner is installed yet.
                    self.select_map_animation_default(id, false, ticks, sounds)?;
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
                anyhow::ensure!(
                    self.actors[id.index].attributes[1] == 0,
                    "ordinary movement and collision stage unsupported"
                );
                self.advance(
                    id,
                    ScriptStage::OrdinaryHook,
                    inputs,
                    Some(sounds),
                    Some(random),
                    budget,
                )?;
                let actor = &self.actors[id.index];
                anyhow::ensure!(
                    !self.updates.task_system
                        || actor
                            .control
                            .as_ref()
                            .is_none_or(|control| control.combat_death_default.is_none())
                        || actor
                            .runtime
                            .as_ref()
                            .is_some_and(cyrus_script::Runtime::has_unpaused_task),
                    "ordinary combat stage unsupported"
                );
                self.advance(
                    id,
                    ScriptStage::OrdinaryMain,
                    inputs,
                    Some(sounds),
                    Some(random),
                    budget,
                )?;
                anyhow::ensure!(
                    self.actors[id.index].attributes[1] == 0,
                    "script enabled an unsupported ordinary collision stage"
                );
                self.restore_axis_locks(id)?;
                if self.spatial(id)?.parent.is_none() {
                    self.refresh_transform(id)?;
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
        self.refresh_families()
    }
}

pub(crate) fn sample_clock(
    time: Res<Time<Real>>,
    actors: Res<ScriptActors>,
    clock: Option<ResMut<SimulationClock>>,
    prepared: OrdinaryAssets,
    mut running: Local<bool>,
    mut commands: Commands,
) {
    let ready = prepared
        .state
        .as_ref()
        .is_some_and(|state| *state.get() == crate::AppState::InGame)
        && !prepared
            .console
            .as_ref()
            .is_some_and(|console| console.open)
        && actors.current.is_some()
        && actors.error.is_none()
        && prepared
            .view
            .as_ref()
            .zip(prepared.request.as_ref())
            .is_some_and(|(view, request)| view.source == request.0);
    commands.remove_resource::<ClockSample>();
    if let Some(mut clock) = clock {
        let tick = bios_tick(time.elapsed());
        if ready {
            if !*running {
                clock.rebase(tick);
            }
            clock.advance(tick);
            commands.insert_resource(ClockSample(actors.current.as_ref().unwrap().generation()));
        }
    }
    *running = ready;
}

#[derive(SystemParam)]
pub(crate) struct OrdinaryAssets<'w> {
    view: Option<Res<'w, ScriptView>>,
    request: Option<Res<'w, AnimationRequest>>,
    assets: Res<'w, Assets<Animations>>,
    sample: Option<Res<'w, ClockSample>>,
    cheats: Option<Res<'w, crate::cheats::Cheats>>,
    console: Option<Res<'w, crate::console::Console>>,
    state: Option<Res<'w, State<crate::AppState>>>,
}

pub(crate) fn step(
    prepared: OrdinaryAssets,
    mut actors: ResMut<ScriptActors>,
    clock: Option<Res<SimulationClock>>,
    mut random: ResMut<crate::random::EngineRandom>,
    mut sounds: Option<ResMut<Messages<SoundCue>>>,
    bindings: Query<(Entity, &super::super::ScriptBinding)>,
    players: Query<&crate::player::placement::PlayerPose, With<crate::player::Player>>,
) {
    let OrdinaryAssets {
        view,
        request,
        assets,
        sample,
        cheats,
        console,
        state,
    } = prepared;
    if !state.is_some_and(|state| *state.get() == crate::AppState::InGame)
        || console.is_some_and(|console| console.open)
        || actors.error.is_some()
    {
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
    if sample.is_none_or(|sample| sample.0 != world.generation()) {
        return;
    }
    let timing = clock.step();
    let player_query = match players.single() {
        Ok(player) => {
            let mut position = [0; 3];
            for (axis, coordinate) in player.position.into_iter().enumerate() {
                let Ok(coordinate) = i32::try_from(coordinate) else {
                    actors.error = Some(format!(
                        "world {} selected player position axis {axis}: outside Q8 i32 range",
                        world.world
                    ));
                    return;
                };
                position[axis] = coordinate;
            }
            Some(super::super::QueryInputs {
                player: Some(super::super::PlayerQuery {
                    position,
                    heading: player.heading,
                }),
                distance_override: Some(
                    cheats
                        .as_ref()
                        .and_then(|cheats| cheats.distance_override().ok())
                        .unwrap_or(world.player_distance_override),
                ),
                fallback_position: None,
                linked_position: None,
            })
        }
        Err(_) => None,
    };
    // Ordinary world scheduling runs outside the separate shell-script context.
    if let Err(error) = world.flush_map_updates(view.position, false) {
        actors.error = Some(format!("automatic deferred movement: {error:#}"));
        return;
    }
    let result = world.update_ordinary(
        data,
        [view.position[0] >> 8, view.position[2] >> 8],
        ScriptInputs {
            nominal_ticks: Some(timing.nominal_ticks),
            frame_scale: Some(timing.frame_scale),
            player_query,
            ..default()
        },
        &mut random,
        &mut |actor, sound| {
            let sounds = sounds
                .as_mut()
                .ok_or_else(|| anyhow::anyhow!("ordinary sound consumer unavailable"))?;
            match sound {
                super::super::ScriptSound::Animation { sound, surface } => {
                    let mut nodes = bindings.iter().filter(|(_, binding)| {
                        binding.actor == actor && binding.source == request.0
                    });
                    let (entity, _) = nodes.next().ok_or_else(|| {
                        anyhow::anyhow!("animation sound scene binding unavailable")
                    })?;
                    anyhow::ensure!(
                        nodes.next().is_none(),
                        "animation sound scene binding ambiguous"
                    );
                    sounds.write(SoundCue::Animation {
                        actor: entity,
                        sound,
                        surface,
                    });
                }
                sound => {
                    sounds.write(SoundCue::Script {
                        generation: actor.generation,
                        index: actor.index,
                        sound,
                    });
                }
            }
            Ok(())
        },
        100_000,
    );
    if let Err(error) = result {
        actors.error = Some(format!("automatic ordinary update: {error:#}"));
    }
}
