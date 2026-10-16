//! Explicit admitted static-ground updates shared by native and browser player paths.
use super::{
    Player,
    data::{PlayerScene, PlayerSceneRequest},
    grounded::SurfaceUpdate,
    placement::{MarkerEntry, PlayerPose},
};
use crate::script::{ScriptActorId, ScriptActors, ScriptBinding};
use anyhow::Context;
use bevy::ecs::system::SystemParam;
use bevy::prelude::{
    AssetId, Assets, Commands, Entity, Handle, Mat3, Message, MessageReader, Messages, Query, Res,
    ResMut, Resource, State, Transform, Vec3, With,
};
use cyrus_world::{
    animation::{AnimationPose, AnimationRequest, Animations},
    sound::{SoundCue, SoundSurface},
};

/// One caller-admitted ordinary sheathed update with animation enabled.
/// The caller supplies original timing, candidate-view coordinates and collision policy.
/// This message does not derive a clock, admit dynamic support, or interpret other actions.
#[derive(Message)]
pub struct GroundedTick {
    pub player: Entity,
    /// Logical player identity from the currently prepared script world.
    pub actor: ScriptActorId,
    pub entry: MarkerEntry,
    pub source: Handle<PlayerScene>,
    pub animation: Handle<Animations>,
    /// Whole nominal tick increment, including zero.
    pub nominal_ticks: u32,
    /// Movement scale with eight fractional bits, independent of whole ticks.
    pub frame_scale: u32,
    /// Original X/Z terrain-view center in map units.
    pub view: [i32; 2],
    /// Caller-established actor orientation in original axes.
    pub rotation: Mat3,
    pub collision_attribute: u8,
    pub alternate_collision: bool,
    pub sidedness: bool,
    pub surface_update: SurfaceUpdate,
    pub walk: bool,
}

#[derive(PartialEq)]
struct Source {
    player: Entity,
    scene: AssetId<PlayerScene>,
    animation: AssetId<Animations>,
    marker: MarkerEntry,
    generation: u64,
}

/// Presentation-entry identity and failure for explicit player updates.
/// Movement continuation belongs to the logical actor in ScriptWorld.
#[derive(Resource, Default)]
pub struct PlayerRuntime {
    source: Option<Source>,
    entry_pending: bool,
    pub(crate) explicit_updated: bool,
    pub error: Option<String>,
}

/// Explicit-update continuation; logical movement remains in the world snapshot.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct PlayerRuntimeSnapshot {
    version: u32,
    source: Option<RuntimeSourceSnapshot>,
    entry_pending: bool,
    explicit_updated: bool,
    error: Option<String>,
}
#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct RuntimeSourceSnapshot {
    player: u64,
    generation: u64,
    marker: MarkerEntry,
}

/// Host-validated source and identity bindings for the prepared player candidate.
pub struct PlayerRuntimeBinding {
    pub saved_player: u64,
    pub saved_generation: u64,
    pub player: Entity,
    pub generation: u64,
    pub scene: Handle<PlayerScene>,
    pub animation: Handle<Animations>,
    pub marker: MarkerEntry,
}

impl PlayerRuntimeSnapshot {
    /// Saved player entity and logical generation needed by the host's binding census.
    pub fn source_identity(&self) -> Option<(u64, u64)> {
        self.source
            .as_ref()
            .map(|source| (source.player, source.generation))
    }
}

impl PlayerRuntime {
    /// Capture at a completed update boundary against the active prepared sources.
    pub fn snapshot(
        &self,
        scene: &Handle<PlayerScene>,
        animation: &Handle<Animations>,
    ) -> anyhow::Result<PlayerRuntimeSnapshot> {
        if let Some(source) = &self.source {
            anyhow::ensure!(
                source.scene == scene.id() && source.animation == animation.id(),
                "player runtime snapshot: active source identity differs"
            );
        }
        Ok(PlayerRuntimeSnapshot {
            version: 1,
            source: self.source.as_ref().map(|source| RuntimeSourceSnapshot {
                player: source.player.to_bits(),
                generation: source.generation,
                marker: source.marker,
            }),
            entry_pending: self.entry_pending,
            explicit_updated: self.explicit_updated,
            error: self.error.clone(),
        })
    }

    /// Validate an off-live replacement without entry initialization or grounded updates.
    /// The aggregate validates installed source content before supplying the binding.
    pub fn from_snapshot(
        snapshot: PlayerRuntimeSnapshot,
        binding: Option<PlayerRuntimeBinding>,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(
            snapshot.version == 1,
            "player runtime snapshot version unsupported"
        );
        let source = match (snapshot.source, binding) {
            (None, None) => None,
            (Some(saved), Some(binding)) => {
                anyhow::ensure!(
                    saved.player == binding.saved_player
                        && saved.generation == binding.saved_generation
                        && saved.marker == binding.marker,
                    "player runtime snapshot: source binding differs"
                );
                anyhow::ensure!(
                    binding.generation != saved.generation,
                    "player runtime snapshot: replacement generation is not fresh"
                );
                Some(Source {
                    player: binding.player,
                    scene: binding.scene.id(),
                    animation: binding.animation.id(),
                    marker: binding.marker,
                    generation: binding.generation,
                })
            }
            _ => anyhow::bail!("player runtime snapshot: source binding census differs"),
        };
        anyhow::ensure!(
            source.is_some() || (!snapshot.entry_pending && !snapshot.explicit_updated),
            "player runtime snapshot: update state lacks a source"
        );
        Ok(Self {
            source,
            entry_pending: snapshot.entry_pending,
            explicit_updated: snapshot.explicit_updated,
            error: snapshot.error,
        })
    }
}

#[derive(SystemParam)]
pub(crate) struct RuntimeAssets<'w> {
    scene_request: Option<Res<'w, PlayerSceneRequest>>,
    animation_request: Option<Res<'w, AnimationRequest>>,
    scenes: Option<Res<'w, Assets<PlayerScene>>>,
    animations: Option<Res<'w, Assets<Animations>>>,
}

pub(crate) fn step(
    mut commands: Commands,
    mut ticks: MessageReader<GroundedTick>,
    simulation: (ResMut<PlayerRuntime>, Option<ResMut<Messages<SoundCue>>>),
    mut actors: Option<ResMut<ScriptActors>>,
    entry: (Res<State<crate::AppState>>, Res<MarkerEntry>),
    assets: RuntimeAssets,
    mut players: Query<(Entity, Option<&mut PlayerPose>, &mut Transform), With<Player>>,
) {
    let (mut runtime, mut sounds) = simulation;
    runtime.explicit_updated = false;
    let (state, marker) = entry;
    let Ok((entity, pose, mut transform)) = players.single_mut() else {
        *runtime = PlayerRuntime::default();
        ticks.clear();
        return;
    };
    let (
        Some(scene_request),
        Some(animation_request),
        Some(scenes),
        Some(animations),
        Some(mut pose),
        Some(world),
    ) = (
        assets.scene_request,
        assets.animation_request,
        assets.scenes,
        assets.animations,
        pose,
        actors.as_mut().and_then(|actors| actors.current.as_mut()),
    )
    else {
        *runtime = PlayerRuntime::default();
        commands
            .entity(entity)
            .remove::<(ScriptBinding, AnimationPose, SoundSurface)>();
        ticks.clear();
        return;
    };
    let source = Source {
        player: entity,
        scene: scene_request.0.id(),
        animation: animation_request.0.id(),
        marker: *marker,
        generation: world.generation(),
    };
    if state.get() != &crate::AppState::InGame || runtime.source.as_ref() != Some(&source) {
        *runtime = PlayerRuntime::default();
        runtime.entry_pending = true;
        commands
            .entity(entity)
            .remove::<(ScriptBinding, AnimationPose, SoundSurface)>();
    }
    if state.get() != &crate::AppState::InGame {
        ticks.clear();
        return;
    }
    runtime.source = Some(source);
    for tick in ticks.read() {
        if tick.player != entity
            || tick.actor.generation != world.generation()
            || tick.entry != *marker
            || tick.source != scene_request.0
            || tick.animation != animation_request.0
        {
            continue;
        }
        if runtime.error.is_some() {
            continue;
        }
        runtime.explicit_updated = true;
        let result = (|| -> anyhow::Result<()> {
            let scene = scenes
                .get(&tick.source)
                .context("player step: scene unavailable")?;
            let data = animations
                .get(&tick.animation)
                .context("player step: animation unavailable")?;
            anyhow::ensure!(
                scene.world == data.world,
                "player step: world {} differs from animation world {}",
                scene.world,
                data.world
            );
            anyhow::ensure!(
                tick.rotation.is_finite(),
                "world {} player step: nonfinite rotation",
                scene.world
            );
            let mut emit = |sound, surface, position: [i64; 3]| {
                let captured_position = if matches!(
                    &sound,
                    cyrus_world::animation::AnimationSound::Play { .. }
                ) {
                    let mut captured = [0; 3];
                    for (axis, value) in position.into_iter().enumerate() {
                        captured[axis] = i32::try_from(value).with_context(|| format!("player animation sound position axis {axis}: Q8 coordinate {value} outside i32"))?;
                    }
                    Some(captured)
                } else {
                    None
                };
                sounds
                    .as_mut()
                    .context("player animation: sound consumer unavailable")?
                    .write(SoundCue::Animation {
                        position: captured_position,
                        actor: entity,
                        sound,
                        surface: Some(surface),
                    });
                Ok(())
            };
            let presentation = world.explicit_grounded_step(
                scene,
                data,
                tick,
                runtime.entry_pending.then_some(&*pose),
                &mut emit,
            )?;
            runtime.entry_pending = false;
            let delta = std::array::from_fn::<_, 3, _>(|i| {
                (presentation.pose.position[i] - pose.position[i]) as f32
                    / (256.0 * crate::ENGINE_UNIT_SCALE)
            });
            transform.translation += Vec3::new(-delta[0], -delta[1], delta[2]);
            *pose = presentation.pose;
            if let Some(frame) = presentation.frame {
                commands.entity(entity).insert(AnimationPose {
                    model: frame.model,
                    frame: frame.frame,
                });
            }
            commands.entity(entity).insert((
                SoundSurface(presentation.surface),
                ScriptBinding {
                    actor: tick.actor,
                    source: tick.animation.clone(),
                },
            ));
            Ok(())
        })();
        if let Err(error) = result {
            let error = format!("player step: {error:#}");
            bevy::log::error_once!("{error}");
            runtime.error = Some(error);
        }
    }
}
