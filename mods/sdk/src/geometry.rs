//! Revision-scoped terrain and owner-scoped indexed scene meshes.
use crate::{Persist, Vec3, raw};

/// Opaque terrain snapshot revision. Reacquire after world/binding replacement.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct TerrainRevision(u64);
/// Opaque world generation used to reject uploads prepared for a departed world.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorldGeneration(u64);
/// Opaque loaded-instance token. Changes on successful reload or reactivation;
/// compare it alongside terrain/world revisions when retaining geometry decisions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GeometryLease([u64; 2]);
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct TextureId {
    index: u32,
    revision: TerrainRevision,
}
#[derive(Clone, Debug)]
pub struct TerrainTriangle {
    pub id: u32,
    pub texture: TextureId,
    pub positions: [Vec3; 3],
    pub uvs: [[f32; 2]; 3],
}
/// A half-open rectangle in scene X/Z. Triangles are assigned by their centroid.
#[derive(Clone, Copy)]
pub struct Region {
    pub min: [f32; 2],
    pub max: [f32; 2],
}
pub struct Terrain(pub(crate) raw::Terrain);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GeometryError {
    Loading,
    Unsupported,
    Stale,
    Unavailable,
    Invalid,
    OverBudget,
}
#[derive(Clone, Debug)]
pub struct Pixels {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}
impl Pixels {
    /// Nearest wrapped sampling, using the source triangle's interpolated UV.
    pub fn sample(&self, uv: [f32; 2]) -> Option<[u8; 4]> {
        if self.width == 0 || self.height == 0 || uv.iter().any(|v| !v.is_finite()) {
            return None;
        }
        let x = (uv[0].rem_euclid(1.0) * self.width as f32) as usize;
        let y = (uv[1].rem_euclid(1.0) * self.height as f32) as usize;
        let i = (y.min(self.height as usize - 1) * self.width as usize
            + x.min(self.width as usize - 1))
        .checked_mul(4)?;
        self.rgba.get(i..i.checked_add(4)?)?.try_into().ok()
    }
}
impl Terrain {
    fn error(&self, revision: TerrainRevision) -> GeometryError {
        match self.0.status(revision.0) {
            0 => GeometryError::Invalid,
            1 => GeometryError::Loading,
            3 => GeometryError::Stale,
            4 => GeometryError::Unsupported,
            5 => GeometryError::OverBudget,
            _ => GeometryError::Unavailable,
        }
    }
    pub fn revision(&self) -> Option<TerrainRevision> {
        self.0.revision().map(TerrainRevision)
    }
    /// Up to 512 triangles per page; max region width/depth 512 scene units.
    pub fn triangles(
        &self,
        revision: TerrainRevision,
        region: Region,
        offset: u32,
    ) -> Result<Vec<TerrainTriangle>, GeometryError> {
        let data = self
            .0
            .triangles(
                revision.0,
                &[region.min[0], region.min[1], region.max[0], region.max[1]],
                offset,
                512,
            )
            .ok_or_else(|| self.error(revision))?;
        Ok(data
            .as_chunks::<17>()
            .0
            .iter()
            .map(|v| TerrainTriangle {
                texture: TextureId {
                    index: v[0] as u32,
                    revision,
                },
                id: v[1] as u32,
                positions: std::array::from_fn(|i| {
                    Vec3::new(v[2 + i * 5], v[3 + i * 5], v[4 + i * 5])
                }),
                uvs: std::array::from_fn(|i| [v[5 + i * 5], v[6 + i * 5]]),
            })
            .collect())
    }
    /// Read the image in bounded rows. Native snapshots cap images at 512 by 512.
    pub fn pixels(
        &self,
        revision: TerrainRevision,
        texture: TextureId,
    ) -> Result<Pixels, GeometryError> {
        if texture.revision != revision {
            return Err(GeometryError::Stale);
        }
        let dimensions = self
            .0
            .dimensions(revision.0, texture.index)
            .ok_or_else(|| self.error(revision))?;
        let &[width, height] = dimensions.as_slice() else {
            return Err(GeometryError::Invalid);
        };
        let mut rgba = Vec::new();
        for y in (0..height).step_by(128) {
            rgba.extend(
                self.0
                    .pixels(
                        revision.0,
                        texture.index,
                        0,
                        y,
                        width,
                        (height - y).min(128),
                    )
                    .ok_or_else(|| self.error(revision))?,
            );
        }
        Ok(Pixels {
            width,
            height,
            rgba,
        })
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vertex {
    pub position: Vec3,
    pub normal: Vec3,
    pub uv: [f32; 2],
    pub color: [f32; 4],
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mesh {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
}
/// Four finite shader values, each within +/-100. The active geometry_surface library
/// interprets them. The built-in library leaves vertices stationary.
#[derive(Clone, Copy, Debug, Default)]
pub struct Material {
    pub parameters: [f32; 4],
    /// Conservative scene-unit expansion of mesh bounds for shader deformation, 0..=100.
    pub bounds_padding: f32,
}
pub struct Geometry {
    pub(crate) raw: raw::Geometry,
    pub(crate) id: &'static str,
}
impl Geometry {
    pub fn contains(&self, key: &str) -> bool {
        self.raw.contains(self.id, key)
    }
    pub fn generation(&self) -> Option<WorldGeneration> {
        self.raw.generation().map(WorldGeneration)
    }
    pub fn lease(&self) -> Option<GeometryLease> {
        self.raw
            .lease(self.id)
            .as_slice()
            .try_into()
            .ok()
            .map(GeometryLease)
    }
    /// 1.5 million vertices and 256 keys per owner, 3 million vertices globally.
    /// Rejected replacements retain the previous mesh. Colors are linear RGBA 0..=1.
    pub fn replace(
        &self,
        key: &str,
        generation: WorldGeneration,
        mesh: &Mesh,
        material: Material,
    ) -> Result<(), GeometryError> {
        let vertices: Vec<_> = mesh
            .vertices
            .iter()
            .flat_map(|v| {
                let mut data = Vec::from(<[f32; 3]>::from(v.position));
                data.extend(<[f32; 3]>::from(v.normal));
                data.extend(v.uv);
                data.extend(v.color);
                data
            })
            .collect();
        match self.raw.replace(
            self.id,
            key,
            generation.0,
            &vertices,
            &mesh.indices,
            &[
                material.parameters[0],
                material.parameters[1],
                material.parameters[2],
                material.parameters[3],
                material.bounds_padding,
            ],
        ) {
            0 => Ok(()),
            1 => Err(GeometryError::Unavailable),
            2 => Err(GeometryError::Invalid),
            _ => Err(GeometryError::OverBudget),
        }
    }
    pub fn remove(&self, key: &str) {
        self.raw.remove(self.id, key);
    }
}

impl Persist for TerrainRevision {
    fn pack(&self) -> Vec<f32> {
        [self.0].pack()
    }
    fn unpack(values: &[f32]) -> Option<Self> {
        <[u64; 1]>::unpack(values).map(|[value]| Self(value))
    }
}
impl Persist for GeometryLease {
    fn pack(&self) -> Vec<f32> {
        self.0.pack()
    }
    fn unpack(values: &[f32]) -> Option<Self> {
        <[u64; 2]>::unpack(values).map(Self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Persist;
    #[test]
    fn leases_roundtrip_without_truncation() {
        let lease = GeometryLease([u64::MAX, (1 << 53) + 1]);
        assert_eq!(GeometryLease::unpack(&lease.pack()), Some(lease));
        assert!(GeometryLease::unpack(&[f32::NAN; 8]).is_none());
        assert!(GeometryLease::unpack(&[1.5; 8]).is_none());
        assert!(GeometryLease::unpack(&[0.; 4]).is_none());
    }
    #[test]
    fn pixels_wrap_uvs_and_reject_invalid_data() {
        let image = Pixels {
            width: 2,
            height: 1,
            rgba: vec![40, 120, 50, 255, 200, 160, 110, 255],
        };
        assert_eq!(image.sample([0.1, 0.8]), Some([40, 120, 50, 255]));
        assert_eq!(image.sample([-0.1, 1.8]), Some([200, 160, 110, 255]));
        assert_eq!(image.sample([f32::NAN, 0.0]), None);
        assert_eq!(
            Pixels {
                width: 0,
                height: 0,
                rgba: vec![]
            }
            .sample([0., 0.]),
            None
        );
    }
    #[test]
    fn revisions_roundtrip_without_floating_point_truncation() {
        for value in [0, 1, u64::MAX, 1 << 53] {
            let revision = TerrainRevision(value);
            assert_eq!(TerrainRevision::unpack(&revision.pack()), Some(revision));
        }
        assert!(TerrainRevision::unpack(&[f32::NAN; 4]).is_none());
        assert!(TerrainRevision::unpack(&[1.5; 4]).is_none());
    }
}
