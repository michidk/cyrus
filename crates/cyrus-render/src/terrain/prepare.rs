//! Camera-space clipping and fixed terrain color and texture planes from preservation's Glide contract.
//!
//! See <https://github.com/michidk/redguard-preservation/blob/837f22c/docs/engine/terrain-lighting.md>.

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Vertex {
    /// Projection-scaled camera coordinates, positive depth, in engine units.
    pub camera: [f32; 3],
    /// Encoded channels in 0..=255, before fixed-point setup.
    pub rgb: [f32; 3],
    /// Original Glide coordinates, before perspective weighting or export row reversal.
    pub uv: [f32; 2],
    pub local: [f32; 3],
    pub normal: [f32; 3],
    pub water: f32,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Projected {
    pub vertex: Vertex,
    /// Unbiased screen coordinates snapped to sixteenths of a pixel.
    pub screen: [f32; 2],
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Triangle {
    pub vertices: [Projected; 3],
    /// Each channel stores the adjusted start, X gradient, and Y gradient.
    pub planes: [[i32; 3]; 3],
    /// S/W, T/W and W: adjusted start, X gradient, Y gradient, with 32 fractional bits.
    pub texture: [[i64; 3]; 3],
    pub anchor: [i32; 2],
}

/// Scratch polygons are reused across every triangle in a frame.
#[derive(Default)]
pub(crate) struct Preparation {
    polygon: Vec<Vertex>,
    next: Vec<Vertex>,
}

impl Vertex {
    fn distance(self, plane: usize, clipping: [f32; 2]) -> f64 {
        let [x, y, z] = self.camera.map(f64::from);
        match plane {
            0 => z - f64::from(clipping[0]),
            1 => f64::from(clipping[1]) - z,
            2 => x + z,
            3 => z - x,
            4 => y + z,
            5 => z - y,
            _ => unreachable!(),
        }
    }

    fn outside(self, clipping: [f32; 2]) -> u8 {
        (0..6).fold(0, |mask, plane| {
            mask | (u8::from(self.distance(plane, clipping) < 0.0) << plane)
        })
    }

    fn intersection(self, other: Self, plane: usize, clipping: [f32; 2]) -> Self {
        let a = self.distance(plane, clipping);
        let t = a / (a - other.distance(plane, clipping));
        let lerp = |a: f32, b: f32| (f64::from(a) + (f64::from(b) - f64::from(a)) * t) as f32;
        let mut out = Self {
            camera: std::array::from_fn(|i| lerp(self.camera[i], other.camera[i])),
            rgb: std::array::from_fn(|i| lerp(self.rgb[i], other.rgb[i])),
            uv: std::array::from_fn(|i| lerp(self.uv[i], other.uv[i])),
            local: std::array::from_fn(|i| lerp(self.local[i], other.local[i])),
            normal: std::array::from_fn(|i| lerp(self.normal[i], other.normal[i])),
            water: lerp(self.water, other.water),
        };
        out.camera[2] = match plane {
            0 | 1 => clipping[plane],
            2 => -out.camera[0],
            3 => out.camera[0],
            4 => -out.camera[1],
            5 => out.camera[1],
            _ => unreachable!(),
        };
        out
    }
}

impl Preparation {
    /// Surviving camera-space corners from the most recent successful admission.
    pub(crate) fn polygon(&self) -> &[Vertex] {
        &self.polygon
    }

    /// Test original pre-raster admission; coincident corners still count.
    pub(crate) fn admits(
        &mut self,
        vertices: impl IntoIterator<Item = Vertex>,
        clipping: [f32; 2],
    ) -> bool {
        self.polygon.clear();
        self.polygon.extend(vertices);
        let (common, union) = self
            .polygon
            .iter()
            .fold((63, 0), |(common, union), vertex| {
                let mask = vertex.outside(clipping);
                (common & mask, union | mask)
            });
        if self.polygon.len() < 3 || common != 0 {
            return false;
        }
        for plane in 0..6 {
            if union & (1 << plane) == 0 {
                continue;
            }
            self.next.clear();
            let count = self.polygon.len();
            for (i, &a) in self.polygon.iter().enumerate() {
                if a.distance(plane, clipping) >= 0.0 {
                    self.next.push(a);
                } else {
                    for j in [(i + count - 1) % count, (i + 1) % count] {
                        let b = self.polygon[j];
                        if b.distance(plane, clipping) >= 0.0 {
                            self.next.push(a.intersection(b, plane, clipping));
                        }
                    }
                }
            }
            std::mem::swap(&mut self.polygon, &mut self.next);
            if self.polygon.len() < 3 {
                return false;
            }
        }
        true
    }

    /// Finite inputs, positive near/far distances and a positive viewport are required.
    pub(super) fn triangles(
        &mut self,
        vertices: [Vertex; 3],
        clipping: [f32; 2],
        viewport: [f32; 2],
        mut emit: impl FnMut(Triangle),
    ) {
        if !self.admits(vertices, clipping) {
            return;
        }
        let a = project(self.polygon[0], viewport);
        let mut b = project(self.polygon[1], viewport);
        for &vertex in &self.polygon[2..] {
            let c = project(vertex, viewport);
            if let Some(triangle) = prepare_triangle([a, b, c]) {
                emit(triangle);
            }
            b = c;
        }
    }
}

pub(crate) fn project(vertex: Vertex, viewport: [f32; 2]) -> Projected {
    let reciprocal = f64::from(1.0 / vertex.camera[2]);
    let [half_x, half_y] = viewport.map(|v| f64::from(v) * 0.5);
    // X has a binary32 store before the common projection bias; Y does not.
    let x = (half_x * f64::from(vertex.camera[0]) * reciprocal + half_x) as f32;
    let y = half_y * f64::from(vertex.camera[1]) * reciprocal + half_y;
    Projected {
        vertex,
        screen: [
            (f64::from(x) + 786432.0) as f32 - 786432.0,
            (y + 786432.0) as f32 - 786432.0,
        ],
    }
}

fn signed24(value: f32) -> i32 {
    let integer = (f64::from(value) * 4096.0).trunc() as i64;
    ((integer as i32) << 8) >> 8
}

fn prepare_triangle(vertices: [Projected; 3]) -> Option<Triangle> {
    let [a, b, c] = vertices.map(|v| v.screen[1]);
    let order = if a < b {
        if b <= c {
            [0, 1, 2]
        } else if a < c {
            [0, 2, 1]
        } else {
            [2, 0, 1]
        }
    } else if b >= c {
        [2, 1, 0]
    } else if a < c {
        [1, 0, 2]
    } else {
        [1, 2, 0]
    };
    let [a, b, c] = order.map(|i| vertices[i]);
    let ab: [f64; 2] = std::array::from_fn(|i| f64::from(a.screen[i]) - f64::from(b.screen[i]));
    let bc: [f64; 2] = std::array::from_fn(|i| f64::from(b.screen[i]) - f64::from(c.screen[i]));
    let determinant = ab[0] * bc[1] - bc[0] * ab[1];
    if determinant == 0.0 {
        return None;
    }
    let ab = ab.map(|v| f64::from((v / determinant) as f32));
    let bc = bc.map(|v| f64::from((v / determinant) as f32));
    let anchor = a.screen.map(|v| ((v * 16.0) as i32 as i16) as i32);
    let gradient = |values: [f32; 3]| {
        let lab = f64::from(values[0]) - f64::from(values[1]);
        let lbc = f64::from(values[1]) - f64::from(values[2]);
        [
            values[0],
            (lab * bc[1] - lbc * ab[1]) as f32,
            (lbc * ab[0] - lab * bc[0]) as f32,
        ]
    };
    let subpixel = anchor.map(|v| i64::from(8 - (v & 15)));
    let planes = std::array::from_fn(|i| {
        let [start, dx, dy] = gradient([a, b, c].map(|v| v.vertex.rgb[i])).map(signed24);
        let adjustment = (subpixel[0] * i64::from(dx) + subpixel[1] * i64::from(dy)) >> 4;
        [start + adjustment as i32, dx, dy]
    });
    let attributes = [a, b, c].map(|v| {
        let w = 1.0 / v.vertex.camera[2];
        [v.vertex.uv[0] * w, v.vertex.uv[1] * w, w]
    });
    let texture = std::array::from_fn(|i| {
        let [start, dx, dy] = gradient(attributes.map(|v| v[i]))
            .map(|v| (f64::from(v) * 4294967296.0).trunc() as i64);
        let adjustment = (subpixel[0] * dx + subpixel[1] * dy) >> 4;
        [start + adjustment, dx, dy]
    });
    Some(Triangle {
        vertices,
        planes,
        texture,
        anchor: anchor.map(|v| v >> 4),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn near_clipping_interpolates_rgb_before_projecting_and_builds_a_fan() {
        let vertices = [
            Vertex {
                camera: [-2.0, -1.0, 3.0],
                rgb: [10.0, 20.0, 30.0],
                uv: [0.0, 0.0],
                ..Default::default()
            },
            Vertex {
                camera: [3.0, -1.0, 20.0],
                rgb: [100.0, 80.0, 60.0],
                uv: [1.0, 0.0],
                ..Default::default()
            },
            Vertex {
                camera: [0.0, 4.0, 30.0],
                rgb: [200.0, 180.0, 160.0],
                uv: [0.0, 1.0],
                ..Default::default()
            },
        ];
        let mut triangles = Vec::new();
        Preparation::default().triangles(vertices, [7.0, 3800.0], [640.0, 480.0], |t| {
            triangles.push(t)
        });
        assert_eq!(triangles.len(), 2);
        assert_eq!(
            triangles[0].vertices[0].vertex,
            triangles[1].vertices[0].vertex
        );
        let crossing = triangles[0].vertices[1].vertex;
        assert_eq!(crossing.camera, [-0.8235294, -1.0, 7.0]);
        assert_eq!(crossing.rgb, [31.17647, 34.117645, 37.058823]);
        assert_eq!(crossing.uv, [0.23529412, 0.0]);
        for t in triangles {
            for v in t.vertices {
                assert_eq!(v.screen.map(|x| (x * 16.0).fract()), [0.0; 2]);
            }
        }
    }

    #[test]
    fn horizontal_top_edge_uses_later_input_anchor() {
        let vertices =
            [([0.0, 0.0], 0.0), ([4.0, 0.0], 255.0), ([0.0, 4.0], 128.0)].map(|(screen, light)| {
                Projected {
                    screen,
                    vertex: Vertex {
                        camera: [0.0, 0.0, 8.0],
                        rgb: [light; 3],
                        ..Default::default()
                    },
                }
            });
        let triangle = prepare_triangle(vertices).unwrap();
        assert_eq!(triangle.anchor, [4, 0]);
        assert_eq!(triangle.planes, [[1_240_576, 261_120, 131_072]; 3]);
    }

    #[test]
    fn texture_planes_keep_original_units_and_adjust_the_top_edge_anchor() {
        let vertices = [
            ([-8.0, -8.0, 8.0], [1.0, 1.0]),
            ([0.0, -8.0, 8.0], [254.0, 1.0]),
            ([-8.0, 0.0, 8.0], [1.0, 254.0]),
        ]
        .map(|(camera, uv)| Vertex {
            camera,
            uv,
            ..Default::default()
        });
        let mut triangles = Vec::new();
        Preparation::default()
            .triangles(vertices, [7.0, 3800.0], [8.0, 8.0], |t| triangles.push(t));
        assert_eq!(triangles.len(), 1);
        assert_eq!(triangles[0].anchor, [4, 0]);
        assert_eq!(
            triangles[0].texture,
            [
                [153_343_754_240, 33_957_085_184, 0],
                [17_515_413_504, 0, 33_957_085_184],
                [536_870_912, 0, 0],
            ]
        );
    }

    #[test]
    fn outside_or_projected_degenerate_triangles_emit_nothing() {
        let outside = [Vertex {
            camera: [0.0, 0.0, 2.0],
            ..Default::default()
        }; 3];
        let degenerate =
            [[0.0, 0.0, 10.0], [0.00001, 0.0, 10.0], [0.0, 0.00001, 10.0]].map(|camera| Vertex {
                camera,
                ..Default::default()
            });
        let mut count = 0;
        let mut preparation = Preparation::default();
        for triangle in [outside, degenerate] {
            preparation.triangles(triangle, [7.0, 3800.0], [640.0, 480.0], |_| count += 1);
        }
        assert_eq!(count, 0);
    }
}
