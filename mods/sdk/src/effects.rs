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
}

/// Read-only batch, replaced each PreUpdate and shared by all mods for that frame.
pub struct Damage(pub(crate) raw::Damage);

impl Damage {
    pub fn events(&self) -> Vec<DamageEvent> {
        (0..self.0.count())
            .map(|index| DamageEvent {
                actor: self.0.actor(index),
                loss: self.0.loss(index) as u8,
                position: match self.0.position(index).as_slice() {
                    &[x, y, z] => Some(Vec3::new(x, y, z)),
                    _ => None,
                },
            })
            .collect()
    }
}

/// One unlit spherical particle. All geometry uses scene coordinates, with Y up.
/// The engine integrates acceleration and fades alpha linearly over `lifetime`.
/// Particles do not collide with geometry and are never saved.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Particle {
    pub position: Vec3,
    /// Scene units per second.
    pub velocity: Vec3,
    /// Scene units per second squared.
    pub acceleration: Vec3,
    /// sRGB red, green, blue and alpha, each in 0..=1.
    pub color: [f32; 4],
    /// Scene units, 0.001..=10.
    pub radius: f32,
    /// Game seconds, 0.01..=10.
    pub lifetime: f32,
}

/// A mod-scoped particle emitter. No guest memory is needed after an emission.
pub struct Particles {
    pub(crate) raw: raw::Particles,
    pub(crate) id: &'static str,
}

impl Particles {
    /// Queue a particle for this frame's PostUpdate. Returns false for invalid values
    /// or an exhausted queue. Live capacity can also drop new emissions: 256 per mod,
    /// 1024 total. Disable/reload removes this mod's particles; world replacement removes all.
    pub fn emit(&self, particle: Particle) -> bool {
        self.raw.emit(
            self.id,
            &<[f32; 3]>::from(particle.position),
            &<[f32; 3]>::from(particle.velocity),
            &<[f32; 3]>::from(particle.acceleration),
            &particle.color,
            particle.radius,
            particle.lifetime,
        )
    }
}
