//! The modern lighting plugin, its camera effects and teardown, and the light marker the light
//! module shares.
use super::day_night::{self, DayNight, Daylight};
use super::lights::sync_lights;
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
    post_process::bloom::Bloom,
    prelude::*,
};
use cyrus_game::fidelity::{Area, Fidelity, Mode};

/// Lights the world with PBR while `Area::Lighting` is Modern; the scene materials switch their
/// own shading. Add after `DefaultPlugins`.
pub struct ModernLightingPlugin;

impl Plugin for ModernLightingPlugin {
    fn build(&self, app: &mut App) {
        // Update, so a new sun is propagated and gets its shadow cascades in the same frame:
        // Bevy builds cascades in PostUpdate, and a light without them panics in the renderer.
        app.init_resource::<DayNight>()
            .init_resource::<Daylight>()
            .add_systems(
                Update,
                (
                    (
                        (day_night::advance, sync_lights, day_night::apply).chain(),
                        configure_camera,
                    )
                        .run_if(lit),
                    restore.run_if(not(lit)),
                    day_night::tint_sky,
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

/// Camera effects. Everything is Bevy's own; a Modern tuning choice. Temporal anti-aliasing is
/// left out because the original's mesh animation rewrites vertex positions in place without
/// motion vectors. Tonemapping is off: the lights are calibrated so a fully lit surface shows
/// its texture colour, and the sky, water and flats keep the original's colours.
#[derive(Bundle)]
struct CameraEffects {
    hdr: Hdr,
    tonemapping: Tonemapping,
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
            tonemapping: Tonemapping::None,
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
fn restore(
    mut commands: Commands,
    cameras: Query<Entity, (With<ViewCamera>, With<ScreenSpaceAmbientOcclusion>)>,
    lights: Query<Entity, With<Light>>,
    mut daylight: ResMut<Daylight>,
) {
    daylight.set_if_neq(Daylight(None));
    for camera in &cameras {
        commands
            .entity(camera)
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
            )>()
            .insert(Tonemapping::default());
    }
    for light in &lights {
        commands.entity(light).despawn();
    }
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
        schedule.add_systems((
            restore,
            sync_lights,
            configure_camera,
            day_night::advance,
            day_night::apply,
            day_night::tint_sky,
        ));
        schedule
            .initialize(&mut world)
            .expect("systems initialize without query conflicts");
    }
}
