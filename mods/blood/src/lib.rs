//! Optional blood particles. Spray dimensions, colors and gravity are mod preferences.
use cyrus_mod_sdk::{Cx, Mod, Particle, Vec3};

struct Blood;

impl Mod for Blood {
    const ID: &'static str = "blood";

    fn frame(cx: &Cx) {
        for event in cx.damage.events() {
            let Some(position) = event.position else {
                continue;
            };
            let seed = event
                .actor
                .bytes()
                .fold(cx.scene.seconds().to_bits(), |s, b| {
                    s.wrapping_mul(31).wrapping_add(u32::from(b))
                });
            for particle in spray(
                position.map_to_scene(),
                event.loss,
                cx.settings.number("amount"),
                cx.settings.number("lifetime"),
                seed,
            ) {
                if !cx.particles.emit(particle) {
                    break;
                }
            }
        }
    }
}

cyrus_mod_sdk::export_mod!(Blood);

fn spray(origin: Vec3, loss: u8, amount: f32, lifetime: f32, seed: u32) -> Vec<Particle> {
    if loss == 0 || !amount.is_finite() || !lifetime.is_finite() {
        return vec![];
    }
    let count = ((8.0 + f32::from(loss) * 0.5) * amount.clamp(0.0, 3.0))
        .round()
        .clamp(0.0, 64.0) as usize;
    let mut state = seed.max(1);
    let mut random = || {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        (state >> 8) as f32 / 16_777_216.0
    };
    (0..count)
        .map(|_| {
            let angle = random() * std::f32::consts::TAU;
            let speed = 0.6 + random() * 2.0;
            Particle {
                position: origin + Vec3::UP * (0.7 + random() * 0.6),
                velocity: Vec3::new(
                    angle.cos() * speed,
                    0.5 + random() * 2.0,
                    angle.sin() * speed,
                ),
                acceleration: Vec3::new(0.0, -9.8, 0.0),
                color: [0.5 + random() * 0.15, 0.015, 0.01, 1.0],
                radius: 0.035 + random() * 0.045,
                lifetime: lifetime.clamp(0.1, 2.0) * (0.6 + random() * 0.4),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_damage_emits_and_user_amount_can_disable_it() {
        assert!(spray(Vec3::ZERO, 0, 1.0, 0.7, 1).is_empty());
        assert!(spray(Vec3::ZERO, 10, 0.0, 0.7, 1).is_empty());
        assert!(spray(Vec3::ZERO, 10, f32::NAN, 0.7, 1).is_empty());
        assert_eq!(spray(Vec3::ZERO, 10, 1.0, 0.7, 1).len(), 13);
        assert_eq!(spray(Vec3::ZERO, 255, 3.0, 0.7, 1).len(), 64);
    }

    #[test]
    fn spray_is_reproducible_finite_and_uses_the_requested_origin_and_lifetime() {
        let origin = Vec3::new(2.0, 3.0, -4.0);
        let particles = spray(origin, 10, 1.0, 0.7, 42);
        assert_eq!(particles, spray(origin, 10, 1.0, 0.7, 42));
        for particle in particles {
            assert_eq!(particle.position.x, origin.x);
            assert_eq!(particle.position.z, origin.z);
            assert!(particle.position.y > origin.y);
            assert!((0.42..=0.7).contains(&particle.lifetime));
            assert!((0.035..=0.08).contains(&particle.radius));
            assert!(particle.velocity.length().is_finite());
        }
    }
}
