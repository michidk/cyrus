//! The Solari plugin, its camera setup and teardown, and the markers the other ray tracing modules share.
use super::{
    Proxy, RayTracingSupport,
    lights::sync_lights,
    proxies::{hide_draws, sync_proxies},
};
use crate::camera::ViewCamera;
use bevy::{
    camera::{CameraMainTextureUsages, Hdr, visibility::RenderLayers},
    core_pipeline::prepass::{DeferredPrepass, DepthPrepass, MotionVectorPrepass},
    light::EnvironmentMapLight,
    pbr::DefaultOpaqueRendererMethod,
    prelude::*,
    render::{RenderApp, render_resource::TextureUsages, renderer::RenderDevice},
    solari::prelude::{SolariLighting, SolariPlugins},
};
use cyrus_game::fidelity::{Area, Fidelity, Mode};

/// Adds Solari behind the `RayTracing` fidelity area. Add after `DefaultPlugins`.
pub struct RayTracingPlugin;

impl Plugin for RayTracingPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(SolariPlugins)
            // Solari makes every opaque material deferred, which would bypass this
            // renderer's forward shaders. Only the stand-ins opt in.
            .insert_resource(DefaultOpaqueRendererMethod::forward())
            .init_resource::<RayTracingSupport>()
            .add_systems(
                PostUpdate,
                (
                    (
                        configure_camera,
                        sync_proxies,
                        hide_draws.after(sync_proxies),
                        sync_lights,
                    )
                        .run_if(active),
                    restore.run_if(not(active)),
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

fn active(fidelity: Res<Fidelity>, support: Res<RayTracingSupport>) -> bool {
    fidelity.mode(Area::RayTracing) == Mode::Modern && support.is_available()
}

/// The source mesh a [`Proxy`] stands in for.
#[derive(Component)]
pub(super) struct StandInOf(pub(super) Entity);

/// A source mesh that has a stand-in; its own draw is hidden.
#[derive(Component)]
pub(super) struct Traced;

/// The render layers an entity had before its draw was hidden for tracing.
#[derive(Component)]
pub(super) struct Hidden(pub(super) Option<RenderLayers>);

/// A light, or the marker that lights were built, owned by this module.
#[derive(Component)]
pub(super) struct Light;

fn configure_camera(
    mut commands: Commands,
    cameras: Query<Entity, (With<ViewCamera>, Without<SolariLighting>)>,
) {
    for camera in &cameras {
        // `Msaa::Off` is already set at spawn. Solari also requires storage binding,
        // and brings HDR and the deferred, depth and motion prepasses as required components.
        commands.entity(camera).insert((
            SolariLighting::default(),
            CameraMainTextureUsages::default().with(TextureUsages::STORAGE_BINDING),
        ));
    }
}

/// Undoes everything this module changed once ray tracing is off or unsupported.
#[allow(clippy::type_complexity)] // Bevy query filters.
fn restore(
    mut commands: Commands,
    cameras: Query<Entity, (With<ViewCamera>, With<SolariLighting>)>,
    owned: Query<Entity, Or<(With<Proxy>, With<Light>)>>,
    hidden: Query<(Entity, &Hidden)>,
) {
    for camera in &cameras {
        commands.entity(camera).remove::<(
            SolariLighting,
            CameraMainTextureUsages,
            EnvironmentMapLight,
            Hdr,
            DeferredPrepass,
            DepthPrepass,
            MotionVectorPrepass,
        )>();
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
            configure_camera,
            restore,
            sync_proxies,
            hide_draws,
            sync_lights,
        ));
        schedule
            .initialize(&mut world)
            .expect("systems initialize without query conflicts");
    }
}
