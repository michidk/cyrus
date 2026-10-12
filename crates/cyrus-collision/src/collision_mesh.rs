//! Authored model queries from preservation's segment and sphere admission rules.
use super::geometry::CollisionFace;
use super::geometry::FaceContact;
use super::geometry::SegmentContact;
use super::geometry::SphereCandidates;
use super::geometry::ordered_contacts;
use anyhow::Context;
use bevy::math::Vec3;
use cyrus_assets::collision::CollisionModel;
use cyrus_assets::collision::CollisionSubobject;

/// Validated, prepared polygons shared by queries against instances of one model.
pub struct CollisionMesh {
    radius: f32,
    faces: Vec<CollisionFace>,
    subobjects: Vec<CollisionSubobject>,
}

/// Segment hit retaining the authored face needed by surface-state selection.
#[derive(Debug)]
pub struct ModelSegmentContact {
    pub face: usize,
    pub contact: SegmentContact,
}

impl CollisionMesh {
    /// Prepare authored geometry once when loading a scene.
    pub fn new(model: &CollisionModel) -> anyhow::Result<Self> {
        let version = model
            .version
            .context("missing collision model version; re-export")?;
        Ok(Self {
            radius: model.radius,
            faces: crate::geometry::CollisionFace::from_model(model)?,
            subobjects: if version[1] == b'5' {
                model.subobjects.clone()
            } else {
                Vec::new()
            },
        })
    }

    /// Ordered model-local body contacts, suppressing successful faces across spheres.
    /// Saturated contact buffers remain unsupported; callers must not treat an error as clearance.
    pub fn sphere_contacts(
        &self,
        spheres: &[(Vec3, f32)],
        sidedness: bool,
    ) -> anyhow::Result<Vec<FaceContact>> {
        let mut candidates = Vec::new();
        for &(center, radius) in spheres {
            anyhow::ensure!(
                center.is_finite() && radius.is_finite() && radius >= 0.0,
                "sphere: invalid center or radius"
            );
            for (index, subobject) in self.subobjects.iter().enumerate() {
                if admits_sphere(subobject, index, center, radius)? {
                    candidates.push(SphereCandidates {
                        center,
                        radius,
                        faces: &subobject.faces,
                    });
                }
            }
        }
        let contacts = ordered_contacts(&self.faces, &candidates, sidedness)?;
        anyhow::ensure!(
            contacts.len() < 32,
            "sphere contact buffer saturation unsupported"
        );
        Ok(contacts)
    }

    /// Test model-admitted spheres in model-local map units for any polygon contact.
    /// The caller owns model-radius admission and actor/placement transforms.
    pub fn intersects_spheres(
        &self,
        spheres: &[(Vec3, f32)],
        sidedness: bool,
    ) -> anyhow::Result<bool> {
        for &(center, radius) in spheres {
            anyhow::ensure!(
                center.is_finite() && radius.is_finite() && radius >= 0.0,
                "sphere: invalid center or radius"
            );
            for (index, subobject) in self.subobjects.iter().enumerate() {
                if !admits_sphere(subobject, index, center, radius)? {
                    continue;
                }
                for &face in &subobject.faces {
                    if self.faces[face]
                        .sphere_contact(center, radius, sidedness)
                        .is_some()
                    {
                        return Ok(true);
                    }
                }
            }
        }
        Ok(false)
    }

    /// Original model-local segment query. Retains duplicates and first 32 successes.
    /// Missing extent factors are unsupported, never replaced with spherical bounds.
    pub fn segment_contacts(
        &self,
        start: Vec3,
        end: Vec3,
        sidedness: bool,
    ) -> anyhow::Result<Vec<ModelSegmentContact>> {
        anyhow::ensure!(
            start.is_finite() && end.is_finite(),
            "segment: nonfinite endpoint"
        );
        if !segment_radius(start, end, self.radius, false) {
            return Ok(Vec::new());
        }
        let mut contacts = Vec::new();
        for (index, subobject) in self.subobjects.iter().enumerate() {
            let extents = subobject
                .extents
                .with_context(|| format!("subobject {index}: missing version-5 extent factors"))?;
            anyhow::ensure!(
                extents.iter().all(|&value| value != 0.0),
                "subobject {index}: zero extent factor unsupported"
            );
            let inverse = Vec3::from_array(extents).recip();
            let center = Vec3::from_array(subobject.center);
            if !segment_radius(
                scale_endpoint(start, center, inverse),
                scale_endpoint(end, center, inverse),
                subobject.radius,
                true,
            ) {
                continue;
            }
            for &face in &subobject.faces {
                if let Some(contact) = self.faces[face].segment_contact(start, end, sidedness) {
                    contacts.push(ModelSegmentContact { face, contact });
                    if contacts.len() == 32 {
                        return Ok(contacts);
                    }
                }
            }
        }
        Ok(contacts)
    }
}

fn admits_sphere(
    subobject: &CollisionSubobject,
    index: usize,
    center: Vec3,
    radius: f32,
) -> anyhow::Result<bool> {
    let extents = subobject
        .extents
        .with_context(|| format!("subobject {index}: missing version-5 extent factors"))?;
    let displacement = center - Vec3::from_array(subobject.center);
    let distance_squared = displacement.as_dvec3().length_squared();
    anyhow::ensure!(
        distance_squared > 0.0,
        "subobject {index}: coincident sphere admission unsupported"
    );
    let reach = (displacement.as_dvec3()
        * (f64::from(subobject.radius) / distance_squared.sqrt())
        * Vec3::from_array(extents).as_dvec3())
    .as_vec3();
    Ok(distance_squared <= reach.as_dvec3().length_squared() + f64::from(radius * radius))
}

fn segment_radius(start: Vec3, end: Vec3, radius: f32, positive_projection: bool) -> bool {
    let radius_squared = radius * radius;
    let start_squared = dot(start, start);
    if start_squared <= radius_squared || dot(end, end) <= radius_squared {
        return true;
    }
    let direction = start - end;
    let length = dot(direction, direction);
    if length < 0.1 {
        return false;
    }
    let projection = dot(start, direction);
    if projection < 0.0 || (positive_projection && projection == 0.0) || projection > length {
        return false;
    }
    f64::from(start_squared) * f64::from(length) - f64::from(projection).powi(2)
        <= f64::from(radius_squared) * f64::from(length)
}

fn dot(a: Vec3, b: Vec3) -> f32 {
    a.as_dvec3().dot(b.as_dvec3()) as f32
}

fn scale_endpoint(point: Vec3, center: Vec3, inverse: Vec3) -> Vec3 {
    ((point.as_dvec3() - center.as_dvec3()) * inverse.as_dvec3()).as_vec3()
}
