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

/// Normal-font reader glyphs are drawn at half their palette colour; the selected font
/// keeps its colour. Observed against the original reader at 640×480: the colour weight is
/// 0.39 (half of the 200/255 alpha) for the normal font and 1.0 for the selected font.
fn reader_glyph_colour(selected: bool, colour: [u8; 3]) -> [u8; 3] {
    if selected {
        colour
    } else {
        colour.map(|c| c / 2)
    }
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
        let selected = line.style == Some(b'9');
        let font = &artwork.fonts[usize::from(selected)];
        let alpha: u8 = if selected { 255 } else { 200 };
        font.draw(&line.text, [30, y as i32], |x, y, index| {
            if !(0..640).contains(&x) || !(0..480).contains(&y) {
                return;
            }
            let color = if font.palette.is_empty() {
                artwork.palette[usize::from(index)]
            } else {
                reader_glyph_colour(selected, font.colour(index))
            };
            let pixel = image.get_pixel_mut(x as u32, y as u32);
            for c in 0..3 {
                pixel[c] = ((u32::from(color[c]) * u32::from(alpha)
                    + u32::from(pixel[c]) * u32::from(255 - alpha)
                    + 127)
                    / 255) as u8;
            }
            pixel[3] = 255;
        });
    }
    #[allow(clippy::approx_constant)] // Original table's decimal period.
    let sample = |angle: i32| (f64::from(angle & 2047) * 6.2831852 / 2048.).sin() as f32;
    let (sin, cos) = (sample(rotation), sample(rotation + 512));
    let mut faces = Vec::new();
    for y in [-212., 200.] {
        for face in &artwork.scroll.faces {
            let vertices: Vec<_> = face
                .corners
                .iter()
                .map(|corner| {
                    let [x, py, z] = artwork.scroll.frames[0][corner.vertex];
                    // Second-axis quarter turn, then the reading screen's third-axis rotation.
                    let px = z - 324.;
                    let view_y = x * sin + py * cos + y;
                    let pz = 362. - x * cos + py * sin;
                    [
                        320. + px / pz * (320. / 0.788),
                        240. + view_y / pz * (320. / 0.788),
                        pz.recip(),
                        corner.uv[0],
                        corner.uv[1],
                    ]
                })
                .collect();
            for corner in 1..vertices.len() - 1 {
                faces.push(MenuFace {
                    vertices: [vertices[0], vertices[corner], vertices[corner + 1]],
                    texture: face.texture,
                    color: face.color,
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
    fn only_the_normal_reader_font_is_halved() {
        assert_eq!(reader_glyph_colour(false, [220, 164, 132]), [110, 82, 66]);
        assert_eq!(reader_glyph_colour(true, [220, 164, 132]), [220, 164, 132]);
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
    }
}
