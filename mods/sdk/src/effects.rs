//! Damage observations and transient particles, with explicit coordinate and lifetime contracts.
use crate::{Vec3, raw};

/// An actual health reduction from a health service, rather than an attempted hit.
#[derive(Clone, Debug, PartialEq)]
pub struct DamageEvent {
    /// Script actor name, for display/filtering. Names need not uniquely identify instances.
    pub actor: String,
    pub loss: u8,
    /// Map units and original axes. Use `Vec3::map_to_scene` before emitting particles.
    pub position: Option<Vec3>,
    /// Known attacker position in map units. This establishes an incoming direction,
    /// not a measured weapon velocity. None for damage without an attributable source.
    pub source: Option<Vec3>,
}

/// Read-only batch, replaced each PreUpdate and shared by all mods for that frame.
pub struct Damage(pub(crate) raw::Damage);

impl Damage {
    pub fn events(&self) -> Vec<DamageEvent> {
        (0..self.0.count())
            .map(|index| DamageEvent {
                actor: self.0.actor(index),
                loss: self.0.loss(index) as u8,
                source: match self.0.source(index).as_slice() {
                    &[x, y, z] => Some(Vec3::new(x, y, z)),
                    _ => None,
                },
                position: match self.0.position(index).as_slice() {
                    &[x, y, z] => Some(Vec3::new(x, y, z)),
                    _ => None,
                },
            })
            .collect()
    }
}

/// One directional Hanabi GPU burst, with Y up and distances in scene units.
/// Droplets face the camera, stretch along their velocity and fade near the end
/// of their lifetimes. They are not saved and do not collide with geometry.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParticleBurst {
    pub position: Vec3,
    /// Nonzero forward direction. The host normalizes it.
    pub direction: Vec3,
    /// Scene units per second squared, in world axes.
    pub acceleration: Vec3,
    /// sRGB RGBA, each in 0..=1.
    pub color: [f32; 4],
    /// Ordered min/max scene units per second, 0..=1000.
    pub speed: [f32; 2],
    /// Ordered min/max scene-unit droplet radii, 0.001..=10.
    pub radius: [f32; 2],
    /// Ordered min/max game seconds, 0.01..=10.
    pub lifetime: [f32; 2],
    /// Lateral velocity relative to forward velocity, 0..=1.
    pub spread: f32,
    /// Elongation along velocity, 1..=8.
    pub stretch: f32,
    /// Particles in this burst, 1..=64.
    pub count: u32,
}

/// A mod-scoped particle emitter. A burst shares one GPU program with other bursts.
pub struct Particles {
    pub(crate) raw: raw::Particles,
    pub(crate) id: &'static str,
}

impl Particles {
    /// Queue a burst for this frame's PostUpdate. Returns false for invalid values
    /// or exhausted capacity. Queued and live limits count particles: 256 per mod,
    /// 1024 total. Overflow drops whole bursts. Disable/reload and world replacement
    /// remove their effects.
    pub fn burst(&self, burst: ParticleBurst) -> bool {
        self.raw.burst(
            self.id,
            &<[f32; 3]>::from(burst.position),
            &<[f32; 3]>::from(burst.direction),
            &<[f32; 3]>::from(burst.acceleration),
            &burst.color,
            &burst.speed,
            &burst.radius,
            &burst.lifetime,
            burst.spread,
            burst.stretch,
            burst.count,
        )
    }
}
