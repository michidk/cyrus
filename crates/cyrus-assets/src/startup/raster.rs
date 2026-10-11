//! Windows splash projection and lighting follow preservation's `docs/engine/startup.md`.

use bevy::math::{Mat4, Vec2, Vec3};
use image::RgbaImage;
use rgpre::import::{
    glide_splash::SplashTexture,
    glide_splash_dll::{WindowsGlideSplash, WindowsSplashDraw},
};

const WIDTH: usize = 1280;
const HEIGHT: usize = 960;

#[derive(Clone, Copy)]
struct Vertex {
    screen: Vec2,
    inverse_z: f32,
    uv: Vec2,
    color: Vec3,
}

struct Canvas {
    color: Vec<Vec3>,
    depth: Vec<f32>,
}

impl Canvas {
    fn triangle(&mut self, vertices: [Vertex; 3], texture: &SplashTexture, alpha: f32) {
        let [a, b, c] = vertices.map(|v| v.screen);
        let area = (b - a).perp_dot(c - a);
        if !area.is_finite()
            || area >= -0.0001
            || vertices
                .iter()
                .any(|v| !v.inverse_z.is_finite() || v.inverse_z <= 0.0)
        {
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
                let inverse_z: f32 = (0..3).map(|i| weights[i] * vertices[i].inverse_z).sum();
                let index = y * WIDTH + x;
                if inverse_z + 0.0000001 < self.depth[index] {
                    continue;
                }
                let uv = (0..3)
                    .map(|i| vertices[i].uv * (weights[i] * vertices[i].inverse_z))
                    .sum::<Vec2>()
                    / inverse_z;
                let color = (0..3).map(|i| vertices[i].color * weights[i]).sum::<Vec3>()
                    * sample(texture, uv)
                    / 255.0;
                self.color[index] = color * alpha + self.color[index] * (1.0 - alpha);
                self.depth[index] = inverse_z;
            }
        }
    }
}

fn sample(texture: &SplashTexture, uv: Vec2) -> Vec3 {
    let scale = texture.width.max(texture.height) as f32 / 256.0;
    let point = uv * scale - Vec2::splat(0.5);
    let base = point.floor();
    let fraction = point - base;
    let texel = |dx, dy| {
        let x = (base.x as i32)
            .wrapping_add(dx)
            .rem_euclid(texture.width as i32) as usize;
        let y = (base.y as i32)
            .wrapping_add(dy)
            .rem_euclid(texture.height as i32) as usize;
        let offset = (y * texture.width as usize + x) * 4;
        Vec3::new(
            texture.rgba[offset] as f32,
            texture.rgba[offset + 1] as f32,
            texture.rgba[offset + 2] as f32,
        )
    };
    texel(0, 0)
        .lerp(texel(1, 0), fraction.x)
        .lerp(texel(0, 1).lerp(texel(1, 1), fraction.x), fraction.y)
}

fn lighting(normal: Vec3, light: Vec3, view: Vec3) -> f32 {
    let dot = normal.dot(light);
    let reflection = (2.0 * normal * dot - light).normalize();
    dot.max(0.0) + reflection.dot(view).max(0.0).powi(8)
}

pub(super) fn render(
    splash: &WindowsGlideSplash,
    draws: &[WindowsSplashDraw],
    visibility: f32,
) -> RgbaImage {
    if visibility == 0.0 {
        return RgbaImage::from_pixel(640, 480, image::Rgba([0, 0, 0, 255]));
    }
    let mut canvas = Canvas {
        color: vec![Vec3::ZERO; WIDTH * HEIGHT],
        depth: vec![0.0; WIDTH * HEIGHT],
    };
    for draw in draws {
        let camera = Mat4::from_cols_array(&draw.camera);
        let matrix = camera * Mat4::from_cols_array(&draw.transform);
        let scale = WIDTH as f32 / (2.0 * (draw.horizontal_fov / 2.0).tan());
        let view = (camera.z_axis.truncate() * Vec3::new(1.0, 1.0, -1.0)).normalize();
        let mesh = &splash.meshes[draw.mesh];
        let vertices: Vec<_> = mesh
            .vertices
            .iter()
            .map(|vertex| {
                let position =
                    matrix.transform_point3(vertex.position.into()) * Vec3::new(1.0, 1.0, -1.0);
                let normal = (matrix.transform_vector3(vertex.normal.into())
                    * Vec3::new(1.0, 1.0, -1.0))
                .normalize();
                let inverse_z = position.z.recip();
                let uv = if splash.sphere_mapped[draw.mesh] {
                    let dot = normal.dot(position);
                    let discriminant = dot * dot - (position.length_squared() - 160000.0);
                    if discriminant > 0.0 {
                        let intersection = position + normal * (discriminant.sqrt() - dot);
                        (intersection.truncate() / 400.0 + Vec2::ONE) * 204.8
                    } else {
                        Vec2::ZERO
                    }
                } else {
                    Vec2::from(vertex.uv) * 256.0
                };
                let to_light = Vec3::from(draw.light_position) - position;
                let distance_squared = to_light.length_squared();
                let attenuation = (4000000.0 / distance_squared).min(1.0).powi(4);
                let positional = lighting(normal, to_light.normalize(), view) * attenuation;
                let directional = lighting(normal, Vec3::new(1.0, 1.0, -1.0) / 3.0f32.sqrt(), view);
                let color = (Vec3::from(draw.light_color) * (255.0 * positional)
                    + Vec3::splat(255.0 * directional + 16.0))
                .clamp(Vec3::ZERO, Vec3::splat(255.0));
                Vertex {
                    screen: Vec2::new(
                        position.x * inverse_z * scale + WIDTH as f32 / 2.0,
                        HEIGHT as f32 / 2.0 - position.y * inverse_z * scale,
                    ),
                    inverse_z,
                    uv,
                    color,
                }
            })
            .collect();
        for face in &mesh.faces {
            canvas.triangle(
                face.vertices.map(|index| vertices[index]),
                &splash.textures[draw.texture],
                f32::from(draw.alpha) / 255.0,
            );
        }
    }
    // shortcut: supersampling approximates Glide edge antialiasing; refine after a hardware pixel comparison.
    RgbaImage::from_fn(640, 480, |x, y| {
        let index = y as usize * 2 * WIDTH + x as usize * 2;
        let color = (canvas.color[index]
            + canvas.color[index + 1]
            + canvas.color[index + WIDTH]
            + canvas.color[index + WIDTH + 1])
            * (visibility / 4.0);
        image::Rgba([color.x as u8, color.y as u8, color.z as u8, 255])
    })
}
