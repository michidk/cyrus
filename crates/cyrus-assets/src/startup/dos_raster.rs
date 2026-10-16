//! DOS Glide overlay splash rendering from preservation's `docs/engine/startup.md`
//! ("DOS overlay splash"), including its projected logo shadow.

use super::raster::sample;
use bevy::math::{Mat4, Vec2, Vec3};
use image::RgbaImage;
use rgpre::import::glide_splash::{GlideSplash, SplashTexture};

const WIDTH: usize = 1280;
const HEIGHT: usize = 960;
const LIGHT: Vec3 = Vec3::splat(-0.57735);
const SHIELD: usize = 0;
const LOGO: usize = 1;
const OUTER_SHIELD: usize = 2;
const LOGO_COLOR: usize = 0;
const HIGHLIGHT: usize = 1;
const SHADOW: usize = 2;
/// Authored frames shown by full playback; frame zero is not played.
pub const FIRST_FRAME: usize = 1;

#[derive(Clone, Copy)]
struct Vertex {
    screen: Vec2,
    /// Reciprocal view depth: depth comparison and perspective-correct attributes.
    oow: f32,
    uv: Vec2,
    color: Vec3,
}

#[derive(Clone, Copy, PartialEq)]
enum Blend {
    Replace,
    Add,
    Multiply,
}

struct Canvas {
    color: Vec<Vec3>,
    depth: Vec<f32>,
}

impl Canvas {
    /// `shade` receives the interpolated vertex color and texture coordinates.
    fn triangle(
        &mut self,
        vertices: [Vertex; 3],
        depth_test: bool,
        depth_write: bool,
        blend: Blend,
        shade: impl Fn(Vec3, Vec2) -> Vec3,
    ) {
        let [a, b, c] = vertices.map(|v| v.screen);
        // Image rows grow downward, so Glide's lower-left positive winding is negative here.
        let area = (b - a).perp_dot(c - a);
        if !area.is_finite() || area >= -0.0001 || vertices.iter().any(|v| v.oow <= 0.0) {
            return;
        }
        let minimum = a.min(b).min(c).floor().max(Vec2::ZERO);
        let maximum = a
            .max(b)
            .max(c)
            .ceil()
            .min(Vec2::new(WIDTH as f32, HEIGHT as f32));
        for y in minimum.y as usize..maximum.y as usize {
            for x in minimum.x as usize..maximum.x as usize {
                let point = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                let weights = [
                    (b - point).perp_dot(c - point) / area,
                    (c - point).perp_dot(a - point) / area,
                    (a - point).perp_dot(b - point) / area,
                ];
                if weights.iter().any(|w| *w < 0.0) {
                    continue;
                }
                let oow: f32 = (0..3).map(|i| weights[i] * vertices[i].oow).sum();
                let index = y * WIDTH + x;
                if depth_test && oow + 0.0000001 < self.depth[index] {
                    continue;
                }
                let color = (0..3).map(|i| vertices[i].color * weights[i]).sum::<Vec3>();
                let uv = (0..3)
                    .map(|i| vertices[i].uv * (weights[i] * vertices[i].oow))
                    .sum::<Vec2>()
                    / oow;
                let source = shade(color, uv);
                let target = &mut self.color[index];
                *target = match blend {
                    Blend::Replace => source,
                    Blend::Add => *target + source,
                    Blend::Multiply => *target * source / 255.0,
                }
                .min(Vec3::splat(255.0));
                if depth_write {
                    self.depth[index] = oow;
                }
            }
        }
    }
}

/// Glide projection with a lower-left origin, returned in downward image rows.
fn project(position: Vec3) -> (Vec2, f32) {
    let oow = position.z.recip();
    let scale = 2.0; // supersampled 640×480 viewport
    (
        Vec2::new(
            position.x * oow * 480.0 * scale + WIDTH as f32 / 2.0,
            HEIGHT as f32 - (position.y * oow * 480.0 * scale + HEIGHT as f32 / 2.0),
        ),
        oow,
    )
}

/// Fog visibility of a 75-frame animation, as truncated 8-bit table values.
pub fn visibility(frame: usize, frames: usize) -> f32 {
    let fade_in = frames * 3 / 10;
    let fade_out = frames / 5;
    let value = if frame < fade_in {
        255 * (frame + 1) / fade_in
    } else if frame > frames - fade_out {
        255 * (frames - frame) / fade_out
    } else {
        255
    };
    value.min(255) as f32 / 255.0
}

fn tint(material: u32) -> Option<Vec3> {
    match material {
        2 => Some(Vec3::new(10.0, 75.0, 120.0) / 255.0),
        3 => Some(Vec3::ONE),
        4 => Some(Vec3::new(248.0, 204.0, 0.0) / 255.0),
        _ => None,
    }
}

/// Renders one authored frame at 640×480.
pub fn render(splash: &GlideSplash, frame: usize) -> RgbaImage {
    let frames = splash.frames.len();
    let fog = visibility(frame, frames);
    let matrices = splash.frames[frame].map(|m| Mat4::from_cols_array(&m));
    let mut canvas = Canvas {
        color: vec![Vec3::ZERO; WIDTH * HEIGHT],
        depth: vec![0.0; WIDTH * HEIGHT],
    };
    let draw_mesh = |canvas: &mut Canvas, mesh: usize, pass: u8| {
        let matrix = matrices[mesh];
        let vertices: Vec<_> = splash.meshes[mesh]
            .vertices
            .iter()
            .map(|vertex| {
                let (screen, oow) = project(matrix.transform_point3(vertex.position.into()));
                let normal = matrix.transform_vector3(vertex.normal.into());
                (screen, oow, Vec2::from(vertex.uv), normal)
            })
            .collect();
        for face in &splash.meshes[mesh].faces {
            if pass == 2 && face.material != 0 {
                continue;
            }
            let corners = face.vertices.map(|index| vertices[index]);
            let vertex = |(screen, oow, uv, normal): (Vec2, f32, Vec2, Vec3), highlight: bool| {
                Vertex {
                    screen,
                    oow,
                    uv: if highlight {
                        Vec2::splat(128.0) + normal.truncate() * 128.0
                    } else {
                        uv
                    },
                    // Unnormalized normals can leave 0..255; iterated colors clamp.
                    color: Vec3::splat((255.0 * (LIGHT.dot(normal) + 1.0) / 2.0).clamp(0.0, 255.0)),
                }
            };
            let highlight = pass == 2 || face.material == 1;
            let triangle = corners.map(|corner| vertex(corner, highlight));
            let (depth_write, blend) = match (mesh, pass) {
                (LOGO, 2) => (true, Blend::Add),
                (LOGO, _) => (true, Blend::Replace),
                _ => (false, Blend::Replace),
            };
            let texture = |index: usize| -> &SplashTexture { &splash.textures[index] };
            match (face.material, highlight) {
                (_, true) => canvas.triangle(triangle, true, depth_write, blend, |_, uv| {
                    sample(texture(HIGHLIGHT), uv) * fog
                }),
                (0, false) => canvas.triangle(triangle, true, depth_write, blend, |color, uv| {
                    (color * sample(texture(LOGO_COLOR), uv) / 255.0) * fog
                }),
                (material, false) => {
                    let Some(tint) = tint(material) else {
                        continue;
                    };
                    canvas.triangle(triangle, true, depth_write, blend, |color, _| {
                        color * tint * fog
                    });
                }
            }
        }
    };
    draw_mesh(&mut canvas, OUTER_SHIELD, 1);
    draw_mesh(&mut canvas, SHIELD, 1);
    shadow(&mut canvas, splash, &matrices);
    draw_mesh(&mut canvas, LOGO, 1);
    draw_mesh(&mut canvas, LOGO, 2);
    // shortcut: supersampling approximates Glide edge antialiasing; refine after a hardware pixel comparison.
    RgbaImage::from_fn(640, 480, |x, y| {
        let index = y as usize * 2 * WIDTH + x as usize * 2;
        let color = (canvas.color[index]
            + canvas.color[index + 1]
            + canvas.color[index + WIDTH]
            + canvas.color[index + WIDTH + 1])
            / 4.0;
        image::Rgba([color.x as u8, color.y as u8, color.z as u8, 255])
    })
}

/// Multiplies the shield by the shadow texture on a logo-space quad, projected
/// from the light onto a plane behind the shield. Fog does not apply. The splash
/// sets no STW hint, so its per-TMU W is unused and S/T follow the projected depth.
fn shadow(canvas: &mut Canvas, splash: &GlideSplash, matrices: &[Mat4; 3]) {
    let receiver = matrices[SHIELD].w_axis.z;
    let light = Vec3::new(5.0, 300.0, -1500.0 + receiver);
    let plane = receiver - 26.0;
    let corners = [
        (Vec3::new(-280.0, 0.0, -160.0), Vec2::new(10.5, 127.5)),
        (Vec3::new(-280.0, 0.0, 150.0), Vec2::new(10.5, 0.5)),
        (Vec3::new(280.0, 0.0, 150.0), Vec2::new(255.0, 0.5)),
        (Vec3::new(280.0, 0.0, -160.0), Vec2::new(255.0, 127.5)),
    ]
    .map(|(local, uv)| {
        let corner = matrices[LOGO].transform_point3(local);
        let hit = light + (corner - light) * ((plane - light.z) / (corner.z - light.z));
        let (screen, oow) = project(hit);
        Vertex {
            screen,
            oow,
            uv,
            color: Vec3::ZERO,
        }
    });
    let [a, b, c, d] = corners;
    for triangle in [[a, b, c], [a, c, d], [c, b, a], [d, c, a]] {
        canvas.triangle(triangle, false, false, Blend::Multiply, |_, uv| {
            sample(&splash.textures[SHADOW], uv)
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fog_matches_documented_fade_in_and_fade_out() {
        assert_eq!(visibility(0, 75), 11.0 / 255.0);
        assert_eq!(visibility(21, 75), 1.0);
        assert_eq!(visibility(30, 75), 1.0);
        assert_eq!(visibility(60, 75), 1.0);
        assert_eq!(visibility(61, 75), (255 * 14 / 15) as f32 / 255.0);
        assert_eq!(visibility(74, 75), 17.0 / 255.0);
    }
}
