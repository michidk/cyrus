//! The candle the player carries at night: a warm point light that follows the player.
//!
//! **Modern only.** The original lights nothing around the player; its HUD candle shows health.
//! The candle shines in outdoor worlds, those with a sky, and fades in as the sun dims, so it
//! lights the installed night world and the day/night cycle's night but not the day, sunset or
//! interiors. Its colour, reach and brightness are Modern tuning choices.
use super::lights::{Sun, point_light_power};
use super::plugin::Light;
use bevy::prelude::*;
use cyrus_script::player::Player;

/// Warm candle flame colour, linear light factors.
const CANDLE_COLOR: Color = Color::srgb(1.0, 0.8, 0.6);
/// Distance from the flame, in world units, where a facing surface gets the full candle colour.
const CANDLE_FULL_AT: f32 = 2.0;
/// Distance the candle reaches, in world units.
const CANDLE_RANGE: f32 = 15.0;
/// Height of the light above the player's origin at the feet, in world units: just above the head,
/// so it lights the player and the ground around them rather than shining from inside the body.
const CANDLE_HEIGHT: f32 = 4.5;
/// Installed 0..=255 sun peak at or below which the candle shines fully: the night island's sun
/// is 44 and the cycle's midnight moon about 61.
const DARK_SUN: f32 = 64.0;
/// Sun peak at or above which the candle is out: the island's day and sunset suns are 255.
const BRIGHT_SUN: f32 = 160.0;

/// The player's candle; `outdoor` while the world has a sky.
#[derive(Component)]
pub(super) struct Candle {
    outdoor: bool,
}

/// Spawns the candle, out (zero intensity) until `follow` finds the player in the dark.
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

/// How fully the candle shines under a sun of this 0..=255 peak; no sun is dark.
fn darkness(sun_peak: Option<f32>) -> f32 {
    let Some(peak) = sun_peak else { return 1.0 };
    let t = ((peak - DARK_SUN) / (BRIGHT_SUN - DARK_SUN)).clamp(0.0, 1.0);
    1.0 - t * t * (3.0 - 2.0 * t)
}

/// Keeps the candle at the player's hand and as bright as the night is dark.
pub(super) fn follow(
    mut candles: Query<(&Candle, &mut PointLight, &mut Transform)>,
    suns: Query<&DirectionalLight, With<Sun>>,
    players: Query<&GlobalTransform, With<Player>>,
) {
    let sun_peak = suns.iter().next().map(|sun| {
        let [r, g, b, _] = sun.color.to_srgba().to_f32_array();
        r.max(g).max(b) * 255.0
    });
    let player = players.iter().next();
    for (candle, mut light, mut transform) in &mut candles {
        let strength = if candle.outdoor && player.is_some() {
            darkness(sun_peak)
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
    fn the_candle_burns_at_night_and_is_out_by_day() {
        // WORLD.INI: island day 255, night 44; the cycle's midnight moon about 61.
        assert_eq!(darkness(Some(255.0)), 0.0);
        assert_eq!(darkness(Some(44.0)), 1.0);
        assert_eq!(darkness(Some(61.0)), 1.0);
        assert_eq!(darkness(None), 1.0);
        let dusk = darkness(Some(110.0));
        assert!(dusk > 0.0 && dusk < 1.0);
    }
}
