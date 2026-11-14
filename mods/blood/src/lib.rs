//! Optional directional blood droplets. Appearance and fallback placement are mod preferences.
use cyrus_mod_sdk::{Cx, DamageEvent, Mod, ParticleBurst, Vec3};

struct Blood;
impl Mod for Blood {
    const ID: &'static str = "blood";
    fn frame(cx: &Cx) {
        for event in cx.damage.events() {
            for burst in spray(
                &event,
                cx.settings.number("amount"),
                cx.settings.number("lifetime"),
            ) {
                if !cx.particles.burst(burst) {
                    break;
                }
            }
        }
    }
}
cyrus_mod_sdk::export_mod!(Blood);

fn spray(event: &DamageEvent, amount: f32, lifetime: f32) -> Vec<ParticleBurst> {
    let Some(position) = event.position else {
        return vec![];
    };
    if event.loss == 0 || !amount.is_finite() || amount <= 0.0 || !lifetime.is_finite() {
        return vec![];
    }
    let origin = position.map_to_scene();
    // Query positions are actor anchors, not wound locations. Keep the artistic
    // offset small; exact contact points are not supplied by every health service.
    let position = origin + Vec3::UP * 1.6;
    let incoming = event.source.and_then(|source| {
        let delta = origin - source.map_to_scene();
        Vec3::new(delta.x, 0.0, delta.z).normalized()
    });
    // Environmental damage has no attacker. A short downward spill avoids inventing one.
    let direction = incoming.unwrap_or(Vec3::new(0.0, -1.0, 0.0));
    let count = ((10.0 + f32::from(event.loss) * 0.6) * amount.clamp(0.0, 3.0))
        .round()
        .clamp(1.0, 64.0) as u32;
    let duration = lifetime.clamp(0.1, 2.0);
    let droplets = ParticleBurst {
        position,
        direction,
        acceleration: Vec3::new(0.0, -9.8, 0.0),
        color: [0.52, 0.025, 0.018, 0.95],
        speed: if incoming.is_some() {
            [4.0, 7.0]
        } else {
            [0.3, 1.0]
        },
        radius: [0.045, 0.09],
        lifetime: [duration * 0.6, duration],
        spread: 0.38,
        stretch: 2.4,
        count,
    };
    let fine = ParticleBurst {
        color: [0.48, 0.02, 0.015, 0.5],
        radius: [0.015, 0.035],
        speed: [2.0, 4.5],
        lifetime: [duration * 0.2, duration * 0.45],
        spread: 0.65,
        stretch: 1.3,
        count: count / 2 + 1,
        ..droplets
    };
    vec![droplets, fine]
}

#[cfg(test)]
mod tests {
    use super::*;
    fn hit(source: Option<Vec3>) -> DamageEvent {
        DamageEvent {
            actor: "target".into(),
            loss: 10,
            position: Some(Vec3::new(0.0, 0.0, 65_535.996)),
            source,
        }
    }
    #[test]
    fn opposite_attackers_produce_opposite_sprays_in_scene_axes() {
        let left = spray(&hit(Some(Vec3::new(100.0, 0.0, 65_535.996))), 1.0, 0.7);
        let right = spray(&hit(Some(Vec3::new(-100.0, 0.0, 65_535.996))), 1.0, 0.7);
        assert_eq!(left[0].direction, Vec3::new(1.0, 0.0, 0.0));
        assert_eq!(right[0].direction, Vec3::new(-1.0, 0.0, 0.0));
        assert!(
            left.iter()
                .all(|burst| burst.direction.x > 0.0 && burst.spread < 1.0)
        );
        assert_eq!(left[0].position, Vec3::UP * 1.6);
    }
    #[test]
    fn missing_or_coincident_sources_spill_down_and_disabled_or_missing_damage_emits_nothing() {
        assert_eq!(
            spray(&hit(None), 1.0, 0.7)[0].direction,
            Vec3::new(0.0, -1.0, 0.0)
        );
        assert_eq!(
            spray(&hit(hit(None).position), 1.0, 0.7)[0].direction,
            Vec3::new(0.0, -1.0, 0.0)
        );
        assert!(spray(&hit(None), 0.0, 0.7).is_empty());
        assert!(spray(&hit(None), f32::NAN, 0.7).is_empty());
        let mut event = hit(None);
        event.loss = 0;
        assert!(spray(&event, 1.0, 0.7).is_empty());
        event.loss = 10;
        event.position = None;
        assert!(spray(&event, 1.0, 0.7).is_empty());
    }
}
