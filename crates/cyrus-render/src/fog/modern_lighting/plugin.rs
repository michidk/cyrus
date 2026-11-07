//! The modern lighting plugin, its camera effects and teardown, and the markers the other lighting
//! modules share.
use super::{
    Proxy,
    lights::sync_lights,
    proxies::{hide_draws, sync_proxies},
};
use crate::camera::ViewCamera;
use bevy::{
    anti_alias::smaa::Smaa,
    camera::{Hdr, visibility::RenderLayers},
    core_pipeline::prepass::{DepthPrepass, NormalPrepass},
    light::{EnvironmentMapLight, ShadowFilteringMethod},
    pbr::{ContactShadows, ScreenSpaceAmbientOcclusion},
    post_process::bloom::Bloom,
    prelude::*,
};
use cyrus_game::fidelity::{Area, Fidelity, Mode};

/// Lights the world with PBR stand-ins while `Area::Lighting` is Modern. Add after `DefaultPlugins`.
pub struct ModernLightingPlugin;

impl Plugin for ModernLightingPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PostUpdate,
            (
                (
                    sync_proxies,
                    hide_draws.after(sync_proxies),
                    sync_lights,
                    configure_camera,
                )
                    .run_if(lit),
                restore.run_if(not(lit)),
            )
                .after(TransformSystems::Propagate)
                .after(bevy::camera::visibility::VisibilitySystems::VisibilityPropagate),
        );
    }
}

fn lit(fidelity: Res<Fidelity>) -> bool {
    fidelity.mode(Area::Lighting) == Mode::Modern
}

/// The source mesh a [`Proxy`] stands in for.
#[derive(Component)]
pub(super) struct StandInOf(pub(super) Entity);

/// A source mesh that has a stand-in; its own draw is hidden.
#[derive(Component)]
pub(super) struct Replaced;

/// The render layers an entity had before its draw was hidden for stand-ins.
#[derive(Component)]
pub(super) struct Hidden(pub(super) Option<RenderLayers>);

/// A light, or the marker that lights were built, owned by this module.
#[derive(Component)]
pub(super) struct Light;

/// Camera effects. Everything is Bevy's own; a Modern tuning choice. Temporal
/// anti-aliasing is left out: the stand-ins of animated meshes are rewritten in place, so they have
/// no valid motion vectors, and the original shader draws write none.
#[derive(Bundle)]
struct CameraEffects {
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

impl Default for CameraEffects {
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

fn configure_camera(
    mut commands: Commands,
    cameras: Query<Entity, (With<ViewCamera>, Without<ScreenSpaceAmbientOcclusion>)>,
) {
    for camera in &cameras {
        commands.entity(camera).insert(CameraEffects::default());
    }
}

/// Undoes everything this module changed once modern lighting is off.
#[allow(clippy::type_complexity)] // Bevy query filters.
fn restore(
    mut commands: Commands,
    cameras: Query<Entity, (With<ViewCamera>, With<ScreenSpaceAmbientOcclusion>)>,
    owned: Query<Entity, Or<(With<Proxy>, With<Light>)>>,
    hidden: Query<(Entity, &Hidden)>,
) {
    for camera in &cameras {
        commands.entity(camera).remove::<(
            Hdr,
            DepthPrepass,
            NormalPrepass,
            ScreenSpaceAmbientOcclusion,
            ContactShadows,
            Smaa,
            Bloom,
            ShadowFilteringMethod,
            EnvironmentMapLight,
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
    entity.remove::<(Hidden, Replaced)>();
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
            configure_camera,
        ));
        schedule
            .initialize(&mut world)
            .expect("systems initialize without query conflicts");
    }
}
