//! Explicit admitted static-ground updates shared by native and browser player paths.
use super::{
    Player,
    data::{PlayerScene, PlayerSceneRequest},
    grounded::{self, ForwardWalk, GroundedUpdate, GroundedWalk, StaticGroundQuery, SurfaceUpdate},
    placement::{MarkerEntry, PlayerPose},
    walk::WalkingStartup,
};
use anyhow::Context;
use bevy::ecs::system::SystemParam;
use bevy::prelude::{
    AssetId, Assets, Commands, Entity, Handle, Mat3, Message, MessageReader, Messages, Query, Res,
    ResMut, Resource, State, Transform, Vec3, With,
};
use cyrus_world::{
    animation::{ActorAnimation, AnimationPose, AnimationRequest, Animations, ModelFrame},
    collision::ground::SupportHistory,
    sound::{SoundCue, SoundSurface},
};

/// One caller-admitted ordinary sheathed update with animation enabled.
/// The caller supplies original timing, candidate-view coordinates and collision policy.
/// This message does not derive a clock, admit dynamic support, or interpret other actions.
#[derive(Message)]
pub struct GroundedTick {
    pub player: Entity,
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
}

struct Running {
    walk: GroundedWalk,
    animation: ActorAnimation,
    frame: Option<ModelFrame>,
    history: SupportHistory,
    veto: i32,
}

/// Retained selected-player simulation. Failure stops further steps until entry changes.
#[derive(Resource, Default)]
pub struct PlayerRuntime {
    source: Option<Source>,
    running: Option<Running>,
    pub error: Option<String>,
}

#[derive(SystemParam)]
pub(super) struct RuntimeAssets<'w> {
    scene_request: Option<Res<'w, PlayerSceneRequest>>,
    animation_request: Option<Res<'w, AnimationRequest>>,
    scenes: Option<Res<'w, Assets<PlayerScene>>>,
    animations: Option<Res<'w, Assets<Animations>>>,
}

pub(super) fn step(
    mut commands: Commands,
    mut ticks: MessageReader<GroundedTick>,
    simulation: (ResMut<PlayerRuntime>, Option<ResMut<Messages<SoundCue>>>),
    entry: (Res<State<crate::AppState>>, Res<MarkerEntry>),
    assets: RuntimeAssets,
    mut players: Query<(Entity, Option<&mut PlayerPose>, &mut Transform), With<Player>>,
) {
    let (mut runtime, mut sounds) = simulation;
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
    ) = (
        assets.scene_request,
        assets.animation_request,
        assets.scenes,
        assets.animations,
        pose,
    )
    else {
        *runtime = PlayerRuntime::default();
        commands
            .entity(entity)
            .remove::<(AnimationPose, SoundSurface)>();
        ticks.clear();
        return;
    };
    let source = Source {
        player: entity,
        scene: scene_request.0.id(),
        animation: animation_request.0.id(),
        marker: *marker,
    };
    if state.get() != &crate::AppState::InGame || runtime.source.as_ref() != Some(&source) {
        *runtime = PlayerRuntime::default();
        commands
            .entity(entity)
            .remove::<(AnimationPose, SoundSurface)>();
    }
    if state.get() != &crate::AppState::InGame {
        ticks.clear();
        return;
    }
    runtime.source = Some(source);
    for tick in ticks.read() {
        if tick.player != entity
            || tick.entry != *marker
            || tick.source != scene_request.0
            || tick.animation != animation_request.0
        {
            continue;
        }
        if runtime.error.is_some() {
            continue;
        }
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
            let mut emit = |sound| {
                sounds
                    .as_mut()
                    .context("player animation: sound consumer unavailable")?
                    .write(SoundCue::Animation {
                        actor: entity,
                        sound,
                    });
                Ok(())
            };
            if runtime.running.is_none() {
                let definition = scene
                    .definition
                    .as_ref()
                    .context("player step: definition unavailable")?;
                let (animation, frame) =
                    super::placement::initial_animation(data, tick.nominal_ticks, &mut emit)?;
                runtime.running = Some(Running {
                    walk: GroundedWalk {
                        position: pose.position,
                        surface: 0,
                        startup: WalkingStartup::new(definition.step_type)
                            .context("player step: unsupported step type")?,
                    },
                    animation,
                    frame: frame.map(|f| data.resolve_frame("CYRUS", f)).transpose()?,
                    history: SupportHistory::new(0, 0),
                    veto: 0,
                });
            }
            let running = runtime.running.as_mut().unwrap();
            if let Some(frame) = grounded::advance_animation(
                scene,
                tick.nominal_ticks,
                &mut running.history,
                &mut running.animation,
                &mut emit,
            )? {
                running.frame = Some(data.resolve_frame("CYRUS", frame)?);
            }
            let candidates = scene.candidates(
                tick.view,
                running.walk.position,
                running.frame.as_ref().map(|f| f.model.as_str()),
            )?;
            let query = StaticGroundQuery {
                scene,
                candidates: &candidates,
                rotation: tick.rotation,
                collision_attribute: tick.collision_attribute,
                alternate_collision: tick.alternate_collision,
                sidedness: tick.sidedness,
                surface_update: tick.surface_update,
            };
            let mut input = GroundedUpdate {
                motion: ForwardWalk {
                    heading: pose.heading,
                    frame_scale: tick.frame_scale,
                    nominal_ticks: tick.nominal_ticks,
                },
                walk: tick.walk,
            };
            if let Some(frame) = running.walk.move_grounded(
                &query,
                &mut input,
                (&mut running.history, &mut running.veto),
                &mut running.animation,
                &mut emit,
            )? {
                running.frame = Some(data.resolve_frame("CYRUS", frame)?);
            }
            let delta = std::array::from_fn::<_, 3, _>(|i| {
                (running.walk.position[i] - pose.position[i]) as f32
                    / (256.0 * crate::ENGINE_UNIT_SCALE)
            });
            transform.translation += Vec3::new(-delta[0], -delta[1], delta[2]);
            pose.position = running.walk.position;
            pose.heading = input.motion.heading;
            if let Some(frame) = &running.frame {
                commands.entity(entity).insert(AnimationPose {
                    model: frame.model.clone(),
                    frame: frame.frame,
                });
            }
            commands
                .entity(entity)
                .insert(SoundSurface(running.walk.surface));
            Ok(())
        })();
        if let Err(error) = result {
            let error = format!("player step: {error:#}");
            log::error!("{error}");
            runtime.error = Some(error);
        }
    }
}
