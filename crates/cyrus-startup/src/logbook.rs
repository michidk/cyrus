//! Installed logbook paper, text and rotating scrolls over the suspended world.
use bevy::{
    asset::{AssetLoader, LoadContext, RenderAssetUsages, io::Reader},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use cyrus_assets::{logbook::LogbookArtwork, menu::MenuFace};
use cyrus_game::{AppState, logbook::LogbookScreen};
use image::RgbaImage;

#[derive(Default, TypePath)]
struct Loader;
impl AssetLoader for Loader {
    type Asset = LogbookArtwork;
    type Settings = ();
    type Error = anyhow::Error;
    async fn load(
        &self,
        reader: &mut dyn Reader,
        _: &(),
        _: &mut LoadContext<'_>,
    ) -> anyhow::Result<Self::Asset> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        let artwork: LogbookArtwork = serde_json::from_slice(&bytes)?;
        artwork.validate()?;
        Ok(artwork)
    }
    fn extensions(&self) -> &[&str] {
        &["logbook"]
    }
}
#[derive(Resource)]
struct Artwork(Handle<LogbookArtwork>);
#[derive(Component)]
struct Panel;
#[derive(Component)]
struct Page;

/// Installed in-game reader, also available when launching a scene directly.
pub struct LogbookPlugin;
impl Plugin for LogbookPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<LogbookArtwork>()
            .init_asset_loader::<Loader>()
            .init_resource::<LogbookScreen>()
            .add_systems(OnEnter(AppState::InGame), setup)
            .add_systems(OnExit(AppState::InGame), cleanup)
            .add_systems(Update, draw.run_if(in_state(AppState::InGame)));
    }
}
fn setup(mut commands: Commands, server: Res<AssetServer>) {
    commands.insert_resource(Artwork(server.load("cache://logbook/reader.logbook")));
    commands
        .spawn((
            Panel,
            GlobalZIndex(110),
            Visibility::Hidden,
            BackgroundColor(Color::BLACK),
            Node {
                width: percent(100),
                height: percent(100),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
        ))
        .with_children(|parent| {
            parent.spawn((
                Page,
                ImageNode::new(Handle::default()),
                Node {
                    height: percent(100),
                    max_width: percent(100),
                    aspect_ratio: Some(640. / 480.),
                    ..default()
                },
            ));
        });
}
fn cleanup(
    mut commands: Commands,
    panels: Query<Entity, With<Panel>>,
    mut screen: ResMut<LogbookScreen>,
) {
    for panel in &panels {
        commands.entity(panel).despawn();
    }
    // The game lifecycle releases its clock pause on the next non-game update.
    screen.ready = false;
    screen.error = None;
}
#[allow(clippy::too_many_arguments)] // Reads one asset and updates its presentation node and image.
fn draw(
    server: Res<AssetServer>,
    artwork: Res<Artwork>,
    assets: Res<Assets<LogbookArtwork>>,
    mut screen: ResMut<LogbookScreen>,
    mut images: ResMut<Assets<Image>>,
    mut panels: Query<&mut Visibility, With<Panel>>,
    mut pages: Query<&mut ImageNode, With<Page>>,
    mut previous: Local<Option<(u32, i32)>>,
) {
    let Some(artwork_data) = assets.get(&artwork.0) else {
        if let Some(bevy::asset::LoadState::Failed(error)) = server.get_load_state(&artwork.0) {
            screen.ready = false;
            screen.error = Some(format!("logbook artwork: {error}"));
        }
        return;
    };
    if !screen.ready {
        screen.bindings = artwork_data.bindings;
        screen.ready = true;
    }
    let Ok(mut panel) = panels.single_mut() else {
        return;
    };
    *panel = if screen.open {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
    if !screen.open {
        *previous = None;
        return;
    }
    let pose = (screen.offset, screen.rotation);
    if *previous == Some(pose)
        && pages
            .single()
            .is_ok_and(|page| page.image != Handle::default())
    {
        return;
    }
    let Some(book) = screen.book.as_ref() else {
        return;
    };
    match paint(artwork_data, book, screen.rotation) {
        Ok(pixels) => {
            let image = Image::new(
                Extent3d {
                    width: 640,
                    height: 480,
                    depth_or_array_layers: 1,
                },
                TextureDimension::D2,
                pixels.into_raw(),
                TextureFormat::Rgba8UnormSrgb,
                RenderAssetUsages::default(),
            );
            if let Ok(mut page) = pages.single_mut() {
                if page.image == Handle::default() {
                    page.image = images.add(image);
                } else if let Some(mut existing) = images.get_mut(&page.image) {
                    *existing = image;
                } else {
                    page.image = images.add(image);
                }
            }
            *previous = Some(pose);
        }
        Err(error) => {
            screen.error = Some(format!("logbook: {error:#}"));
            screen.open = false;
            screen.waiting_for_release = true;
        }
    }
}

/// The style table entry for an entry's prefix digit, `None` for style 5, which the original never
/// initialises and therefore draws as nothing while the entry keeps its place in the layout. A
/// missing prefix uses style 7.
fn reader_style(
    artwork: &LogbookArtwork,
    prefix: Option<u8>,
) -> Option<cyrus_assets::logbook::ReaderStyle> {
    match prefix.filter(u8::is_ascii_digit) {
        Some(digit) => artwork.styles[usize::from(digit - b'0')],
        None => artwork.styles[7],
    }
}

/// The original draws glyphs with a constant colour factor `A / 255` (255 is full strength).
pub(super) fn scale_colour(colour: [u8; 3], scale: u8) -> [u8; 3] {
    colour.map(|c| ((u32::from(c) * u32::from(scale) + 127) / 255) as u8)
}

/// Lighting inputs the reader sets for the scrolls, independent of the loaded world (preservation
/// engine/hud.md, logbook scroll lighting): a white directional light along the normalised
/// integer vector (11313, 11313, 16000), ambient 64 and two white local lights of radius 6400, all
/// in the reader's view frame. Every colour is white, so one level stands for the three channels.
const AMBIENT: f32 = 64.0;
const DIRECTION: [f32; 3] = [11313.0, 11313.0, 16000.0];
const STRENGTH: f32 = 255.0;
const LOCAL_LIGHTS: [[f32; 3]; 2] = [[-390.0, 230.0, 600.0], [390.0, 230.0, 600.0]];
const LOCAL_RADIUS: f32 = 6400.0;

/// Light level (0 to 255) at one scroll corner: ambient plus the face brightness byte plus the
/// directional term `max(0, -D.N) * S` plus each local light's `numerator / (q + 3d)` weighted by
/// `max(0, -V.N)` (`V` the normalised vertex-to-light offset, `q` its squared length, `d` its
/// length, numerator `(r^2 + 3r) / 256`), clamped to 255. `normal` is a unit vector in view space.
fn reader_light(position: [f32; 3], normal: [f32; 3], brightness: u8) -> f32 {
    let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let length = dot(DIRECTION, DIRECTION).sqrt();
    let direction = DIRECTION.map(|c| c / length);
    let mut level = AMBIENT + f32::from(brightness) + (-dot(direction, normal)).max(0.0) * STRENGTH;
    let numerator = (LOCAL_RADIUS * LOCAL_RADIUS + 3.0 * LOCAL_RADIUS) / 256.0;
    for light in LOCAL_LIGHTS {
        let offset = [0, 1, 2].map(|axis| position[axis] - light[axis]);
        let q = dot(offset, offset);
        let d = q.sqrt();
        if d == 0.0 {
            continue;
        }
        let facing = (-dot(offset.map(|c| c / d), normal)).max(0.0);
        level += facing * numerator / (q + 3.0 * d) * STRENGTH;
    }
    level.clamp(0.0, 255.0)
}

fn paint(
    artwork: &LogbookArtwork,
    book: &cyrus_game::logbook::Logbook,
    rotation: i32,
) -> anyhow::Result<RgbaImage> {
    let mut image = RgbaImage::new(640, 480);
    for y in 0..480 {
        let source_y = (y + book.offset()) % 480;
        let start = source_y as usize * 640 * 4;
        let destination = y as usize * 640 * 4;
        image.as_mut()[destination..destination + 640 * 4]
            .copy_from_slice(&artwork.paper.rgba[start..start + 640 * 4]);
    }
    for line in book.lines() {
        let y = i64::from(line.y) - i64::from(book.offset());
        if !(0..480).contains(&y) {
            continue;
        }
        let Some(style) = reader_style(artwork, line.style) else {
            continue;
        };
        let font = &artwork.fonts[style.font];
        // Like the original's draw loop: a glyph that would pass x = 640 starts a new line at the
        // entry's x, one font line lower; drawing stops at y = 480.
        let (mut pen_x, mut pen_y) = (30, y as i32);
        for ch in line.text.bytes() {
            let Some(glyph) = font.glyph(ch) else {
                continue;
            };
            if 640 < i32::from(glyph.width) + pen_x {
                pen_x = 30;
                pen_y += i32::from(font.line_height);
                if pen_y >= 480 {
                    break;
                }
            }
            font.draw_char(ch, [pen_x, pen_y], |x, y, index| {
                if !(0..640).contains(&x) || !(0..480).contains(&y) {
                    return;
                }
                let color = if style.page_palette || font.palette.is_empty() {
                    artwork.palette[usize::from(index)]
                } else {
                    scale_colour(font.colour(index), style.scale)
                };
                let alpha = u32::from(style.alpha);
                let pixel = image.get_pixel_mut(x as u32, y as u32);
                for c in 0..3 {
                    pixel[c] =
                        ((u32::from(color[c]) * alpha + u32::from(pixel[c]) * (255 - alpha) + 127)
                            / 255) as u8;
                }
                pixel[3] = 255;
            });
            pen_x += font.advance(ch);
        }
    }
    #[allow(clippy::approx_constant)] // Original table's decimal period.
    let sample = |angle: i32| (f64::from(angle & 2047) * 6.2831852 / 2048.).sin() as f32;
    let (sin, cos) = (sample(rotation), sample(rotation + 512));
    let mut faces = Vec::new();
    for y in [-212., 200.] {
        for (face_index, face) in artwork.scroll.faces.iter().enumerate() {
            let (vertices, levels): (Vec<_>, Vec<_>) = face
                .corners
                .iter()
                .enumerate()
                .map(|(index, corner)| {
                    let [x, py, z] = artwork.scroll.frames[0][corner.vertex];
                    // Second-axis quarter turn, then the reading screen's third-axis rotation.
                    let px = z - 324.;
                    let view_y = x * sin + py * cos + y;
                    let pz = 362. - x * cos + py * sin;
                    let [nx, ny, nz] = artwork.scroll_normals[face_index][index];
                    let level = reader_light(
                        [px, view_y, pz],
                        [nz, nx * sin + ny * cos, -nx * cos + ny * sin],
                        artwork.scroll_brightness[face_index],
                    );
                    (
                        [
                            320. + px / pz * (320. / 0.788),
                            240. + view_y / pz * (320. / 0.788),
                            pz.recip(),
                            corner.uv[0],
                            corner.uv[1],
                        ],
                        level,
                    )
                })
                .unzip();
            for corner in 1..vertices.len() - 1 {
                faces.push(MenuFace {
                    vertices: [vertices[0], vertices[corner], vertices[corner + 1]],
                    texture: face.texture,
                    color: face.color,
                    light: Some([levels[0], levels[corner], levels[corner + 1]]),
                });
            }
        }
    }
    let textures = artwork
        .textures
        .iter()
        .map(|texture| {
            RgbaImage::from_raw(
                u32::from(texture.width),
                u32::from(texture.height),
                texture.rgba.clone(),
            )
            .expect("validated logbook texture")
        })
        .collect::<Vec<_>>();
    super::menu_raster::render_into(&mut image, &faces, &textures)?;
    Ok(image)
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    #[test]
    fn glyph_colour_scales_by_the_constant_factor() {
        assert_eq!(scale_colour([220, 164, 132], 255), [220, 164, 132]);
        assert_eq!(scale_colour([220, 164, 132], 125), [108, 80, 65]);
        assert_eq!(scale_colour([4, 4, 4], 0), [0, 0, 0]);
    }
    #[test]
    fn reader_light_sums_ambient_directional_and_local_terms_and_clamps() {
        // Far from both local lights and facing away from the directional light, only the ambient
        // level and the face brightness byte remain.
        let far = [0.0, 0.0, 100_000.0];
        assert_eq!(reader_light(far, [0.0, 0.0, 1.0], 0), 64.0);
        assert_eq!(reader_light(far, [0.0, 0.0, 1.0], 20), 84.0);
        // Facing the directional light: 64 + 255 * 16000 / |(11313, 11313, 16000)| (about 244.3),
        // and 255 once a face brightness byte pushes it past the clamp.
        let expected =
            64.0 + 255.0 * 16000.0 / (2.0 * 11313.0_f32.powi(2) + 16000.0_f32.powi(2)).sqrt();
        assert!((reader_light(far, [0.0, 0.0, -1.0], 0) - expected).abs() < 0.01);
        assert_eq!(reader_light(far, [0.0, 0.0, -1.0], 40), 255.0);
        // A vertex coincident with a local light takes no local term.
        let on_light = reader_light([-390.0, 230.0, 600.0], [0.0, 0.0, 1.0], 0);
        assert!((on_light - 64.0).abs() < 1.0, "{on_light}");
        // A normal facing a nearby local light brightens it beyond the ambient level.
        let lit = reader_light([-390.0, 230.0, 500.0], [0.0, 0.0, 1.0], 0);
        assert!(lit > 100.0, "{lit}");
    }

    #[test]
    fn installed_reader_exports_and_draws_both_targets_from_the_same_asset() {
        let root = std::env::var_os("REDGUARD_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../redguard")
            });
        let install = match cyrus_assets::install::GameInstall::open(&root) {
            Ok(install) => install,
            Err(error) => return eprintln!("skipped: no install ({error})"),
        };
        let source = cyrus_assets::native::Files::open(&install).unwrap();
        let files = cyrus_assets::game::prepare(&source, "logbook/reader.logbook").unwrap();
        let artwork: LogbookArtwork =
            serde_json::from_slice(&files["logbook/reader.logbook"]).unwrap();
        artwork.validate().unwrap();
        assert_eq!(artwork.bindings[12], 38);
        // Preservation's reader style table: 5 uninitialised; 1 and 2 use gui_low_font; 7 and 9 use the small fonts.
        let styles: Vec<_> = artwork
            .styles
            .iter()
            .map(|style| style.map(|s| (s.font, s.scale, s.alpha)))
            .collect();
        assert_eq!(styles[1..3], [Some((4, 255, 255)); 2]);
        assert!(artwork.styles[1].is_some_and(|s| s.page_palette));
        assert!(artwork.styles[7].is_some_and(|s| !s.page_palette));
        assert_eq!(styles[5], None);
        assert!(
            reader_style(&artwork, Some(b'5')).is_none(),
            "style 5 draws nothing"
        );
        assert!(
            reader_style(&artwork, None).is_some(),
            "a missing prefix draws as style 7"
        );
        assert_eq!(styles[6], Some((2, 255, 180)));
        assert_eq!(styles[7].map(|s| (s.0, s.2)), Some((0, 200)));
        assert_eq!(styles[8].map(|s| (s.0, s.2)), Some((3, 50)));
        assert_eq!(styles[9], Some((1, 255, 255)));
        let mut book =
            cyrus_game::logbook::Logbook::new(&vec!["7,A readable journal entry.".into(); 20])
                .unwrap();
        let bottom = paint(&artwork, &book, 0).unwrap();
        book.update(cyrus_game::logbook::LogbookInput {
            home: true,
            ..default()
        });
        let top = paint(&artwork, &book, 0).unwrap();
        assert_ne!(top, bottom);
        assert_ne!(top, paint(&artwork, &book, 100).unwrap());
        assert_eq!(top.dimensions(), (640, 480));
        // Opt-in artifact for local original/native comparisons; never emitted by CI.
        if let Some(path) = std::env::var_os("CYRUS_LOGBOOK_CAPTURE") {
            top.save(path).unwrap();
        }
        // Opt-in rotated poses (the reader's third angle after Up taps) for the original lighting
        // comparison; the scroll rows are free of text, so the entry text need not match.
        if let Some(directory) = std::env::var_os("CYRUS_LOGBOOK_POSES") {
            for rotation in [0, 1988, 1928] {
                paint(&artwork, &book, rotation)
                    .unwrap()
                    .save(std::path::Path::new(&directory).join(format!("native-{rotation}.png")))
                    .unwrap();
            }
        }
    }
}
