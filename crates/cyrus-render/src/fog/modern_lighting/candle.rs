//! The candle the player carries at night: a faint, flickering warm light that follows the player.
//!
//! **Modern only.** The original lights nothing around the player; its HUD candle shows health.
//! The candle belongs to the day/night cycle: it burns only while the cycle runs and its hour is
//! night, in outdoor worlds, those with a sky, and fades with the cycle's daylight. Its colour,
//! reach, brightness and flicker are Modern tuning choices, kept subtle.
use super::day_night::{self, DayNight};
use super::lights::{Sun, point_light_power};
use super::plugin::Light;
use bevy::prelude::*;
use cyrus_script::player::Player;

/// Warm candle flame colour, linear light factors.
const CANDLE_COLOR: Color = Color::srgb(1.0, 0.78, 0.55);
/// Distance from the flame, in world units, where a facing surface gets the full candle colour;
/// small, so the candle only warms the player and the ground at their feet.
const CANDLE_FULL_AT: f32 = 0.9;
/// Distance the candle reaches, in world units.
const CANDLE_RANGE: f32 = 10.0;
/// Height of the light above the player's origin at the feet, in world units: just above the head,
/// so it lights the player and the ground around them rather than shining from inside the body.
const CANDLE_HEIGHT: f32 = 4.5;
/// How far the flicker dims the flame below its full strength, as a fraction.
const FLICKER_DEPTH: f32 = 0.25;

/// The player's candle; `outdoor` while the world has a sky.
#[derive(Component)]
pub(super) struct Candle {
    outdoor: bool,
}

/// Spawns the candle, out (zero intensity) until `follow` finds the player at night.
pub(super) fn spawn(commands: &mut Commands, outdoor: bool) {
    commands.spawn((
        Light,
        Candle { outdoor },
        PointLight {
            color: CANDLE_COLOR,
            intensity: 0.0,
            range: CANDLE_RANGE,
            // The player stands in it, so its own shadow pass would cost every frame.
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::default(),
    ));
}

/// The flame's strength at `seconds`, `1.0 - FLICKER_DEPTH..=1.0`: a few unrelated sines, so the
/// flicker never visibly repeats.
fn flicker(seconds: f32) -> f32 {
    let wave = (seconds * 7.3).sin() * 0.5
        + (seconds * 13.1 + 1.7).sin() * 0.3
        + (seconds * 23.9 + 4.2).sin() * 0.2;
    1.0 - FLICKER_DEPTH * (0.5 + 0.5 * wave)
}

/// Keeps the candle above the player, lit as much as the cycle's hour is night, and flickering.
pub(super) fn follow(
    time: Res<Time>,
    cycle: Res<DayNight>,
    mut candles: Query<(&Candle, &mut PointLight, &mut Transform)>,
    suns: Query<&Sun>,
    players: Query<&GlobalTransform, With<Player>>,
) {
    let night = suns
        .iter()
        .next()
        .filter(|_| cycle.enabled)
        .map_or(0.0, |sun| day_night::night(sun.direction, cycle.hour));
    let player = players.iter().next();
    for (candle, mut light, mut transform) in &mut candles {
        let strength = if candle.outdoor && player.is_some() {
            night * flicker(time.elapsed_secs())
        } else {
            0.0
        };
        let intensity = point_light_power(CANDLE_FULL_AT) * strength;
        if light.intensity != intensity {
            light.intensity = intensity;
        }
        if let Some(player) = player {
            transform.set_if_neq(Transform::from_translation(
                player.translation() + Vec3::Y * CANDLE_HEIGHT,
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn the_candle_burns_only_at_the_cycles_night() {
        let installed = Dir3::new(Vec3::new(-0.5, -0.8, 0.3)).unwrap();
        assert_eq!(day_night::night(installed, 12.0), 0.0);
        assert_eq!(day_night::night(installed, 0.0), 1.0);
        let dusk = day_night::night(installed, 18.0);
        assert!(dusk > 0.0 && dusk < 1.0);
    }
}
