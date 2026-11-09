//! The candle the player carries at night.
//!
//! **Modern only.** The original lights nothing around the player. While the day/night cycle
//! darkens the sky, a faint, flickering warm light follows the player, more the darker it is. The
//! colour, reach, brightness, height and flicker are Modern tuning choices, not recovered
//! behaviour. Both lighting tiers read the one `Candle`: the raster tier lights it with a point
//! light (`light`), ray tracing with an emissive sphere beside its object lights.
use super::day_night::Daylight;
use super::lights::{light_color, point_light_power};
use super::plugin::Light;
use bevy::prelude::*;
use cyrus_script::player::Player;

/// How far the candle's light reaches, in world units.
pub(in crate::fog) const REACH: f32 = 30.0;
/// The candle's colour as a 0..=255 light triple.
pub(in crate::fog) const RGB: [f32; 3] = [255.0, 170.0, 80.0];
/// Height above the player's origin the candle is held at, in world units; the player is about
/// three tall.
const HEIGHT: f32 = 4.0;
/// Darkness below which the candle stays unlit, so day leaves no stray light in the scene.
const DARK_FROM: f32 = 0.02;
/// Distance from the flame, in world units, where a facing surface gets the full candle colour;
/// small, so the candle stays faint and mostly warms the player and the ground at their feet.
const FULL_AT: f32 = 0.9;
/// How far the flicker dims the flame below its full strength, as a fraction.
const FLICKER_DEPTH: f32 = 0.25;
/// Luminance weights of linear sRGB.
const LUMINANCE: Vec3 = Vec3::new(0.2126, 0.7152, 0.0722);

/// The lit candle, if any.
#[derive(Resource, Default, Clone, Copy, PartialEq, Debug)]
pub(in crate::fog) struct Candle(pub(in crate::fog) Option<Flame>);

/// Where the candle burns and how strongly: `darkness` is 0 by day and up to 1 in the dark of
/// night, dimmed by the flicker.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(in crate::fog) struct Flame {
    pub(in crate::fog) position: Vec3,
    pub(in crate::fog) darkness: f32,
}

/// How dark the cycle's sky is: 0 at noon, towards 1 at night. Without the cycle, 0.
fn darkness(daylight: &Daylight) -> f32 {
    daylight
        .0
        .map_or(0.0, |sky| (1.0 - sky.tint.dot(LUMINANCE)).clamp(0.0, 1.0))
}

/// The flame's strength at `seconds`, `1.0 - FLICKER_DEPTH..=1.0`: a few unrelated sines, so the
/// flicker never visibly repeats.
fn flicker(seconds: f32) -> f32 {
    let wave = (seconds * 7.3).sin() * 0.5
        + (seconds * 13.1 + 1.7).sin() * 0.3
        + (seconds * 23.9 + 4.2).sin() * 0.2;
    1.0 - FLICKER_DEPTH * (0.5 + 0.5 * wave)
}

/// Lights the candle on the player once the cycle has made the sky dark.
pub(in crate::fog) fn track(
    time: Res<Time>,
    daylight: Res<Daylight>,
    players: Query<&GlobalTransform, With<Player>>,
    mut candle: ResMut<Candle>,
) {
    let darkness = darkness(&daylight);
    let flame = players
        .iter()
        .next()
        .filter(|_| darkness >= DARK_FROM)
        .map(|player| Flame {
            position: player.translation() + Vec3::Y * HEIGHT,
            darkness: darkness * flicker(time.elapsed_secs()),
        });
    candle.set_if_neq(Candle(flame));
}

/// Marks the raster tier's candle light.
#[derive(Component)]
pub(in crate::fog) struct CandleLamp;

/// The raster tier's candle: a point light that follows the flame and goes out with it.
pub(in crate::fog) fn light(
    mut commands: Commands,
    candle: Res<Candle>,
    mut lamps: Query<(Entity, &mut Transform, &mut PointLight), With<CandleLamp>>,
) {
    let Some(flame) = candle.0 else {
        for (lamp, ..) in &lamps {
            commands.entity(lamp).despawn();
        }
        return;
    };
    let intensity = point_light_power(FULL_AT) * flame.darkness;
    let mut held = false;
    for (_, mut transform, mut light) in &mut lamps {
        held = true;
        transform.translation = flame.position;
        light.intensity = intensity;
    }
    if !held {
        commands.spawn((
            Light,
            CandleLamp,
            PointLight {
                color: light_color(RGB),
                intensity,
                range: REACH,
                shadow_maps_enabled: false,
                ..default()
            },
            Transform::from_translation(flame.position),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::super::day_night::SkyLight;
    use super::*;

    fn dark_sky(tint: Vec3) -> Daylight {
        Daylight(Some(SkyLight {
            tint,
            sun_rotation: Quat::IDENTITY,
            sun_disc: 0.0,
        }))
    }

    #[test]
    fn the_flicker_only_dims_the_flame_a_little() {
        let samples: Vec<f32> = (0..2000).map(|step| flicker(step as f32 * 0.01)).collect();
        let (low, high) = samples
            .iter()
            .fold((f32::MAX, f32::MIN), |(low, high), &value| {
                (low.min(value), high.max(value))
            });
        assert!(low >= 1.0 - FLICKER_DEPTH - 1e-6 && high <= 1.0 + 1e-6);
        assert!(high - low > FLICKER_DEPTH * 0.5, "it visibly flickers");
    }

    #[test]
    fn the_candle_burns_only_in_the_dark() {
        assert_eq!(darkness(&Daylight(None)), 0.0, "no cycle, no candle");
        assert!(darkness(&dark_sky(Vec3::ONE)) < DARK_FROM, "noon");
        assert!(
            darkness(&dark_sky(Vec3::new(0.03, 0.04, 0.09))) > 0.9,
            "night"
        );
    }
}
