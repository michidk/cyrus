//! The ray-tracing plugin: Solari's camera setup and teardown, the history reset on cuts, and the
//! markers the stand-in and light modules share.
use super::super::modern_lighting::{CameraHandover, candle, day_night};
use super::{
    RayTracingReference, RayTracingSupport, StandIn,
    lights::sync_lights,
    proxies::{hide_draws, sync_proxies},
    ray_traced,
};
use crate::{camera::ViewCamera, environment::EnvironmentRequest};
use bevy::{
    camera::{CameraMainTextureUsages, Hdr, visibility::RenderLayers},
    core_pipeline::{
        prepass::{
            DeferredPrepass, DeferredPrepassDoubleBuffer, DepthPrepass, DepthPrepassDoubleBuffer,
            MotionVectorPrepass,
        },
        tonemapping::Tonemapping,
    },
    light::EnvironmentMapLight,
    pbr::DefaultOpaqueRendererMethod,
    prelude::*,
    render::{RenderApp, render_resource::TextureUsages, renderer::RenderDevice},
    solari::{
        pathtracer::{Pathtracer, PathtracingPlugin},
        prelude::{SolariLighting, SolariPlugins},
    },
};
use cyrus_game::fidelity::Fidelity;

/// Lights the world with Solari while `Area::RayTracing` is Modern and the GPU can trace. Add after
/// `DefaultPlugins` and the world renderer.
pub struct RayTracingPlugin;

impl Plugin for RayTracingPlugin {
    fn build(&self, app: &mut App) {
        CameraHandover::configure(app);
        app.add_plugins((SolariPlugins, PathtracingPlugin, super::haze::HazePlugin))
            .init_resource::<RayTracingReference>()
            // Solari makes every opaque material deferred, which would bypass this renderer's
            // forward shaders. Only the traced stand-ins opt in.
            .insert_resource(DefaultOpaqueRendererMethod::forward())
            // Update, so a new sun is propagated and gets its shadow cascades in the same frame:
            // Bevy builds cascades in PostUpdate, and a light without them panics in the renderer.
            .add_systems(
                Update,
                (
                    (
                        (
                            day_night::advance,
                            sync_lights,
                            day_night::apply,
                            candle::track,
                        )
                            .chain(),
                        configure_camera.in_set(CameraHandover::Setup),
                        super::haze::update_traced_fog.after(super::super::apply_scene_fog),
                    )
                        .run_if(traced),
                    // After flat preparation, whose queued markers on the stand-ins must land
                    // before they are despawned.
                    leave_tracing
                        .run_if(not(traced))
                        .in_set(CameraHandover::Teardown)
                        .after(crate::flat::PrepareFlats),
                ),
            )
            .add_systems(
                PostUpdate,
                (sync_proxies, hide_draws.after(sync_proxies), reset_on_cut)
                    .run_if(traced)
                    .after(TransformSystems::Propagate)
                    .after(bevy::camera::visibility::VisibilitySystems::VisibilityPropagate),
            );
    }

    fn finish(&self, app: &mut App) {
        let required = SolariPlugins::required_wgpu_features();
        let support = match app
            .get_sub_app(RenderApp)
            .and_then(|render| render.world().get_resource::<RenderDevice>())
        {
            None => RayTracingSupport::Unavailable,
            Some(device) if device.features().contains(required) => RayTracingSupport::Available,
            Some(device) => RayTracingSupport::MissingFeatures(format!(
                "{:?}",
                required.difference(device.features())
            )),
        };
        if support != RayTracingSupport::Available {
            warn!("ray tracing unavailable: {support:?}");
        }
        app.insert_resource(support);
    }
}

pub(super) fn traced(fidelity: Res<Fidelity>, support: Res<RayTracingSupport>) -> bool {
    ray_traced(&fidelity, &support)
}

/// A source mesh that has a stand-in; its own draw is hidden.
#[derive(Component)]
pub(super) struct Traced;

/// The render layers an entity had before its draw was hidden for stand-ins.
#[derive(Component)]
pub(super) struct Hidden(pub(super) Option<RenderLayers>);

/// A light, or the marker that lights were built, owned by this module.
#[derive(Component)]
pub(super) struct Light;

/// Solari settings; a Modern tuning choice. Solari's own output is raw per-pixel sampling noise
/// that upstream expects DLSS Ray Reconstruction to clean up. This renderer filters it with the
/// vendored denoiser instead, which needs ReSTIR's previous-frame buffers.
fn solari_lighting() -> SolariLighting {
    SolariLighting {
        // Reuses light samples between neighbouring pixels and frames.
        restir: true,
        denoise: true,
        // Sun and emissive light samples per pixel, up from 8 and 4.
        primary_di_samples: 24,
        secondary_di_samples: 8,
        // Longer cache history trades slower reaction to light changes for stability.
        world_cache_max_temporal_samples: 96.0,
        // Solari's default cells are 0.15 m, sized for a room. An island has far more cells than
        // the per-frame update budget can fill, and the unfilled ones show as speckle in shadowed
        // areas. World units are metres (**Inferred** from the glTF export scale).
        world_cache_position_base_cell_size: 1.0,
        // Reuse light samples for longer between frames.
        confidence_weight_cap: 24.0,
        ..default()
    }
}

/// Gives the camera Solari's real-time lighting, or its path tracer while a reference image is
/// requested. Re-adds the HDR target and prepasses if another module removed them.
#[allow(clippy::type_complexity)] // Bevy query filters.
pub(super) fn configure_camera(
    mut commands: Commands,
    reference: Res<RayTracingReference>,
    cameras: Query<
        (
            Entity,
            Has<SolariLighting>,
            Has<Pathtracer>,
            Has<DeferredPrepass>,
            Has<DepthPrepass>,
            Has<MotionVectorPrepass>,
            Has<Hdr>,
            Option<&Tonemapping>,
        ),
        With<ViewCamera>,
    >,
) {
    for (camera, lighting, pathtracer, deferred, depth, motion, hdr, tonemapping) in &cameras {
        let mut camera = commands.entity(camera);
        // `Msaa::Off` is already set at spawn. Both renderers write the main texture as storage.
        // The lights are calibrated so a fully lit surface shows its texture colour, as the raster
        // modern lighting is, so tonemapping stays off.
        let usages = CameraMainTextureUsages::default().with(TextureUsages::STORAGE_BINDING);
        if reference.0 {
            if lighting {
                camera.remove::<SolariLighting>();
            }
            if !pathtracer {
                camera.insert((Pathtracer { reset: true }, usages));
            }
        } else {
            if pathtracer {
                camera.remove::<Pathtracer>();
            }
            if !lighting {
                camera.insert((solari_lighting(), usages));
            }
        }
        if !(deferred && depth && motion && hdr) {
            camera.insert((DeferredPrepass, DepthPrepass, MotionVectorPrepass, Hdr));
        }
        if tonemapping != Some(&Tonemapping::None) {
            camera.insert(Tonemapping::None);
        }
    }
}

/// Camera translation, in world units, past which a frame is a cut and history is dropped.
const CUT_DISTANCE: f32 = 5.0;
/// Camera rotation, in radians, past which a frame is a cut and history is dropped.
const CUT_ANGLE: f32 = 0.8;

/// Drops the denoiser's and ReSTIR's history when the view jumps or the world changes, so the new
/// view is not smeared with the old one.
pub(super) fn reset_on_cut(
    request: Res<EnvironmentRequest>,
    mut cameras: Query<(&GlobalTransform, &mut SolariLighting), With<ViewCamera>>,
    mut previous: Local<Option<Transform>>,
) {
    for (global, mut lighting) in &mut cameras {
        let current = global.compute_transform();
        let cut = previous.is_some_and(|previous| {
            previous.translation.distance(current.translation) > CUT_DISTANCE
                || previous.rotation.angle_between(current.rotation) > CUT_ANGLE
        });
        *previous = Some(current);
        if cut || request.is_changed() {
            lighting.reset = true;
        }
    }
}

/// Undoes everything tracing changed once Solari no longer lights the scene. The raster modern
/// lighting adds back what it needs. Solari's double-buffer markers go too: they require their
/// prepass, and extraction re-inserting them would bring the prepass back into the render world
/// without its textures, which fails validation.
#[allow(clippy::type_complexity)] // Bevy query filters.
pub(super) fn leave_tracing(
    mut commands: Commands,
    cameras: Query<
        Entity,
        (
            With<ViewCamera>,
            Or<(With<SolariLighting>, With<Pathtracer>)>,
        ),
    >,
    owned: Query<Entity, Or<(With<StandIn>, With<Light>)>>,
    hidden: Query<(Entity, &Hidden)>,
) {
    for camera in &cameras {
        commands
            .entity(camera)
            .remove::<(
                SolariLighting,
                Pathtracer,
                CameraMainTextureUsages,
                Hdr,
                DeferredPrepass,
                DeferredPrepassDoubleBuffer,
                DepthPrepass,
                DepthPrepassDoubleBuffer,
                MotionVectorPrepass,
                EnvironmentMapLight,
                super::haze::TracedFog,
            )>()
            .insert(Tonemapping::default());
    }
    for entity in &owned {
        commands.entity(entity).despawn();
    }
    for (entity, hidden) in &hidden {
        unhide(&mut commands, entity, hidden);
    }
}

pub(super) fn unhide(commands: &mut Commands, entity: Entity, hidden: &Hidden) {
    let mut entity = commands.entity(entity);
    match &hidden.0 {
        Some(layers) => entity.insert(layers.clone()),
        None => entity.remove::<RenderLayers>(),
    };
    entity.remove::<(Hidden, Traced)>();
}

#[cfg(test)]
mod tests {
    use super::*;

    // Bevy rejects overlapping mutable and shared queries when a system is first set up, which
    // panicked at launch before this was covered.
    #[test]
    fn systems_have_disjoint_queries() {
        let mut world = World::new();
        let mut schedule = Schedule::default();
        schedule.add_systems((
            sync_lights,
            configure_camera,
            sync_proxies,
            hide_draws,
            reset_on_cut,
            leave_tracing,
        ));
        schedule
            .initialize(&mut world)
            .expect("systems initialize without query conflicts");
    }

    #[test]
    fn a_view_jump_resets_the_lighting_history() {
        let mut app = App::new();
        app.insert_resource(EnvironmentRequest(Handle::default()))
            .add_systems(Update, reset_on_cut);
        let camera = app
            .world_mut()
            .spawn((
                ViewCamera,
                GlobalTransform::default(),
                SolariLighting {
                    reset: false,
                    ..default()
                },
            ))
            .id();
        // The first frames see the new request and no previous view.
        app.update();
        app.update();
        let move_to = |app: &mut App, x: f32| {
            let mut entity = app.world_mut().entity_mut(camera);
            entity.get_mut::<SolariLighting>().unwrap().reset = false;
            *entity.get_mut::<GlobalTransform>().unwrap() =
                GlobalTransform::from(Transform::from_xyz(x, 0.0, 0.0));
            app.update();
            app.world().get::<SolariLighting>(camera).unwrap().reset
        };
        assert!(!move_to(&mut app, 0.1), "walking keeps the history");
        assert!(move_to(&mut app, 50.0), "a jump drops the history");
    }

    // Solari's double-buffer markers require their prepass; one left behind brought the deferred
    // prepass back in the render world without its textures, and leaving tracing crashed.
    #[test]
    fn leaving_tracing_removes_every_prepass_marker() {
        let mut app = App::new();
        app.add_systems(Update, leave_tracing);
        let camera = app
            .world_mut()
            .spawn((
                ViewCamera,
                solari_lighting(),
                DeferredPrepassDoubleBuffer,
                DepthPrepassDoubleBuffer,
            ))
            .id();
        app.update();
        let camera = app.world().entity(camera);
        assert!(!camera.contains::<DeferredPrepassDoubleBuffer>());
        assert!(!camera.contains::<DepthPrepassDoubleBuffer>());
        assert!(!camera.contains::<DeferredPrepass>());
        assert!(!camera.contains::<DepthPrepass>());
        assert!(!camera.contains::<SolariLighting>());
    }
}
