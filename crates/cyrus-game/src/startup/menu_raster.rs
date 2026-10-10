//! Render the installed book's settled geometry into presentation-cache images.
//! Projection and placements follow preservation's engine/menu.md.
use super::menu_export::BookModel;
use bevy::math::{Vec2, Vec3};
use image::{Rgba, RgbaImage};
use rgpre::import::{model3d::TextureData, palette::Palette};
use std::collections::BTreeMap;

const WIDTH: usize = 640;
const HEIGHT: usize = 480;

// shortcut: settled geometry omits lighting and book motion; add them after their presentation rules are recovered.
pub(super) fn render(
    models: &[BookModel],
    textures: &BTreeMap<(u16, u16), RgbaImage>,
    palette: &Palette,
) -> anyhow::Result<RgbaImage> {
    let mut image = RgbaImage::from_pixel(WIDTH as u32, HEIGHT as u32, Rgba([0, 0, 0, 255]));
    let mut depth = vec![0.0; WIDTH * HEIGHT];
    for model in models {
        let coordinates = &model.data.frame_vertex_data[0].coords;
        for face in &model.data.face_data {
            let vertices: Vec<_> = face
                .face_vertices
                .iter()
                .map(|vertex| {
                    let point = coordinates[vertex.vertex_index as usize];
                    // All five book models use a half-turn around Y in engine coordinates.
                    let point =
                        Vec3::from_array(model.position) + Vec3::new(-point.x, point.y, -point.z);
                    let inverse_z = point.z.recip();
                    (
                        Vec2::new(320.0, 240.0) + point.truncate() * inverse_z * (320.0 / 0.788),
                        inverse_z,
                        Vec2::new(vertex.u as f32, vertex.v as f32) / 16.0,
                    )
                })
                .collect();
            for corner in 1..vertices.len().saturating_sub(1) {
                let triangle = [vertices[0], vertices[corner], vertices[corner + 1]];
                let [a, b, c] = triangle.map(|v| v.0);
                let area = (b - a).perp_dot(c - a);
                if area.abs() < 0.0001
                    || !area.is_finite()
                    || triangle.iter().any(|v| v.1 <= 0.0 || !v.1.is_finite())
                {
                    continue;
                }
                let min = a.min(b).min(c).floor().max(Vec2::ZERO);
                let max = a
                    .max(b)
                    .max(c)
                    .ceil()
                    .min(Vec2::new(WIDTH as f32, HEIGHT as f32));
                for y in min.y as usize..max.y as usize {
                    for x in min.x as usize..max.x as usize {
                        let point = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                        let weights = [
                            (b - point).perp_dot(c - point) / area,
                            (c - point).perp_dot(a - point) / area,
                            (a - point).perp_dot(b - point) / area,
                        ];
                        if weights.iter().any(|v| *v < 0.0) {
                            continue;
                        }
                        let inverse_z: f32 = (0..3).map(|i| weights[i] * triangle[i].1).sum();
                        let index = y * WIDTH + x;
                        if inverse_z + 0.0000001 < depth[index] {
                            continue;
                        }
                        let color = match face.texture_data {
                            TextureData::SolidColor(index) => {
                                let [r, g, b] = palette.colors[usize::from(index)];
                                Rgba([r, g, b, 255])
                            }
                            TextureData::Texture {
                                texture_id,
                                image_id,
                            } => {
                                let uv = (0..3)
                                    .map(|i| triangle[i].2 * (weights[i] * triangle[i].1))
                                    .sum::<Vec2>()
                                    / inverse_z;
                                sample(&textures[&(texture_id, image_id.into())], uv)
                            }
                        };
                        if color.0[3] == 0 {
                            continue;
                        }
                        image.put_pixel(x as u32, y as u32, color);
                        depth[index] = inverse_z;
                    }
                }
            }
        }
    }
    Ok(image)
}

fn sample(image: &RgbaImage, uv: Vec2) -> Rgba<u8> {
    let point = uv - Vec2::splat(0.5);
    let base = point.floor();
    let fraction = point - base;
    let texel = |dx: i32, dy: i32| {
        let x = (base.x as i32 + dx).rem_euclid(image.width() as i32) as u32;
        let y = (base.y as i32 + dy).rem_euclid(image.height() as i32) as u32;
        image.get_pixel(x, y).0.map(f32::from)
    };
    let a = texel(0, 0);
    let b = texel(1, 0);
    let c = texel(0, 1);
    let d = texel(1, 1);
    Rgba(std::array::from_fn(|i| {
        ((a[i] * (1.0 - fraction.x) + b[i] * fraction.x) * (1.0 - fraction.y)
            + (c[i] * (1.0 - fraction.x) + d[i] * fraction.x) * fraction.y)
            .round() as u8
    }))
}
