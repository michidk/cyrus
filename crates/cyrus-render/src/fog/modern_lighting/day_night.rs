//! A day/night cycle for the modern lighting.
//!
//! **Modern only.** The original has one fixed sun per world, and night is a separate world
//! variant. While the cycle is on and `Area::Lighting` is Modern, the hour of the day moves
//! the sun along an arc whose noon is the world's installed sun, and it scales the sun, the
//! ambient light, the fog colour and the sky. Below the horizon a dim moonlight replaces the
//! sun. Worlds without an installed sun, such as interiors, are left alone. Every colour and
//! the arc's orientation are Modern tuning choices, not recovered behaviour.
use super::lights::{Ambient, Sun, ambient_texels, light_color, light_transform};
use crate::sky::SkyMaterial;
use bevy::prelude::*;
use std::f32::consts::TAU;

/// The cycle's settings, edited from the debug menu's day/night panel and the control port.
#[derive(Resource, Debug, Clone, PartialEq)]
pub struct DayNight {
    /// Off keeps the world's installed lighting, also in Modern.
    pub enabled: bool,
    /// Whether the hour advances on its own.
    pub running: bool,
    /// Hour of the day, `0.0..24.0`; 12 is the world's installed sun.
    pub hour: f32,
    /// In-game minutes that pass per real second while running.
    pub minutes_per_second: f32,
}

impl Default for DayNight {
    fn default() -> Self {
        Self {
            enabled: false,
            running: true,
            hour: 12.0,
            minutes_per_second: 30.0,
        }
    }
}

impl DayNight {
    /// The hour as `HH:MM`.
    pub fn clock(&self) -> String {
        let minutes = (self.hour.rem_euclid(24.0) * 60.0) as u32;
        format!("{:02}:{:02}", minutes / 60, minutes % 60)
    }
}

/// What the running cycle does to the fog and sky; `None` while it changes nothing.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq)]
pub struct Daylight(pub Option<SkyLight>);

/// The cycle's effect on the fog and sky.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SkyLight {
    /// Factor on the encoded fog and sky colours, quantised to 8-bit steps so the scene
    /// materials are only rewritten when the colour visibly changes.
    pub tint: Vec3,
    /// Rotation from the installed noon sun to the current one, for the sky's sun disc.
    pub sun_rotation: Quat,
    /// Visibility of the sky's sun disc, `0.0..=1.0`.
    pub sun_disc: f32,
}

const WHITE: Vec3 = Vec3::ONE;
const SUNSET_LIGHT: Vec3 = Vec3::new(1.0, 0.55, 0.32);
const SUNSET_AMBIENT: Vec3 = Vec3::new(1.0, 0.75, 0.6);
const SUNSET_SKY: Vec3 = Vec3::new(1.0, 0.68, 0.52);
const MOONLIGHT: Vec3 = Vec3::new(0.12, 0.15, 0.24);
const NIGHT_AMBIENT: Vec3 = Vec3::new(0.2, 0.25, 0.42);
const NIGHT_SKY: Vec3 = Vec3::new(0.1, 0.13, 0.27);

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The lighting at one hour, as factors on the world's installed values.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Phase {
    /// Unit vector towards the sun.
    sun_position: Vec3,
    /// The direction the sun or moon light travels.
    light_direction: Dir3,
    /// Factor on the installed sun colour.
    light: Vec3,
    /// Factor on the installed ambient colour.
    ambient: Vec3,
    sky: SkyLight,
}

/// The phase at `hour` for a world whose installed sun shines along `noon_direction`.
///
/// The sun moves on a great circle through the installed sun at noon and the horizon at 6
/// and 18 o'clock, perpendicular to the noon sun's heading.
fn phase(noon_direction: Dir3, hour: f32) -> Phase {
    let noon = -*noon_direction;
    let heading = Vec3::new(noon.x, 0.0, noon.z)
        .try_normalize()
        .unwrap_or(Vec3::X);
    let across = heading.cross(Vec3::Y);
    let angle = (hour - 12.0) / 24.0 * TAU;
    let sun_position = (noon * angle.cos() + across * angle.sin()).normalize();
    let height = sun_position.y;
    let day = smoothstep(-0.05, 0.15, height);
    // Warm light while the sun is low.
    let low = 1.0 - smoothstep(0.0, 0.35, height);
    let (light_direction, light) = if height >= 0.0 {
        (
            -sun_position,
            WHITE.lerp(SUNSET_LIGHT, low) * smoothstep(-0.02, 0.08, height),
        )
    } else {
        // The moon stands opposite the sun.
        (sun_position, MOONLIGHT * smoothstep(0.0, 0.15, -height))
    };
    let warm = low * day;
    let sky = NIGHT_SKY.lerp(WHITE, day) * WHITE.lerp(SUNSET_SKY, warm * 0.6);
    Phase {
        sun_position,
        light_direction: Dir3::new(light_direction).unwrap_or(noon_direction),
        light,
        ambient: NIGHT_AMBIENT.lerp(WHITE, day) * WHITE.lerp(SUNSET_AMBIENT, warm * 0.5),
        sky: SkyLight {
            tint: (sky * 255.0).round() / 255.0,
            sun_rotation: Quat::from_rotation_arc(noon, sun_position),
            sun_disc: smoothstep(-0.05, 0.05, height),
        },
    }
}

/// Advances the hour while the cycle runs.
pub(super) fn advance(time: Res<Time>, mut cycle: ResMut<DayNight>) {
    if cycle.enabled && cycle.running && cycle.minutes_per_second != 0.0 {
        cycle.hour =
            (cycle.hour + time.delta_secs() * cycle.minutes_per_second / 60.0).rem_euclid(24.0);
    }
}

/// Moves and tints the sun and the ambient light to the hour, or back to the installed values.
pub(super) fn apply(
    cycle: Res<DayNight>,
    mut suns: Query<(&Sun, &mut DirectionalLight, &mut Transform)>,
    ambients: Query<&Ambient>,
    mut images: ResMut<Assets<Image>>,
    mut daylight: ResMut<Daylight>,
) {
    let phase = suns
        .iter()
        .next()
        .filter(|_| cycle.enabled)
        .map(|(sun, ..)| phase(sun.direction, cycle.hour));
    for (sun, mut light, mut transform) in &mut suns {
        let (rgb, direction) = match phase {
            Some(phase) => (
                (Vec3::from(sun.rgb) * phase.light).to_array(),
                phase.light_direction,
            ),
            None => (sun.rgb, sun.direction),
        };
        let color = light_color(rgb);
        if light.color != color {
            light.color = color;
        }
        transform.set_if_neq(light_transform(direction));
    }
    for ambient in &ambients {
        let rgb = phase.map_or(ambient.rgb, |phase| {
            (Vec3::from(ambient.rgb) * phase.ambient).to_array()
        });
        let texels = ambient_texels(rgb);
        if images
            .get(&ambient.image)
            .is_some_and(|image| image.data.as_deref() != Some(texels.as_slice()))
            && let Some(mut image) = images.get_mut(&ambient.image)
        {
            image.data = Some(texels);
        }
    }
    daylight.set_if_neq(Daylight(phase.map(|phase| phase.sky)));
}

/// Gives the sky the cycle's tint and sun position; neutral values leave it as installed.
pub(super) fn tint_sky(
    daylight: Res<Daylight>,
    skies: Query<&MeshMaterial3d<SkyMaterial>>,
    mut materials: ResMut<Assets<SkyMaterial>>,
) {
    let (tint, rotation) = daylight.0.map_or((Vec4::ONE, Vec4::W), |sky| {
        (
            sky.tint.extend(sky.sun_disc),
            Vec4::from_array(sky.sun_rotation.to_array()),
        )
    });
    for handle in &skies {
        if materials
            .get(&handle.0)
            .is_some_and(|sky| sky.day_night != tint || sky.day_night_sun != rotation)
            && let Some(mut sky) = materials.get_mut(&handle.0)
        {
            sky.day_night = tint;
            sky.day_night_sun = rotation;
        }
    }
}

/// Gives the background beyond the scenery the cycle's fog tint, so the far edge of the world darkens
/// with the sky; without the cycle it is the installed background colour.
pub(super) fn tint_clear_color(
    daylight: Res<Daylight>,
    request: Res<crate::environment::EnvironmentRequest>,
    environments: Res<Assets<cyrus_assets::environment::Environment>>,
    mut clear: ResMut<ClearColor>,
) {
    // Without the cycle the world's own background stays, except once after the cycle stops.
    if daylight.0.is_none() && !daylight.is_changed() {
        return;
    }
    let Some(environment) = environments.get(&request.0) else {
        return;
    };
    let tint = daylight.0.map_or(Vec3::ONE, |sky| sky.tint);
    let [r, g, b] = environment.background.map(|value| f32::from(value) / 255.0);
    let color = Color::srgb(r * tint.x, g * tint.y, b * tint.z);
    if clear.0 != color {
        clear.0 = color;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn installed() -> Dir3 {
        Dir3::new(Vec3::new(0.3, -0.8, 0.5)).unwrap()
    }

    #[test]
    fn noon_reproduces_the_installed_sun() {
        let noon = phase(installed(), 12.0);
        assert!(noon.light_direction.angle_between(*installed()) < 1e-3);
        assert!(noon.light.abs_diff_eq(Vec3::ONE, 1e-4));
        assert!(noon.ambient.abs_diff_eq(Vec3::ONE, 1e-4));
        assert_eq!(noon.sky.tint, Vec3::ONE);
        assert_eq!(noon.sky.sun_disc, 1.0);
        assert!(noon.sky.sun_rotation.abs_diff_eq(Quat::IDENTITY, 1e-4));
    }

    #[test]
    fn the_sun_sets_at_six_and_midnight_is_moonlit_and_dark() {
        for hour in [6.0, 18.0] {
            assert!(
                phase(installed(), hour).sun_position.y.abs() < 1e-4,
                "{hour}"
            );
        }
        let night = phase(installed(), 0.0);
        assert!(night.sun_position.y < 0.0);
        assert!(night.light_direction.y < 0.0, "moonlight shines down");
        assert!(night.light.max_element() < 0.3);
        assert!(night.ambient.max_element() < 0.5);
        assert_eq!(night.sky.sun_disc, 0.0);
        let dusk = phase(installed(), 17.5);
        assert!(dusk.light.x > dusk.light.z, "a low sun is warm");
    }

    #[test]
    fn clock_formats_the_hour() {
        let cycle = DayNight {
            hour: 18.5,
            ..default()
        };
        assert_eq!(cycle.clock(), "18:30");
    }
}
