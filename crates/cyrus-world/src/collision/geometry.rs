//! Original polygon contacts from preservation's engine/player.md.
use bevy::math::Vec3;

/// Original polygon geometry, before renderer triangulation. Normals retain the original model values.
pub struct CollisionFace {
    vertices: Vec<Vec3>,
    normal: Vec3,
}

/// Model-local segment intersection, before the placement transform.
#[derive(Debug, PartialEq)]
pub struct SegmentContact {
    pub point: Vec3,
    pub normal: Vec3,
    /// Original stored segment fraction, scaled by 32,768.
    pub rank: u32,
}

impl CollisionFace {
    /// Constructs query polygons from a validated authored model without triangulation.
    pub fn from_model(
        model: &cyrus_assets::collision::CollisionModel,
    ) -> anyhow::Result<Vec<Self>> {
        model.validate()?;
        model
            .faces
            .iter()
            .zip(&model.normals)
            .map(|(face, normal)| {
                Self::new(
                    face.iter()
                        .map(|&i| Vec3::from_array(model.vertices[i]))
                        .collect(),
                    Vec3::from_array(*normal),
                )
            })
            .collect()
    }

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

    /// Intersects a segment in model-local map units with the authored polygon.
    /// Coplanar segments are excluded; a crossing may end on the face boundary.
    pub fn segment_contact(
        &self,
        start: Vec3,
        end: Vec3,
        sidedness: bool,
    ) -> Option<SegmentContact> {
        let start_distance = (self.vertices[0] - start).dot(self.normal);
        let end_distance = (self.vertices[0] - end).dot(self.normal);
        if (sidedness && start_distance > 4.0) || (start_distance < 0.0) == (end_distance < 0.0) {
            return None;
        }
        let fraction = start_distance / (start_distance - end_distance);
        let point = start + (end - start) * fraction;
        for index in 0..self.vertices.len() {
            let a = self.vertices[index] - point;
            let b = self.vertices[(index + 1) % self.vertices.len()] - point;
            if a.cross(b).dot(self.normal) < 0.0 {
                return None;
            }
        }
        Some(SegmentContact {
            point,
            normal: self.normal,
            rank: (fraction * 32_768.0).round_ties_even() as u32,
        })
    }
}

/// One admitted sphere and its authored face references, in traversal order.
/// Subobject admission and model-space transforms are supplied by the caller.
pub struct SphereCandidates<'a> {
    pub center: Vec3,
    pub radius: f32,
    pub faces: &'a [usize],
}

/// A successful model-local contact, retaining the original face identity.
#[derive(Debug, PartialEq)]
pub struct FaceContact {
    pub face: usize,
    pub projection: Vec3,
    pub normal: Vec3,
}

/// Test admitted spheres in order, suppressing only faces that already succeeded.
/// References may repeat; failed tests remain eligible for later spheres.
/// Inputs use model-local map units and original polygon indices.
pub fn ordered_contacts(
    faces: &[CollisionFace],
    spheres: &[SphereCandidates<'_>],
    sidedness: bool,
) -> anyhow::Result<Vec<FaceContact>> {
    let mut accepted = vec![false; faces.len()];
    let mut contacts = Vec::new();
    for (sphere_index, sphere) in spheres.iter().enumerate() {
        anyhow::ensure!(
            sphere.center.is_finite() && sphere.radius.is_finite() && sphere.radius >= 0.0,
            "collision sphere {sphere_index}: invalid center or radius"
        );
        for &index in sphere.faces {
            let face = faces.get(index).ok_or_else(|| {
                anyhow::anyhow!("collision sphere {sphere_index}: face {index} outside model")
            })?;
            if accepted[index] {
                continue;
            }
            if let Some(projection) = face.sphere_contact(sphere.center, sphere.radius, sidedness) {
                accepted[index] = true;
                contacts.push(FaceContact {
                    face: index,
                    projection,
                    normal: face.normal,
                });
            }
        }
    }
    Ok(contacts)
}
