//! The candle the player carries at night.
//!
//! **Modern only.** The original lights nothing around the player. While the day/night cycle
//! darkens the sky, a warm light follows the player, more the darker it is, so the way stays
//! visible at night. The colour, reach and height are Modern tuning choices, not recovered
//! behaviour. Both lighting tiers read the one `Candle`: the raster tier lights it with a point
//! light (`light`), ray tracing with an emissive sphere beside its object lights.
use super::day_night::Daylight;
use super::lights::{OBJECT_LIGHT_FULL_AT, light_color, point_light_power};
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
/// Luminance weights of linear sRGB.
const LUMINANCE: Vec3 = Vec3::new(0.2126, 0.7152, 0.0722);

/// The lit candle, if any.
#[derive(Resource, Default, Clone, Copy, PartialEq, Debug)]
pub(in crate::fog) struct Candle(pub(in crate::fog) Option<Flame>);

/// Where the candle burns and how strongly: `darkness` is 0 by day and 1 in the dark of night.
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

/// Lights the candle on the player once the cycle has made the sky dark.
pub(in crate::fog) fn track(
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
            darkness,
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
    let intensity = point_light_power(REACH * OBJECT_LIGHT_FULL_AT) * flame.darkness;
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
    fn the_candle_burns_only_in_the_dark() {
        assert_eq!(darkness(&Daylight(None)), 0.0, "no cycle, no candle");
        assert!(darkness(&dark_sky(Vec3::ONE)) < DARK_FROM, "noon");
        assert!(
            darkness(&dark_sky(Vec3::new(0.03, 0.04, 0.09))) > 0.9,
            "night"
        );
    }
}
