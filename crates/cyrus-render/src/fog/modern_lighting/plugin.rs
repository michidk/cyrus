//! The modern lighting plugins, the Solari camera setup and teardown, and the markers the other
//! lighting modules share.
use super::{
    Proxy, RayTracingSupport, Tier,
    lights::sync_lights,
    proxies::{hide_draws, sync_proxies},
};
use crate::camera::ViewCamera;
use bevy::{
    anti_alias::smaa::Smaa,
    camera::{Hdr, visibility::RenderLayers},
    core_pipeline::prepass::{DepthPrepass, NormalPrepass},
    ecs::system::SystemParam,
    light::{EnvironmentMapLight, ShadowFilteringMethod},
    pbr::{ContactShadows, ScreenSpaceAmbientOcclusion},
    post_process::bloom::Bloom,
    prelude::*,
};
use cyrus_game::fidelity::Fidelity;

/// Lights the world with PBR stand-ins while `Area::Lighting` or `Area::RayTracing` is Modern.
/// Add after `DefaultPlugins`.
pub struct ModernLightingPlugin;

impl Plugin for ModernLightingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RayTracingSupport>().add_systems(
            PostUpdate,
            (
                (sync_proxies, hide_draws.after(sync_proxies), sync_lights).run_if(lit),
                configure_raster_camera.run_if(raster),
                leave_raster,
                restore.run_if(not(lit)),
            )
                .after(TransformSystems::Propagate)
                .after(bevy::camera::visibility::VisibilitySystems::VisibilityPropagate),
        );
    }
}

/// The lighting tier the fidelity settings select.
#[derive(SystemParam)]
pub(super) struct ActiveTier<'w> {
    fidelity: Res<'w, Fidelity>,
    support: Res<'w, RayTracingSupport>,
}

impl ActiveTier<'_> {
    pub(super) fn get(&self) -> Option<Tier> {
        Tier::select(&self.fidelity, &self.support)
    }
}

fn lit(tier: ActiveTier) -> bool {
    tier.get().is_some()
}

fn raster(tier: ActiveTier) -> bool {
    tier.get() == Some(Tier::Raster)
}

/// The source mesh a [`Proxy`] stands in for.
#[derive(Component)]
pub(super) struct StandInOf(pub(super) Entity);

/// A source mesh that has a stand-in; its own draw is hidden.
#[derive(Component)]
pub(super) struct Traced;

/// The render layers an entity had before its draw was hidden for stand-ins.
#[derive(Component)]
pub(super) struct Hidden(pub(super) Option<RenderLayers>);

/// A light, or the marker that lights were built, owned by this module.
#[derive(Component)]
pub(super) struct Light;

/// Camera effects of the raster tier. Everything is Bevy's own; a Modern tuning choice. Temporal
/// anti-aliasing is left out: the stand-ins of animated meshes are rewritten in place, so they have
/// no valid motion vectors, and the original shader draws write none.
#[derive(Bundle)]
struct RasterEffects {
    hdr: Hdr,
    // Screen-space ambient occlusion and contact shadows need depth, and the former normals.
    depth: DepthPrepass,
    normals: NormalPrepass,
    ambient_occlusion: ScreenSpaceAmbientOcclusion,
    contact_shadows: ContactShadows,
    anti_aliasing: Smaa,
    bloom: Bloom,
    shadow_filtering: ShadowFilteringMethod,
}

impl Default for RasterEffects {
    fn default() -> Self {
        Self {
            hdr: Hdr,
            depth: DepthPrepass,
            normals: NormalPrepass,
            ambient_occlusion: ScreenSpaceAmbientOcclusion::default(),
            contact_shadows: ContactShadows::default(),
            anti_aliasing: Smaa::default(),
            bloom: Bloom::NATURAL,
            shadow_filtering: ShadowFilteringMethod::Gaussian,
        }
    }
}

fn configure_raster_camera(
    mut commands: Commands,
    cameras: Query<Entity, (With<ViewCamera>, Without<ScreenSpaceAmbientOcclusion>)>,
) {
    for camera in &cameras {
        commands.entity(camera).insert(RasterEffects::default());
    }
}

/// Removes the raster effects once the tier changes. Solari brings its own HDR and prepasses, so
/// those stay while it lights the scene.
fn leave_raster(
    mut commands: Commands,
    tier: ActiveTier,
    cameras: Query<Entity, (With<ViewCamera>, With<ScreenSpaceAmbientOcclusion>)>,
) {
    let tier = tier.get();
    if tier == Some(Tier::Raster) {
        return;
    }
    for camera in &cameras {
        let mut camera = commands.entity(camera);
        camera.remove::<(
            ScreenSpaceAmbientOcclusion,
            ContactShadows,
            Smaa,
            Bloom,
            ShadowFilteringMethod,
        )>();
        if tier.is_none() {
            camera.remove::<(Hdr, DepthPrepass, NormalPrepass)>();
        }
    }
}

/// Undoes everything this module changed once modern lighting is off.
#[allow(clippy::type_complexity)] // Bevy query filters.
fn restore(
    mut commands: Commands,
    cameras: Query<Entity, (With<ViewCamera>, With<EnvironmentMapLight>)>,
    owned: Query<Entity, Or<(With<Proxy>, With<Light>)>>,
    hidden: Query<(Entity, &Hidden)>,
) {
    for camera in &cameras {
        commands.entity(camera).remove::<EnvironmentMapLight>();
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

#[cfg(feature = "raytracing")]
pub use tracing::RayTracingPlugin;

#[cfg(feature = "raytracing")]
mod tracing {
    use super::{ActiveTier, Light, Proxy, RayTracingSupport, Tier, sync_proxies};
    use crate::{camera::ViewCamera, environment::EnvironmentRequest};
    use bevy::{
        camera::{CameraMainTextureUsages, Hdr},
        core_pipeline::prepass::{DeferredPrepass, DepthPrepass, MotionVectorPrepass},
        pbr::DefaultOpaqueRendererMethod,
        prelude::*,
        render::{RenderApp, render_resource::TextureUsages, renderer::RenderDevice},
        solari::prelude::{RaytracingMesh3d, SolariLighting, SolariPlugins},
    };

    /// Adds Solari behind the `RayTracing` fidelity area. Add after [`super::ModernLightingPlugin`].
    pub struct RayTracingPlugin;

    impl Plugin for RayTracingPlugin {
        fn build(&self, app: &mut App) {
            app.add_plugins(SolariPlugins)
                // Solari makes every opaque material deferred, which would bypass this
                // renderer's forward shaders. Only the traced stand-ins opt in.
                .insert_resource(DefaultOpaqueRendererMethod::forward())
                .add_systems(
                    PostUpdate,
                    (
                        (
                            configure_camera,
                            trace_stand_ins.after(sync_proxies),
                            reset_on_cut,
                        )
                            .run_if(traced),
                        leave_tracing.run_if(not(traced)),
                    )
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
                Some(device) if device.features().contains(required) => {
                    RayTracingSupport::Available
                }
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

    fn traced(tier: ActiveTier) -> bool {
        tier.get() == Some(Tier::Traced)
    }

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
            // Solari's default cells are 0.15 m, sized for a room. An island has far more cells
            // than the per-frame update budget can fill, and the unfilled ones show as speckle in
            // shadowed areas. World units are metres (**Inferred** from the glTF export scale).
            world_cache_position_base_cell_size: 1.0,
            // Reuse light samples for longer between frames.
            confidence_weight_cap: 24.0,
            ..default()
        }
    }

    pub(super) fn configure_camera(
        mut commands: Commands,
        cameras: Query<Entity, (With<ViewCamera>, Without<SolariLighting>)>,
    ) {
        for camera in &cameras {
            // `Msaa::Off` is already set at spawn. Solari also requires storage binding,
            // and brings HDR and the deferred, depth and motion prepasses as required components.
            commands.entity(camera).insert((
                solari_lighting(),
                CameraMainTextureUsages::default().with(TextureUsages::STORAGE_BINDING),
            ));
        }
    }

    /// Camera translation, in world units, past which a frame is a cut and history is dropped.
    const CUT_DISTANCE: f32 = 5.0;
    /// Camera rotation, in radians, past which a frame is a cut and history is dropped.
    const CUT_ANGLE: f32 = 0.8;

    /// Drops the denoiser's and ReSTIR's history when the view jumps or the world changes, so the
    /// new view is not smeared with the old one.
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

    /// Lets Solari trace each stand-in and each emissive light.
    pub(super) fn trace_stand_ins(
        mut commands: Commands,
        proxies: Query<(Entity, &Mesh3d, Option<&RaytracingMesh3d>), With<Proxy>>,
    ) {
        for (entity, mesh, tracing) in &proxies {
            if tracing.is_none_or(|tracing| tracing.0 != mesh.0) {
                commands
                    .entity(entity)
                    .insert(RaytracingMesh3d(mesh.0.clone()));
            }
        }
    }

    /// Undoes what tracing added once Solari is no longer lighting the scene.
    #[allow(clippy::type_complexity)] // Bevy query filters.
    pub(super) fn leave_tracing(
        mut commands: Commands,
        cameras: Query<Entity, (With<ViewCamera>, With<SolariLighting>)>,
        traced: Query<Entity, (With<RaytracingMesh3d>, Or<(With<Proxy>, With<Light>)>)>,
    ) {
        for camera in &cameras {
            commands.entity(camera).remove::<(
                SolariLighting,
                CameraMainTextureUsages,
                Hdr,
                DeferredPrepass,
                DepthPrepass,
                MotionVectorPrepass,
            )>();
        }
        for entity in &traced {
            commands.entity(entity).remove::<RaytracingMesh3d>();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        lights::sync_lights,
        proxies::{hide_draws, sync_proxies},
    };
    use super::*;

    // Bevy rejects overlapping mutable and shared queries when a system is first set up, which
    // panicked at launch before this was covered.
    #[test]
    fn systems_have_disjoint_queries() {
        let mut world = World::new();
        let mut schedule = Schedule::default();
        schedule.add_systems((
            restore,
            sync_proxies,
            hide_draws,
            sync_lights,
            configure_raster_camera,
            leave_raster,
        ));
        schedule
            .initialize(&mut world)
            .expect("systems initialize without query conflicts");
    }

    #[cfg(feature = "raytracing")]
    #[test]
    fn tracing_systems_have_disjoint_queries() {
        let mut world = World::new();
        let mut schedule = Schedule::default();
        schedule.add_systems((
            super::tracing::configure_camera,
            super::tracing::reset_on_cut,
            super::tracing::trace_stand_ins,
            super::tracing::leave_tracing,
        ));
        schedule
            .initialize(&mut world)
            .expect("systems initialize without query conflicts");
    }

    #[cfg(feature = "raytracing")]
    #[test]
    fn a_view_jump_resets_the_lighting_history() {
        use crate::environment::EnvironmentRequest;
        use bevy::solari::prelude::SolariLighting;
        let mut app = App::new();
        app.insert_resource(EnvironmentRequest(Handle::default()))
            .add_systems(Update, super::tracing::reset_on_cut);
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
}
