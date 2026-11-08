//! The modern lighting plugin, its camera effects and teardown, and the light marker the light
//! module shares.
use super::day_night::{self, DayNight, Daylight};
use super::effects::{self, AutoExposureAssets, ModernEffects, ShaftVolume};
use super::foliage;
use super::lights::sync_lights;
use super::post::{self, ModernPost, PostPlugin};
use crate::camera::ViewCamera;
use bevy::{
    anti_alias::smaa::Smaa,
    camera::Hdr,
    core_pipeline::{
        prepass::{DepthPrepass, NormalPrepass},
        tonemapping::Tonemapping,
    },
    light::{EnvironmentMapLight, ShadowFilteringMethod},
    pbr::{ContactShadows, ScreenSpaceAmbientOcclusion},
    post_process::{
        auto_exposure::{AutoExposure, AutoExposurePlugin},
        bloom::Bloom,
    },
    prelude::*,
};
use cyrus_game::fidelity::{Area, Fidelity, Mode};

/// Lights the world with PBR while `Area::Lighting` is Modern; the scene materials switch their
/// own shading. Add after `DefaultPlugins`.
pub struct ModernLightingPlugin;

impl Plugin for ModernLightingPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((AutoExposurePlugin, PostPlugin))
            .init_resource::<DayNight>()
            .init_resource::<Daylight>()
            .init_resource::<ModernEffects>()
            .init_resource::<AutoExposureAssets>()
            // Update, so a new sun is propagated and gets its shadow cascades in the same frame:
            // Bevy builds cascades in PostUpdate, and a light without them panics in the renderer.
            .add_systems(
                Update,
                (
                    foliage::find_plants,
                    (
                        (day_night::advance, sync_lights, day_night::apply).chain(),
                        (configure_camera, effects::apply_camera_effects).chain(),
                        effects::focus,
                        effects::sun_shafts,
                        foliage::sway,
                        post::update_post,
                    )
                        .chain()
                        .run_if(lit),
                    restore.run_if(not(lit)),
                    day_night::tint_sky,
                    effects::sky_glow,
                )
                    .chain(),
            );
    }
}

fn lit(fidelity: Res<Fidelity>) -> bool {
    fidelity.mode(Area::Lighting) == Mode::Modern
}

/// A light, or the marker that lights were built, owned by this module.
#[derive(Component)]
pub(super) struct Light;

/// Camera effects that are always on. Everything is Bevy's own; a Modern tuning choice.
/// Temporal anti-aliasing is left out because the original's mesh animation rewrites vertex
/// positions in place without motion vectors. Tonemapping and the optional effects come from
/// `effects`.
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
#[allow(clippy::type_complexity)]
fn restore(
    mut commands: Commands,
    cameras: Query<
        (Entity, Option<&AutoExposure>),
        (With<ViewCamera>, With<ScreenSpaceAmbientOcclusion>),
    >,
    removed: Query<Entity, Or<(With<Light>, With<ShaftVolume>)>>,
    mut daylight: ResMut<Daylight>,
    auto_exposure: Res<AutoExposureAssets>,
    mut plants: Query<(Entity, &foliage::Sway, &mut Transform)>,
) {
    daylight.set_if_neq(Daylight(None));
    for (camera, exposure) in &cameras {
        let mut camera = commands.entity(camera);
        if !effects::remove_camera_effects(&mut camera, exposure, &auto_exposure) {
            continue;
        }
        camera
            .remove::<(
                Hdr,
                DepthPrepass,
                NormalPrepass,
                ScreenSpaceAmbientOcclusion,
                ContactShadows,
                Smaa,
                Bloom,
                ShadowFilteringMethod,
                EnvironmentMapLight,
                ModernPost,
            )>()
            .insert(Tonemapping::default());
    }
    for entity in &removed {
        commands.entity(entity).despawn();
    }
    foliage::settle(&mut commands, &mut plants);
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
        world.init_resource::<DayNight>();
        world.init_resource::<Daylight>();
        world.init_resource::<ModernEffects>();
        schedule.add_systems((
            restore,
            sync_lights,
            configure_camera,
            effects::apply_camera_effects,
            effects::focus,
            effects::sun_shafts,
            effects::sky_glow,
            foliage::find_plants,
            foliage::sway,
            post::update_post,
            day_night::advance,
            day_night::apply,
            day_night::tint_sky,
        ));
        schedule
            .initialize(&mut world)
            .expect("systems initialize without query conflicts");
    }
}
