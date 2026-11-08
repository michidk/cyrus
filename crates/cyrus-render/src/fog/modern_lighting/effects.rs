//! The modern lighting's optional effects, switched from the debug menu's effects panel and the
//! control port.
//!
//! **Modern only.** None of these exist in the original, and every value here is a Modern tuning
//! choice. They run only while `Area::Lighting` is Modern: tonemapping with colour grading,
//! automatic exposure, sun shafts through Bevy's volumetric fog, depth of field and motion blur
//! on the camera; the sky glow and stars (`sky.wesl`), water reflections and the lens flare
//! (`post`), and swaying plants (`foliage`).
use super::lights::Sun;
use crate::{camera::ViewCamera, sky::SkyMaterial};
use bevy::{
    asset::RenderAssetUsages,
    core_pipeline::{prepass::MotionVectorPrepass, tonemapping::Tonemapping},
    curve::cubic_splines::LinearSpline,
    light::{FogVolume, VolumetricFog, VolumetricLight},
    post_process::{
        auto_exposure::{AutoExposure, AutoExposureCompensationCurve},
        dof::{DepthOfField, DepthOfFieldMode},
        motion_blur::MotionBlur,
    },
    prelude::*,
    render::{
        render_resource::{Extent3d, TextureDimension, TextureFormat},
        view::{ColorGrading, ColorGradingGlobal, ColorGradingSection},
    },
};
use cyrus_game::fidelity::{Area, Fidelity, Mode};
use cyrus_script::player::Player;
use serde::{Deserialize, Serialize};

/// The effects and their settings.
#[derive(Resource, Debug, Clone, PartialEq, Serialize)]
pub struct ModernEffects {
    /// Tonemapping and colour grading; off keeps the calibrated colours untouched.
    pub grading: bool,
    pub tonemapper: Tonemapper,
    /// Exposure compensation in stops.
    pub exposure: f32,
    /// Saturation after tonemapping; 1 keeps it.
    pub saturation: f32,
    /// Contrast; 1 keeps it.
    pub contrast: f32,
    /// Adapts the exposure to the brightness on screen, within a few stops.
    pub auto_exposure: bool,
    /// Sun shafts through a thin volumetric haze around the camera.
    pub god_rays: bool,
    /// Density of that haze.
    pub god_ray_density: f32,
    /// Glare, streak and ghosts while the sun is on screen and not covered.
    pub lens_flare: bool,
    /// Glow around the sun, a warm horizon while it is low, and stars at night.
    pub sky: bool,
    /// Screen-space reflections on water.
    pub water_reflections: bool,
    /// Palm trees and plants sway in the wind.
    pub foliage: bool,
    /// Blurs what is far nearer and farther than the player.
    pub depth_of_field: bool,
    /// Lens aperture in f-stops; lower blurs more.
    pub aperture: f32,
    /// Blurs along the camera's and objects' motion.
    pub motion_blur: bool,
}

impl Default for ModernEffects {
    fn default() -> Self {
        Self {
            grading: true,
            tonemapper: Tonemapper::default(),
            exposure: 0.0,
            saturation: 1.05,
            contrast: 1.0,
            auto_exposure: true,
            god_rays: true,
            god_ray_density: 0.002,
            lens_flare: true,
            sky: true,
            water_reflections: true,
            foliage: true,
            // Both soften gameplay views, so they are opt-in.
            depth_of_field: false,
            aperture: 0.12,
            motion_blur: false,
        }
    }
}

/// One switchable effect, as the control port names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Effect {
    Grading,
    AutoExposure,
    GodRays,
    LensFlare,
    Sky,
    WaterReflections,
    Foliage,
    DepthOfField,
    MotionBlur,
}

impl Effect {
    pub const ALL: [Self; 9] = [
        Self::Grading,
        Self::AutoExposure,
        Self::GodRays,
        Self::LensFlare,
        Self::Sky,
        Self::WaterReflections,
        Self::Foliage,
        Self::DepthOfField,
        Self::MotionBlur,
    ];

    /// The panel's label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Grading => "Tonemapping and colour grading",
            Self::AutoExposure => "Auto exposure",
            Self::GodRays => "God rays",
            Self::LensFlare => "Lens flare and sun glare",
            Self::Sky => "Sky glow and stars",
            Self::WaterReflections => "Water reflections",
            Self::Foliage => "Swaying plants",
            Self::DepthOfField => "Depth of field",
            Self::MotionBlur => "Motion blur",
        }
    }
}

impl ModernEffects {
    /// The switch of one effect.
    pub fn switch(&mut self, effect: Effect) -> &mut bool {
        match effect {
            Effect::Grading => &mut self.grading,
            Effect::AutoExposure => &mut self.auto_exposure,
            Effect::GodRays => &mut self.god_rays,
            Effect::LensFlare => &mut self.lens_flare,
            Effect::Sky => &mut self.sky,
            Effect::WaterReflections => &mut self.water_reflections,
            Effect::Foliage => &mut self.foliage,
            Effect::DepthOfField => &mut self.depth_of_field,
            Effect::MotionBlur => &mut self.motion_blur,
        }
    }
}

/// The tonemapping curves offered; Bevy's names.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tonemapper {
    /// Keeps hue and saturation of everything below the highlights, so the calibrated texture
    /// colours survive.
    #[default]
    PbrNeutral,
    TonyMcMapface,
    AgX,
    AcesFitted,
    BlenderFilmic,
}

impl Tonemapper {
    pub const ALL: [Self; 5] = [
        Self::PbrNeutral,
        Self::TonyMcMapface,
        Self::AgX,
        Self::AcesFitted,
        Self::BlenderFilmic,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::PbrNeutral => "Khronos PBR Neutral",
            Self::TonyMcMapface => "Tony McMapface",
            Self::AgX => "AgX",
            Self::AcesFitted => "ACES",
            Self::BlenderFilmic => "Blender Filmic",
        }
    }
}

impl From<Tonemapper> for Tonemapping {
    fn from(tonemapper: Tonemapper) -> Self {
        match tonemapper {
            Tonemapper::PbrNeutral => Self::KhronosPbrNeutral,
            Tonemapper::TonyMcMapface => Self::TonyMcMapface,
            Tonemapper::AgX => Self::AgX,
            Tonemapper::AcesFitted => Self::AcesFitted,
            Tonemapper::BlenderFilmic => Self::BlenderFilmic,
        }
    }
}

/// Average log2 luminance the automatic exposure keeps unchanged; brighter or darker screens
/// adapt towards it.
const EXPOSURE_TARGET: f32 = -2.5;
/// How far, in stops, the automatic exposure darkens a bright screen and brightens a dark one.
const EXPOSURE_LIMITS: [f32; 2] = [-1.0, 1.5];
/// The log2 luminance range the automatic exposure meters.
const METERED: [f32; 2] = [-12.0, 4.0];

/// The automatic exposure's compensation curves and its blind metering mask.
///
/// Bevy 0.20 keeps applying a camera's automatic exposure after the component is removed, so
/// it is switched off before it is removed: the camera then meters through a black mask.
/// Nothing reaches the histogram, Bevy reads the lowest metered luminance, and the identity
/// curve maps that to no change; changing the settings also resets the adapted exposure to
/// none. Modern lighting keeps the switched-off exposure on its camera, and restoring Faithful
/// removes it, with the high dynamic range it requires, one frame later.
#[derive(Resource)]
pub(super) struct AutoExposureAssets {
    /// Bevy's exposure moves the screen's average log luminance to this curve's value, which
    /// keeps the change within `EXPOSURE_LIMITS` of the `EXPOSURE_TARGET`.
    adaptive: Handle<AutoExposureCompensationCurve>,
    identity: Handle<AutoExposureCompensationCurve>,
    blind: Handle<Image>,
}

impl FromWorld for AutoExposureAssets {
    fn from_world(world: &mut World) -> Self {
        let [low, high] = METERED;
        let [darken, brighten] = EXPOSURE_LIMITS;
        let adaptive = AutoExposureCompensationCurve::from_curve(LinearSpline::new([
            vec2(low, low + brighten),
            vec2(EXPOSURE_TARGET - brighten, EXPOSURE_TARGET),
            vec2(EXPOSURE_TARGET - darken, EXPOSURE_TARGET),
            vec2(high, high + darken),
        ]))
        .expect("the exposure curve is continuous and increasing");
        let identity = AutoExposureCompensationCurve::from_curve(LinearSpline::new([
            vec2(low, low),
            vec2(high, high),
        ]))
        .expect("the identity curve is continuous and increasing");
        let mut curves = world.resource_mut::<Assets<AutoExposureCompensationCurve>>();
        let (adaptive, identity) = (curves.add(adaptive), curves.add(identity));
        let blind = world.resource_mut::<Assets<Image>>().add(Image::new_fill(
            Extent3d::default(),
            TextureDimension::D2,
            &[0, 0, 0, 255],
            TextureFormat::Rgba8Unorm,
            RenderAssetUsages::RENDER_WORLD,
        ));
        Self {
            adaptive,
            identity,
            blind,
        }
    }
}

impl AutoExposureAssets {
    /// The camera's automatic exposure, adapting or switched off.
    fn auto_exposure(&self, adapting: bool) -> AutoExposure {
        let [low, high] = METERED;
        let exposure = AutoExposure {
            range: low..=high,
            speed_brighten: 1.0,
            speed_darken: 1.5,
            ..default()
        };
        if adapting {
            AutoExposure {
                compensation_curve: self.adaptive.clone(),
                ..exposure
            }
        } else {
            AutoExposure {
                compensation_curve: self.identity.clone(),
                metering_mask: self.blind.clone(),
                ..exposure
            }
        }
    }
}

/// The effects a camera was last given.
#[derive(Component)]
pub(super) struct Applied(ModernEffects);

fn differs<T: PartialEq>(
    old: Option<&ModernEffects>,
    new: &ModernEffects,
    key: impl Fn(&ModernEffects) -> T,
) -> bool {
    old.is_none_or(|old| key(old) != key(new))
}

/// Gives the view camera the effects' Bevy components, touching only those whose settings
/// changed, so adjusting one does not restart another's adaptation.
pub(super) fn apply_camera_effects(
    mut commands: Commands,
    effects: Res<ModernEffects>,
    auto_exposure: Res<AutoExposureAssets>,
    cameras: Query<(Entity, Option<&Applied>), With<ViewCamera>>,
) {
    let new = effects.as_ref();
    for (camera, applied) in &cameras {
        let old = applied.map(|applied| &applied.0);
        if old == Some(new) {
            continue;
        }
        let mut camera = commands.entity(camera);
        if differs(old, new, |e| {
            (
                e.grading,
                e.auto_exposure,
                e.tonemapper,
                e.exposure,
                e.saturation,
                e.contrast,
            )
        }) {
            // Automatic exposure is applied in the tonemapping pass, which needs a curve.
            let (tonemapping, grading) = match (new.grading, new.auto_exposure) {
                (true, _) => (
                    Tonemapping::from(new.tonemapper),
                    ColorGrading::with_identical_sections(
                        ColorGradingGlobal {
                            exposure: new.exposure,
                            post_saturation: new.saturation,
                            ..default()
                        },
                        ColorGradingSection {
                            contrast: new.contrast,
                            ..default()
                        },
                    ),
                ),
                (false, true) => (Tonemapping::Linear, ColorGrading::default()),
                (false, false) => (Tonemapping::None, ColorGrading::default()),
            };
            camera.insert((tonemapping, grading));
        }
        if differs(old, new, |e| e.auto_exposure) {
            camera.insert(auto_exposure.auto_exposure(new.auto_exposure));
        }
        if differs(old, new, |e| e.god_rays) {
            if new.god_rays {
                camera.insert(VolumetricFog {
                    // The environment light already brightens the scene; only the sun scatters.
                    ambient_intensity: 0.0,
                    step_count: 48,
                    ..default()
                });
            } else {
                camera.remove::<VolumetricFog>();
            }
        }
        if differs(old, new, |e| (e.depth_of_field, e.aperture)) {
            if new.depth_of_field {
                camera.insert(DepthOfField {
                    mode: DepthOfFieldMode::Gaussian,
                    aperture_f_stops: new.aperture,
                    max_circle_of_confusion_diameter: 24.0,
                    ..default()
                });
            } else {
                camera.remove::<DepthOfField>();
            }
        }
        if differs(old, new, |e| e.motion_blur) {
            if new.motion_blur {
                camera.insert(MotionBlur {
                    shutter_angle: 0.5,
                    samples: 2,
                });
            } else {
                camera.remove::<(MotionBlur, MotionVectorPrepass)>();
            }
        }
        camera.insert(Applied(new.clone()));
    }
}

/// Removes every component `apply_camera_effects` added. While the automatic exposure still
/// adapts it only switches it off and returns false: Bevy must run one frame with it off, so
/// the exposure it keeps applying after removal is none.
pub(super) fn remove_camera_effects(
    camera: &mut EntityCommands,
    exposure: Option<&AutoExposure>,
    auto_exposure: &AutoExposureAssets,
) -> bool {
    if exposure.is_some_and(|exposure| exposure.metering_mask != auto_exposure.blind) {
        camera.insert(auto_exposure.auto_exposure(false));
        return false;
    }
    camera
        .remove::<(
            Applied,
            AutoExposure,
            VolumetricFog,
            DepthOfField,
            MotionBlur,
            MotionVectorPrepass,
        )>()
        .insert(ColorGrading::default());
    true
}

/// Distance the depth of field focuses at without a player, in world units.
const DEFAULT_FOCUS: f32 = 40.0;
/// Height above the player's origin the depth of field focuses at, in world units.
const FOCUS_HEIGHT: f32 = 3.0;

/// Focuses the depth of field on the player.
pub(super) fn focus(
    mut cameras: Query<(&GlobalTransform, &mut DepthOfField), With<ViewCamera>>,
    players: Query<&GlobalTransform, With<Player>>,
) {
    for (camera, mut depth_of_field) in &mut cameras {
        let distance = players.iter().next().map_or(DEFAULT_FOCUS, |player| {
            camera
                .translation()
                .distance(player.translation() + Vec3::Y * FOCUS_HEIGHT)
        });
        let distance = distance.max(1.0);
        if depth_of_field.focal_distance != distance {
            depth_of_field.focal_distance = distance;
        }
    }
}

/// The haze the god rays scatter in; it follows the camera.
#[derive(Component)]
pub(super) struct ShaftVolume;

/// Size of the haze around the camera, in world units; the sun's shadow cascades reach 400.
const SHAFT_VOLUME: Vec3 = Vec3::new(800.0, 400.0, 800.0);

/// Keeps the god rays' haze around the camera and the sun lighting it, while they are on.
pub(super) fn sun_shafts(
    mut commands: Commands,
    effects: Res<ModernEffects>,
    cameras: Query<&GlobalTransform, With<ViewCamera>>,
    mut volumes: Query<(Entity, &mut Transform, &mut FogVolume), With<ShaftVolume>>,
    suns: Query<(Entity, Has<VolumetricLight>), With<Sun>>,
) {
    for (sun, lit) in &suns {
        match (effects.god_rays, lit) {
            (true, false) => {
                commands.entity(sun).insert(VolumetricLight);
            }
            (false, true) => {
                commands.entity(sun).remove::<VolumetricLight>();
            }
            _ => {}
        }
    }
    let camera = cameras.iter().next().map(GlobalTransform::translation);
    let (Some(camera), true) = (camera, effects.god_rays) else {
        for (volume, ..) in &volumes {
            commands.entity(volume).despawn();
        }
        return;
    };
    let transform = Transform::from_translation(camera).with_scale(SHAFT_VOLUME);
    let fog = FogVolume {
        density_factor: effects.god_ray_density,
        absorption: 0.02,
        scattering: 0.3,
        scattering_asymmetry: 0.7,
        ..default()
    };
    match volumes.single_mut() {
        Ok((_, mut placed, mut volume)) => {
            placed.set_if_neq(transform);
            if volume.density_factor != fog.density_factor {
                volume.density_factor = fog.density_factor;
            }
        }
        Err(_) => {
            for (volume, ..) in &volumes {
                commands.entity(volume).despawn();
            }
            commands.spawn((ShaftVolume, fog, transform));
        }
    }
}

/// Turns the sky glow and stars on and off and points the glow at the sun light; it runs in
/// every mode, so leaving modern lighting clears it.
pub(super) fn sky_glow(
    effects: Res<ModernEffects>,
    fidelity: Res<Fidelity>,
    suns: Query<&GlobalTransform, With<Sun>>,
    skies: Query<&MeshMaterial3d<SkyMaterial>>,
    mut materials: ResMut<Assets<SkyMaterial>>,
) {
    let lit = fidelity.mode(Area::Lighting) == Mode::Modern;
    let value = if lit && effects.sky {
        suns.iter()
            .next()
            .map_or(Vec3::ZERO, |sun| -sun.forward().as_vec3())
            .extend(1.0)
    } else {
        Vec4::ZERO
    };
    for handle in &skies {
        if materials
            .get(&handle.0)
            .is_some_and(|sky| sky.modern_sky != value)
            && let Some(mut sky) = materials.get_mut(&handle.0)
        {
            sky.modern_sky = value;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_effect_has_its_own_switch() {
        let mut effects = ModernEffects::default();
        for effect in Effect::ALL {
            *effects.switch(effect) = false;
        }
        assert!(
            !effects.grading
                && !effects.auto_exposure
                && !effects.god_rays
                && !effects.lens_flare
                && !effects.sky
                && !effects.water_reflections
                && !effects.foliage
                && !effects.depth_of_field
                && !effects.motion_blur
        );
    }

    #[test]
    fn effects_parse_from_their_control_names() {
        for (name, effect) in [
            ("god_rays", Effect::GodRays),
            ("water_reflections", Effect::WaterReflections),
            ("motion_blur", Effect::MotionBlur),
        ] {
            assert_eq!(
                serde_json::from_value::<Effect>(serde_json::json!(name)).unwrap(),
                effect
            );
        }
    }

    #[test]
    fn exposure_curves_build() {
        let mut world = World::new();
        world.init_resource::<Assets<AutoExposureCompensationCurve>>();
        world.init_resource::<Assets<Image>>();
        let assets = AutoExposureAssets::from_world(&mut world);
        let curves = world.resource::<Assets<AutoExposureCompensationCurve>>();
        assert!(curves.contains(&assets.adaptive) && curves.contains(&assets.identity));
    }
}
