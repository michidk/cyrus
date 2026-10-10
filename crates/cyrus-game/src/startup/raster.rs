//! Projection and materials follow preservation's `docs/engine/startup.md`.

use bevy::math::{Mat4, Vec2, Vec3};
use image::RgbaImage;
use rgpre::import::glide_splash::{GlideSplash, SplashTexture};

const WIDTH: usize = 1280;
const HEIGHT: usize = 960;

#[derive(Clone, Copy)]
struct Vertex {
    screen: Vec2,
    inverse_z: f32,
    uv: Vec2,
    normal: Vec3,
    brightness: f32,
}

impl Vertex {
    fn projected(position: Vec3, uv: Vec2, normal: Vec3) -> Self {
        let inverse_z = position.z.recip();
        Self {
            screen: Vec2::new(
                (position.x * inverse_z * 0.75 + 0.5) * WIDTH as f32,
                (0.5 - position.y * inverse_z) * HEIGHT as f32,
            ),
            inverse_z,
            uv,
            normal,
            brightness: (normal.dot(Vec3::splat(-0.57735)) + 1.0) * 0.5,
        }
    }
}

#[derive(Clone, Copy)]
enum Pass {
    Surface(u32),
    Highlight,
    Shadow,
}

struct Canvas {
    color: Vec<Vec3>,
    depth: Vec<f32>,
}

impl Canvas {
    fn triangle(
        &mut self,
        vertices: [Vertex; 3],
        pass: Pass,
        splash: &GlideSplash,
        fade: f32,
        write_depth: bool,
    ) {
        let [a, b, c] = vertices.map(|v| v.screen);
        let area = (b - a).perp_dot(c - a);
        // The lower-left Glide viewport reverses winding relative to this image.
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
                if !matches!(pass, Pass::Shadow) && inverse_z + 0.0000001 < self.depth[index] {
                    continue;
                }
                let uv: Vec2 = (0..3)
                    .map(|i| vertices[i].uv * (weights[i] * vertices[i].inverse_z))
                    .sum::<Vec2>()
                    / inverse_z;
                let brightness: f32 = (0..3).map(|i| weights[i] * vertices[i].brightness).sum();
                let color = match pass {
                    Pass::Shadow => sample(&splash.textures[2], uv),
                    Pass::Highlight | Pass::Surface(1) => sample(&splash.textures[1], uv) * fade,
                    Pass::Surface(0) => sample(&splash.textures[0], uv) * brightness * fade,
                    Pass::Surface(2) => Vec3::new(10.0, 75.0, 120.0) * brightness * fade,
                    Pass::Surface(3) => Vec3::splat(255.0 * brightness * fade),
                    Pass::Surface(4) => Vec3::new(248.0, 204.0, 0.0) * brightness * fade,
                    Pass::Surface(_) => unreachable!("parser validates materials"),
                };
                self.color[index] = match pass {
                    Pass::Shadow => self.color[index] * color / 255.0,
                    Pass::Highlight => self.color[index] + color,
                    Pass::Surface(_) => color,
                }
                .clamp(Vec3::ZERO, Vec3::splat(255.0));
                if write_depth {
                    self.depth[index] = inverse_z;
                }
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

pub(super) fn render(splash: &GlideSplash, frame: usize) -> RgbaImage {
    let mut canvas = Canvas {
        color: vec![Vec3::ZERO; WIDTH * HEIGHT],
        depth: vec![0.0; WIDTH * HEIGHT],
    };
    let matrices = splash.frames[frame].map(|m| Mat4::from_cols_array(&m));
    let fade = if frame < 22 {
        ((255 * (frame + 1) / 22) as f32) / 255.0
    } else if frame > 60 {
        ((255 * (75 - frame) / 15) as f32) / 255.0
    } else {
        1.0
    };
    let transformed: [Vec<Vertex>; 3] = std::array::from_fn(|mesh| {
        splash.meshes[mesh]
            .vertices
            .iter()
            .map(|vertex| {
                Vertex::projected(
                    matrices[mesh].transform_point3(vertex.position.into()),
                    vertex.uv.into(),
                    matrices[mesh].transform_vector3(vertex.normal.into()),
                )
            })
            .collect::<Vec<_>>()
    });
    for mesh in [2, 0] {
        for face in &splash.meshes[mesh].faces {
            canvas.triangle(
                face.vertices.map(|i| transformed[mesh][i]),
                Pass::Surface(face.material),
                splash,
                fade,
                false,
            );
        }
    }
    let shield_z = matrices[0].w_axis.z;
    let light = Vec3::new(5.0, 300.0, shield_z - 1500.0);
    let corners = [
        Vec3::new(-280.0, 0.0, -160.0),
        Vec3::new(-280.0, 0.0, 150.0),
        Vec3::new(280.0, 0.0, 150.0),
        Vec3::new(280.0, 0.0, -160.0),
    ];
    let uv = [
        Vec2::new(10.5, 127.5),
        Vec2::new(10.5, 0.5),
        Vec2::new(255.0, 0.5),
        Vec2::new(255.0, 127.5),
    ];
    // shortcut: shadows use reciprocal Z; add per-TMU W when its effective sampling behavior is verified.
    let shadow = std::array::from_fn::<_, 4, _>(|i| {
        let direction = matrices[1].transform_point3(corners[i]) - light;
        let position = light + direction * ((shield_z - 26.0 - light.z) / direction.z);
        Vertex::projected(position, uv[i], Vec3::ZERO)
    });
    for indices in [[0, 1, 2], [0, 2, 3], [0, 2, 1], [0, 3, 2]] {
        canvas.triangle(indices.map(|i| shadow[i]), Pass::Shadow, splash, 1.0, true);
    }
    for face in &splash.meshes[1].faces {
        let mut triangle = face.vertices.map(|i| transformed[1][i]);
        if face.material == 1 {
            for vertex in &mut triangle {
                vertex.uv = Vec2::splat(128.0) + vertex.normal.truncate() * 128.0;
            }
        }
        canvas.triangle(triangle, Pass::Surface(face.material), splash, fade, true);
    }
    for face in splash.meshes[1]
        .faces
        .iter()
        .filter(|face| face.material == 0)
    {
        let triangle = face.vertices.map(|i| {
            let mut vertex = transformed[1][i];
            vertex.uv = Vec2::splat(128.0) + vertex.normal.truncate() * 128.0;
            vertex
        });
        canvas.triangle(triangle, Pass::Highlight, splash, fade, true);
    }
    // shortcut: supersampling replaces Glide edge coverage and uses the largest mip; refine after a paired pixel comparison.
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
