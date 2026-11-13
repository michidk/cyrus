//! The modern lighting plugin, its camera effects and teardown, and the light marker the light
//! module shares.
use super::day_night::{self, DayNight, Daylight};
use super::effects::{self, AutoExposureAssets, ModernEffects, ShaftVolume};
use super::foliage;
use super::lights::sync_lights;
use super::post::{self, ModernPost, PostPlugin};
use super::shadows;
use super::sharing::{self, SharedMaterials};
use crate::camera::ViewCamera;
use bevy::{
    camera::Hdr,
    core_pipeline::{
        prepass::{DepthPrepass, NormalPrepass},
        tonemapping::Tonemapping,
    },
    light::{EnvironmentMapLight, ShadowFilteringMethod},
    post_process::auto_exposure::{AutoExposure, AutoExposurePlugin},
    prelude::*,
};
use cyrus_game::fidelity::{Area, Fidelity, Mode};

/// Lights the world with PBR while `Area::Lighting` is Modern; the scene materials switch their
/// own shading. Add after `DefaultPlugins`.
pub struct ModernLightingPlugin;

impl Plugin for ModernLightingPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            AutoExposurePlugin,
            PostPlugin,
            MaterialPlugin::<shadows::ShadowMaterial>::default(),
        ))
        .init_resource::<DayNight>()
        .init_resource::<Daylight>()
        .init_resource::<super::candle::Candle>()
        .init_resource::<ModernEffects>()
        .init_resource::<AutoExposureAssets>()
        .init_resource::<shadows::TwinMaterial>()
        .init_resource::<SharedMaterials>()
        .init_resource::<super::glow::LampLights>()
        // Update, so a new sun is propagated and gets its shadow cascades in the same frame:
        // Bevy builds cascades in PostUpdate, and a light without them panics in the renderer.
        .add_systems(
            Update,
            (
                foliage::find_plants,
                (
                    (
                        day_night::advance,
                        sync_lights,
                        day_night::apply,
                        super::candle::track,
                        super::candle::light,
                    )
                        .chain(),
                    (configure_camera, effects::apply_camera_effects).chain(),
                    effects::switch_lights,
                    effects::focus,
                    effects::sun_shafts,
                    foliage::sway,
                    post::update_post,
                    shadows::add_twins,
                    super::glow::find_lamps,
                    sharing::share,
                )
                    .chain()
                    .run_if(lit),
                (restore, super::glow::restore).chain().run_if(not(lit)),
                day_night::tint_sky,
                day_night::tint_clear_color,
                effects::sky_glow,
            )
                .chain(),
        )
        .add_systems(
            PostUpdate,
            shadows::follow
                .after(bevy::transform::TransformSystems::Propagate)
                .run_if(lit),
        );
    }
}

fn lit(fidelity: Res<Fidelity>) -> bool {
    fidelity.mode(Area::Lighting) == Mode::Modern
}

/// A light, or the marker that lights were built, owned by this module.
#[derive(Component)]
pub(super) struct Light;

/// What every modern-lit camera needs. Everything is Bevy's own; a Modern tuning choice.
/// Ambient occlusion, contact shadows, anti-aliasing (SMAA; temporal anti-aliasing is left out
/// because the original's mesh animation rewrites vertex positions in place without motion
/// vectors), bloom, tonemapping and the optional effects come from `effects`.
#[derive(Bundle)]
struct CameraEffects {
    hdr: Hdr,
    // Screen-space ambient occlusion and contact shadows need depth, and the former normals.
    depth: DepthPrepass,
    normals: NormalPrepass,
    // Also marks the camera as configured for modern lighting.
    shadow_filtering: ShadowFilteringMethod,
}

fn configure_camera(
    mut commands: Commands,
    cameras: Query<Entity, (With<ViewCamera>, Without<ShadowFilteringMethod>)>,
) {
    for camera in &cameras {
        commands.entity(camera).insert(CameraEffects {
            hdr: Hdr,
            depth: DepthPrepass,
            normals: NormalPrepass,
            shadow_filtering: ShadowFilteringMethod::Gaussian,
        });
    }
}

/// Everything `restore` puts back: the modern cameras, the lights and volumes it spawned, the
/// day/night and candle state, swaying plants, shadow twins and shared materials.
#[allow(clippy::type_complexity)]
#[derive(bevy::ecs::system::SystemParam)]
struct Modernized<'w, 's> {
    cameras: Query<
        'w,
        's,
        (Entity, Option<&'static AutoExposure>),
        (With<ViewCamera>, With<ShadowFilteringMethod>),
    >,
    removed: Query<
        'w,
        's,
        Entity,
        Or<(
            With<Light>,
            With<ShaftVolume>,
            With<super::candle::CandleLamp>,
        )>,
    >,
    daylight: ResMut<'w, Daylight>,
    candle: ResMut<'w, super::candle::Candle>,
    auto_exposure: Res<'w, AutoExposureAssets>,
    plants: foliage::Swaying<'w, 's>,
    twins: Query<'w, 's, Entity, With<shadows::ShadowTwin>>,
    twinned: Query<'w, 's, Entity, With<shadows::Twinned>>,
    shared: ResMut<'w, SharedMaterials>,
    scene_materials: ResMut<'w, Assets<super::super::SceneMaterial>>,
    shared_models: Query<
        'w,
        's,
        (
            Entity,
            &'static mut MeshMaterial3d<super::super::SceneMaterial>,
            &'static sharing::Unshared,
        ),
    >,
    kept: Query<'w, 's, Entity, With<sharing::KeepsOwn>>,
}

/// Undoes everything this module changed once modern lighting is off.
fn restore(mut commands: Commands, modernized: Modernized) {
    let Modernized {
        cameras,
        removed,
        mut daylight,
        mut candle,
        auto_exposure,
        mut plants,
        twins,
        twinned,
        mut shared,
        mut scene_materials,
        mut shared_models,
        kept,
    } = modernized;
    daylight.set_if_neq(Daylight(None));
    candle.set_if_neq(super::candle::Candle(None));
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
    shadows::remove_twins(&mut commands, &twins, &twinned);
    sharing::unshare(
        &mut commands,
        &mut shared,
        &mut scene_materials,
        &mut shared_models,
        &kept,
    );
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
        world.init_resource::<super::super::candle::Candle>();
        world.init_resource::<ModernEffects>();
        world.init_resource::<SharedMaterials>();
        schedule.add_systems((
            restore,
            sync_lights,
            configure_camera,
            effects::apply_camera_effects,
            effects::switch_lights,
            effects::focus,
            effects::sun_shafts,
            effects::sky_glow,
            foliage::find_plants,
            foliage::sway,
            post::update_post,
            shadows::add_twins,
            shadows::follow,
            sharing::share,
            super::super::glow::find_lamps,
            super::super::glow::restore,
            day_night::advance,
            day_night::apply,
            day_night::tint_sky,
            (super::super::candle::track, super::super::candle::light),
        ));
        schedule
            .initialize(&mut world)
            .expect("systems initialize without query conflicts");
    }
}
