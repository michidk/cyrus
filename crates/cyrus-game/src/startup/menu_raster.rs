//! Render the installed book's authored geometry into presentation images.
//! Projection and placements follow preservation's engine/menu.md.
use super::{
    book_motion::BookPose,
    menu::MenuState,
    menu_data::{MenuAsset, MenuFace, MenuFont, MenuPicture, PageTexture},
};
use anyhow::{Context, ensure};
use bevy::math::Vec2;
use image::{Rgba, RgbaImage};

const WIDTH: usize = 640;
const HEIGHT: usize = 480;

// shortcut: book lighting and camera-plane clipping remain unsupported; add them after their rendering rules are recovered.
fn render(faces: &[MenuFace], textures: &[RgbaImage]) -> anyhow::Result<RgbaImage> {
    let mut image = RgbaImage::from_pixel(WIDTH as u32, HEIGHT as u32, Rgba([0, 0, 0, 255]));
    let mut depth = vec![0.0; WIDTH * HEIGHT];
    for face in faces {
        let triangle = face
            .vertices
            .map(|v| (Vec2::new(v[0], v[1]), v[2], Vec2::new(v[3], v[4])));
        let [a, b, c] = triangle.map(|v| v.0);
        let area = (b - a).perp_dot(c - a);
        if area >= -0.0001
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
                let color = if let Some(texture) = face.texture {
                    let uv = (0..3)
                        .map(|i| triangle[i].2 * (weights[i] * triangle[i].1))
                        .sum::<Vec2>()
                        / inverse_z;
                    sample(&textures[texture], uv)
                } else {
                    Rgba(face.color)
                };
                if color.0[3] == 0 {
                    continue;
                }
                image.put_pixel(x as u32, y as u32, color);
                depth[index] = inverse_z;
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

fn project(book: &MenuAsset, pose: &BookPose) -> Vec<MenuFace> {
    #[allow(clippy::approx_constant)] // Decimal period from the original rotation table.
    let sample = |angle: i32| (f64::from(angle & 2047) * 6.2831852 / 2048.0).sin() as f32;
    let sin = sample(pose.pitch);
    let cos = sample(pose.pitch + 512);
    let mut faces = Vec::new();
    for (index, model) in book.models.iter().enumerate() {
        for face in &model.faces {
            let vertices: Vec<_> = face
                .corners
                .iter()
                .map(|corner| {
                    let point = model.frames[pose.frames[index]][corner.vertex];
                    let x = model.position[0] - point[0] - pose.camera[0];
                    let y = model.position[1] + point[1] - pose.camera[1];
                    let z = model.position[2] + pose.depth[index] - point[2] - pose.camera[2];
                    let view_y = y * cos - z * sin;
                    let view_z = y * sin + z * cos;
                    [
                        320.0 + x / view_z * (320.0 / 0.788),
                        240.0 + view_y / view_z * (320.0 / 0.788),
                        view_z.recip(),
                        corner.uv[0],
                        corner.uv[1],
                    ]
                })
                .collect();
            for corner in 1..vertices.len().saturating_sub(1) {
                faces.push(MenuFace {
                    vertices: [vertices[0], vertices[corner], vertices[corner + 1]],
                    texture: face.texture,
                    color: face.color,
                });
            }
        }
    }
    faces
}

pub(super) fn paint(book: &MenuAsset, state: &MenuState) -> anyhow::Result<RgbaImage> {
    let preferences = state
        .preferences
        .as_ref()
        .context("menu: preferences unavailable")?;
    let mut surfaces = book.textures.clone();
    for (page_id, page) in &book.pages {
        for (index, entry) in page.entries.iter().enumerate() {
            if !entry.item.visible {
                continue;
            }
            let selected = state.selections.get(page_id).copied().flatten() == Some(index)
                && entry.item.eligible();
            let font = &book.fonts[usize::from(selected)];
            let surface = &mut surfaces[entry.texture];
            let style = if selected {
                0
            } else if !entry.item.selectable {
                3
            } else if entry.item.grayed {
                2
            } else {
                1
            };
            text(
                surface,
                font,
                &book.palette,
                &entry.item.text,
                entry.position,
                entry.justify,
                style,
            )?;
            if let Some(value) = preferences.values.get(&entry.item.action) {
                let frames = if matches!(entry.item.action, 601 | 602) {
                    &book.sliders
                } else {
                    &book.checks
                };
                let frame = if !matches!(entry.item.action, 601 | 602) {
                    *value as usize
                } else {
                    ((*value as f32 * (frames.len() - 1) as f32
                        / (entry.slider[1] - entry.slider[0]) as f32)
                        .round() as usize)
                        .min(frames.len() - 1)
                };
                picture(surface, &frames[frame], entry.output_position);
            }
            if (710..=724).contains(&entry.item.action) {
                let capturing =
                    state.page == *page_id && state.capturing == Some(entry.item.action);
                let prompt = if state.blink_on {
                    format!("<{}>", book.waiting_message)
                } else {
                    format!(">{}<", book.waiting_message)
                };
                let label = if capturing {
                    &prompt
                } else {
                    &book.key_names
                        [usize::from(preferences.bindings[(entry.item.action - 710) as usize])]
                };
                text(
                    surface,
                    &book.fonts[usize::from(capturing)],
                    &book.palette,
                    label,
                    entry.output_position,
                    entry.output_justify,
                    if capturing { 0 } else { 1 },
                )?;
            }
        }
        if *page_id == 3
            && let Some(index) = state.selections.get(page_id).copied().flatten()
        {
            let action = page.entries[index].item.action;
            if (301..=305).contains(&action)
                && let Some(frame) = book.movies.get((action - 301) as usize)
                && let Some(surface) = page.surfaces.get(1)
            {
                picture(&mut surfaces[*surface], frame, [0, 33]);
            }
        }
    }
    let textures: Vec<_> = surfaces
        .into_iter()
        .map(|s| RgbaImage::from_raw(s.width.into(), s.height.into(), s.rgba).unwrap())
        .collect();
    let motion = state
        .motion
        .as_ref()
        .context("menu: book motion unavailable")?;
    render(&project(book, &motion.pose), &textures)
}

fn picture(surface: &mut PageTexture, picture: &MenuPicture, position: [i32; 2]) {
    for y in 0..picture.height {
        for x in 0..picture.width {
            let dx = position[0] + i32::from(x);
            let dy = position[1] + i32::from(y);
            if dx < 0 || dy < 0 || dx >= i32::from(surface.width) || dy >= i32::from(surface.height)
            {
                continue;
            }
            let source = (usize::from(y) * usize::from(picture.width) + usize::from(x)) * 4;
            if picture.rgba[source + 3] == 0 {
                continue;
            }
            let target = (dy as usize * usize::from(surface.width) + dx as usize) * 4;
            surface.rgba[target..target + 4].copy_from_slice(&picture.rgba[source..source + 4]);
        }
    }
}

fn text(
    surface: &mut PageTexture,
    font: &MenuFont,
    palette: &[[u8; 3]],
    label: &str,
    position: [i32; 2],
    justify: u8,
    style: u8,
) -> anyhow::Result<()> {
    ensure!(
        label.is_ascii(),
        "menu: unsupported non-ASCII text {label:?}"
    );
    let label = label.to_ascii_uppercase();
    let glyph = |ch: u8| {
        usize::from(ch)
            .checked_sub(usize::from(font.start))
            .and_then(|i| font.glyphs.get(i))
            .filter(|g| g.enabled)
    };
    let advance = |ch| {
        glyph(ch).map_or(0, |g| {
            i32::from(g.width)
                + 1
                + if ch == b' ' {
                    font.glyphs.get(1).map_or(0, |g| i32::from(g.width))
                } else {
                    0
                }
        })
    };
    let width: i32 = label.bytes().map(advance).sum();
    let mut cursor = position[0]
        - match justify {
            1 => width / 2,
            2 => width,
            _ => 0,
        };
    for ch in label.bytes() {
        let glyph = glyph(ch)
            .with_context(|| format!("menu: text {label:?} has unsupported glyph {ch}"))?;
        for y in 0..glyph.height {
            for x in 0..glyph.width {
                let pixel =
                    glyph.pixels[usize::from(y) * usize::from(glyph.width) + usize::from(x)];
                if pixel == 0 {
                    continue;
                }
                let dx = cursor + glyph.offset[0] + i32::from(x);
                let dy = position[1] + glyph.offset[1] + i32::from(y);
                if dx < 0
                    || dy < 0
                    || dx >= i32::from(surface.width)
                    || dy >= i32::from(surface.height)
                {
                    continue;
                }
                let offset = dy as usize * usize::from(surface.width) + dx as usize;
                let index = match style {
                    0 => pixel,
                    2 => surface.indices[offset].wrapping_add(4),
                    3 => (pixel & 15) + 144,
                    _ => pixel.wrapping_add(5),
                };
                surface.indices[offset] = index;
                let [r, g, b] = palette[usize::from(index)];
                surface.rgba[offset * 4..offset * 4 + 4].copy_from_slice(&[r, g, b, 255]);
            }
        }
        cursor += advance(ch);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn back_of_a_turned_page_does_not_cover_its_visible_surface() {
        let faces = vec![
            MenuFace {
                vertices: [
                    [0., 0., 1., 0., 0.],
                    [0., 5., 1., 0., 0.],
                    [5., 0., 1., 0., 0.],
                ],
                texture: None,
                color: [255, 0, 0, 255],
            },
            MenuFace {
                vertices: [
                    [0., 0., 2., 0., 0.],
                    [5., 0., 2., 0., 0.],
                    [0., 5., 2., 0., 0.],
                ],
                texture: None,
                color: [0, 0, 255, 255],
            },
        ];
        let image = render(&faces, &[]).unwrap();
        assert_eq!(image.get_pixel(1, 1).0, [255, 0, 0, 255]);
        assert_eq!(image.get_pixel(6, 6).0, [0, 0, 0, 255]);
    }
}
