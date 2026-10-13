//! Original bound and authored-polygon admission before texture binding.
use anyhow::ensure;
use bevy::prelude::*;
use serde::Deserialize;

pub(super) type Clip = crate::terrain::Preparation;

#[derive(Deserialize)]
pub(super) struct Model {
    version: u32,
    radius_fixed: u32,
    polygons: Vec<Polygon>,
}

#[derive(Deserialize)]
struct Polygon {
    vertices: Option<Vec<usize>>,
    normal: Option<[f32; 3]>,
}

impl Model {
    pub(super) fn parse(extras: &str) -> anyhow::Result<Option<Self>> {
        #[derive(Deserialize)]
        struct Extras {
            rgpre_admission: Option<Model>,
        }
        let model = serde_json::from_str::<Extras>(extras)?.rgpre_admission;
        if let Some(model) = &model {
            ensure!(
                model.version == 1,
                "polygon admission version {} unsupported",
                model.version
            );
            for (face, polygon) in model.polygons.iter().enumerate() {
                ensure!(
                    polygon
                        .vertices
                        .as_ref()
                        .is_some_and(|vertices| vertices.len() >= 3),
                    "polygon {face}: missing authored corners"
                );
                ensure!(
                    polygon
                        .normal
                        .is_some_and(|normal| normal.into_iter().all(f32::is_finite)),
                    "polygon {face}: missing finite authored normal"
                );
            }
        }
        Ok(model)
    }

    pub(super) fn validate(&self, positions: &[[f32; 3]]) -> anyhow::Result<()> {
        for (face, polygon) in self.polygons.iter().enumerate() {
            let vertices = polygon
                .vertices
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("polygon {face}: missing corners"))?;
            for &index in vertices {
                ensure!(
                    positions
                        .get(index)
                        .is_some_and(|p| p.iter().all(|v| v.is_finite())),
                    "polygon {face}: invalid position index {index}"
                );
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn admits(
        &self,
        positions: &[[f32; 3]],
        normals: Option<&[[f32; 3]]>,
        model: &GlobalTransform,
        camera: &GlobalTransform,
        scale: Vec2,
        clipping: [f32; 2],
        clip: &mut Clip,
    ) -> bool {
        let camera_from_model = camera.affine().inverse() * model.affine();
        let scaled = |p: Vec3| {
            let point = camera_from_model.transform_point3(p) * 20.0;
            [point.x * scale.x, point.y * scale.y, -point.z]
        };
        let origin = scaled(Vec3::ZERO);
        let radius = self.radius_fixed as f32 / 256.0;
        let Some(crosses) = classify(origin, radius, scale, clipping) else {
            return false;
        };
        let local_camera = model
            .affine()
            .inverse()
            .transform_point3(camera.translation());
        self.polygons.iter().any(|polygon| {
            let Some(indices) = &polygon.vertices else {
                return false;
            };
            let Some(&first) = indices.first() else {
                return false;
            };
            if indices.iter().any(|&index| {
                positions
                    .get(index)
                    .is_none_or(|p| p.iter().any(|v| !v.is_finite()))
            }) {
                return false;
            }
            let normal = match normals {
                Some(values) => values.get(first).copied(),
                None => polygon.normal,
            };
            let Some(normal) = normal else { return false };
            let relative = Vec3::from(positions[first]) - local_camera;
            let facing = (relative.x * normal[0] + relative.y * normal[1]) + relative.z * normal[2];
            if facing >= 0.0 || !facing.is_finite() {
                return false;
            }
            !crosses
                || clip.admits(
                    indices.iter().map(|&index| crate::terrain::ClipVertex {
                        camera: scaled(Vec3::from(positions[index])),
                        ..Default::default()
                    }),
                    clipping,
                )
        })
    }
}

// Projection-scaled bounds and single-precision stores follow preservation's
// texture-animation.md, including strict rejection and plane-crossing boundaries.
fn classify([x, y, z]: [f32; 3], radius: f32, scale: Vec2, [near, far]: [f32; 2]) -> Option<bool> {
    let hx = 1.415 * (radius * scale.x);
    let hy = 1.415 * (radius * scale.y);
    let bounds = [
        (z - radius, z + radius, near, far),
        (x - hx, x + hx, -z, z),
        (y - hy, y + hy, -z, z),
    ];
    if bounds
        .iter()
        .any(|&(low, high, min, max)| high <= min || low >= max)
    {
        return None;
    }
    Some(
        bounds
            .iter()
            .any(|&(low, high, min, max)| low < min || high > max),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounds_reject_tangency_but_do_not_classify_equal_endpoints_as_crossings() {
        assert_eq!(
            classify([0., 0., 2.], 1., Vec2::ONE, [1., 10.]),
            Some(false)
        );
        assert_eq!(classify([0., 0., 0.], 1., Vec2::ONE, [1., 10.]), None);
        assert_eq!(classify([0., 0., 11.], 1., Vec2::ONE, [1., 10.]), None);
        assert_eq!(
            classify([0., 0., 1.5], 1., Vec2::ONE, [1., 10.]),
            Some(true)
        );
    }
    #[test]
    fn admission_precedes_raster_area_and_respects_authored_facing() {
        let model = Model::parse(r#"{"rgpre_admission":{"version":1,"radius_fixed":256,"polygons":[{"vertices":[0,1,2],"normal":[0,0,1]}]}}"#).unwrap().unwrap();
        let mut clip = Clip::default();
        let camera = GlobalTransform::IDENTITY;
        let transform = GlobalTransform::from_translation(Vec3::new(0., 0., -1.));
        let degenerate = [[0., 0., 0.]; 3];
        assert!(model.admits(
            &degenerate,
            None,
            &transform,
            &camera,
            Vec2::ONE,
            [1., 20.5],
            &mut clip
        ));
        assert!(!model.admits(
            &degenerate,
            Some(&[[0., 0., -1.]; 3]),
            &transform,
            &camera,
            Vec2::ONE,
            [1., 20.5],
            &mut clip
        ));
        let outside = [[100., 0., 0.]; 3];
        assert!(!model.admits(
            &outside,
            None,
            &transform,
            &camera,
            Vec2::ONE,
            [1., 20.5],
            &mut clip
        ));
    }
}
