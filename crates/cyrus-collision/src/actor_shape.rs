//! Authored actor collision spheres in original map coordinates.
use anyhow::{Context, Result, ensure};

/// Local integer sphere before the original single-precision query conversion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BodySphere {
    pub center: [i32; 3],
    pub radius: i32,
}

/// Axis chosen by strict dominant-total comparisons; ties are upright.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orientation {
    Upright,
    X,
    Z,
}

/// Prepared body count and separately stored additional sphere.
#[derive(Debug, PartialEq, Eq)]
pub struct ActorShape {
    pub orientation: Orientation,
    pub body: Vec<BodySphere>,
    pub additional: BodySphere,
}

/// Validated nonnegative MPSZ extents and signed RAEX radius scale.
/// Construction follows preservation `engine/player.md` actor collision shapes.
#[derive(Debug)]
pub struct ActorBounds {
    positive: [i32; 3],
    negative: [i32; 3],
    total: [i32; 3],
    scale: i32,
}

impl ActorBounds {
    /// Derive initial static-actor bounds in stored vertex order. Nearest-even
    /// conversion precedes the next extremum comparison; only X/Z extents use
    /// the positive system scale. Follows preservation's ordinary initialization.
    pub fn from_vertices(
        vertices: &[[f32; 3]],
        system_scale: i32,
        radius_scale: i32,
    ) -> Result<Self> {
        let mut positive = [0i32; 3];
        let mut negative = [0i32; 3];
        for (index, vertex) in vertices.iter().enumerate() {
            for axis in 0..3 {
                let value = vertex[axis];
                ensure!(
                    value.is_finite()
                        && (-2147483648.0..2147483648.0)
                            .contains(&f64::from(value.round_ties_even())),
                    "actor bounds vertex {index} axis {axis}: outside signed coordinate range"
                );
                if value >= 0.0 {
                    if value > positive[axis] as f32 {
                        positive[axis] = value.round_ties_even() as i32;
                    }
                } else if value < negative[axis] as f32 {
                    negative[axis] = value.round_ties_even() as i32;
                }
            }
        }
        let system_scale = if system_scale > 0 { system_scale } else { 256 };
        for axis in 0..3 {
            negative[axis] = negative[axis]
                .checked_abs()
                .context("actor bounds: negative magnitude overflow")?;
            if axis != 1 {
                positive[axis] = positive[axis]
                    .checked_mul(system_scale)
                    .context("actor bounds: positive scaled extent overflow")?
                    >> 8;
                negative[axis] = negative[axis]
                    .checked_mul(system_scale)
                    .context("actor bounds: negative scaled extent overflow")?
                    >> 8;
            }
        }
        Self::new(positive, negative, radius_scale)
    }

    /// Nonnegative positive extents in original map units, after any system scaling.
    pub fn positive_extent(&self) -> [i32; 3] {
        self.positive
    }

    /// Stored nonnegative negative-side extents, after any system scaling.
    pub fn negative_extent(&self) -> [i32; 3] {
        self.negative
    }

    pub fn new(positive: [i32; 3], negative: [i32; 3], scale: i32) -> Result<Self> {
        let mut total = [0; 3];
        for axis in 0..3 {
            ensure!(
                positive[axis] >= 0 && negative[axis] >= 0,
                "actor bounds axis {axis}: negative extent"
            );
            total[axis] = positive[axis]
                .checked_add(negative[axis])
                .with_context(|| format!("actor bounds axis {axis}: total exceeds signed range"))?;
        }
        Ok(Self {
            positive,
            negative,
            total,
            scale: if scale > 0 { scale } else { 256 },
        })
    }

    pub fn orientation(&self) -> Orientation {
        let [x, y, z] = self.total;
        if x > y && x > z {
            Orientation::X
        } else if z > x && z > y {
            Orientation::Z
        } else {
            Orientation::Upright
        }
    }

    /// Prepare only ordinary obstacle spheres; no support sphere is included.
    /// `post_collide_height` uses installed SYSTEM.INI map units.
    pub fn body_spheres(&self, post_collide_height: u32) -> Result<Vec<BodySphere>> {
        let post = i32::try_from(post_collide_height)
            .context("actor shape: post_collide_height exceeds signed range")?;
        let orientation = self.orientation();
        let horizontal = match orientation {
            Orientation::X => Some(0),
            Orientation::Z => Some(2),
            Orientation::Upright => None,
        };
        let mut radius = (match horizontal {
            Some(0) => self.total[1].max(self.total[2]),
            Some(_) => self.total[0].max(self.total[1]),
            None => self.total[0].max(self.total[2]),
        } / 2)
            .max(4);
        let (spacing, count, first_y) = if let Some(axis) = horizontal {
            let y = if i64::from(post) < 2 * i64::from(radius) {
                radius = (radius - post / 2).max(4);
                radius.wrapping_sub(self.negative[1])
            } else {
                -post
            };
            let spacing = radius.wrapping_mul(3) >> 1;
            ensure!(
                spacing > 0,
                "actor shape: horizontal spacing outside signed range"
            );
            (spacing, (self.total[axis] / spacing).max(1), y)
        } else {
            let spacing = (radius / 2).wrapping_mul(3);
            ensure!(
                spacing > 0,
                "actor shape: upright spacing outside signed range"
            );
            let available = self.total[1] - post;
            (
                spacing,
                ((if available < 0 { post } else { available }) / spacing).max(2),
                radius.wrapping_sub(self.negative[1]),
            )
        };
        ensure!(
            count <= i32::from(i16::MAX),
            "actor shape: sphere count {count} exceeds signed record range"
        );
        let scaled_radius = radius.wrapping_mul(self.scale) >> 8;
        ensure!(
            scaled_radius >= 0,
            "actor shape: scaled radius outside nonnegative range"
        );
        let mut body = Vec::with_capacity(count as usize);
        for index in 0..count {
            let center = match orientation {
                Orientation::X => [radius.wrapping_sub(self.negative[0]), first_y, 0],
                Orientation::Z => [
                    0,
                    first_y,
                    radius
                        .wrapping_sub(self.negative[2])
                        .wrapping_add(index.wrapping_mul(spacing)),
                ],
                Orientation::Upright => [
                    0,
                    if index + 1 == count {
                        self.positive[1]
                            .wrapping_sub(post)
                            .wrapping_sub(scaled_radius)
                    } else {
                        first_y.wrapping_add(index.wrapping_mul(spacing))
                    },
                    0,
                ],
            };
            body.push(BodySphere {
                center,
                radius: scaled_radius,
            });
        }
        Ok(body)
    }

    /// Include the extra stored sphere. Upright preparation uses the runtime Q8
    /// standing-height value directly in its map-coordinate center expression.
    pub fn prepare(&self, post_collide_height: u32, standing_height: i32) -> Result<ActorShape> {
        let body = self.body_spheres(post_collide_height)?;
        let orientation = self.orientation();
        let mut additional = body[0];
        if orientation == Orientation::Upright {
            additional.center = [
                0,
                self.positive[1]
                    .wrapping_sub(additional.radius)
                    .wrapping_sub(standing_height.wrapping_shl(8))
                    .wrapping_sub(2),
                0,
            ];
        }
        Ok(ActorShape {
            orientation,
            body,
            additional,
        })
    }
}
