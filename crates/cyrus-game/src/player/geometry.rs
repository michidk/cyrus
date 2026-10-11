//! Sphere-to-polygon contacts from preservation's engine/player.md.
use bevy::math::Vec3;

/// Original polygon geometry, before renderer triangulation. Normals retain the original model values.
pub struct CollisionFace {
    vertices: Vec<Vec3>,
    normal: Vec3,
}

impl CollisionFace {
    /// Validates exported geometry at the scene-data boundary.
    pub fn new(vertices: Vec<Vec3>, normal: Vec3) -> anyhow::Result<Self> {
        anyhow::ensure!(
            vertices.len() >= 3 && vertices.iter().all(|vertex| vertex.is_finite()),
            "collision face: expected at least three finite vertices"
        );
        anyhow::ensure!(
            normal.is_finite() && normal != Vec3::ZERO,
            "collision face: expected a finite nonzero normal"
        );
        Ok(Self { vertices, normal })
    }

    /// Tests in model-local map units, returning the plane projection even when an
    /// edge or vertex accepts the sphere. The caller owns world/model transforms.
    pub fn sphere_contact(&self, center: Vec3, radius: f32, sidedness: bool) -> Option<Vec3> {
        let distance = (self.vertices[0] - center).dot(self.normal);
        let radius_squared = radius * radius;
        if (sidedness && distance > 4.0) || distance * distance > radius_squared {
            return None;
        }
        let projection = center + self.normal * distance;
        let mut inside = true;
        for index in 0..self.vertices.len() {
            let a = self.vertices[index];
            let b = self.vertices[(index + 1) % self.vertices.len()];
            if a.distance_squared(center) <= radius_squared {
                return Some(projection);
            }
            let edge = b - a;
            let length_squared = edge.length_squared();
            if length_squared > 0.0 {
                let fraction = ((center - a).dot(edge) / length_squared).clamp(0.0, 1.0);
                if (a + edge * fraction).distance_squared(center) <= radius_squared {
                    return Some(projection);
                }
            }
            inside &= (a - projection).cross(b - projection).dot(self.normal) >= 0.0;
        }
        inside.then_some(projection)
    }
}
