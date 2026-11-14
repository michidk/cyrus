//! Installed logbook paper, text and rotating scrolls over the suspended world.
use super::view3d::{LitAssets, LitFace, LitMesh, LitModel, LitScene, target_image, view_camera};
use bevy::{
    asset::{AssetLoader, LoadContext, RenderAssetUsages, io::Reader},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use cyrus_assets::{
    logbook::LogbookArtwork,
    menu::{ConfirmationArtwork, MenuAsset},
};
use cyrus_game::AppState;
use cyrus_script::logbook::LogbookScreen;
use cyrus_world::heading::wrap;
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
struct Artwork {
    reader: Handle<LogbookArtwork>,
    menu: Handle<MenuAsset>,
}
#[derive(PartialEq, Eq)]
struct ReaderFrame {
    offset: u32,
    rotation: i32,
    entry: Option<String>,
    cursor: bool,
}
#[derive(Component)]
struct Panel;
#[derive(Component)]
struct Page;

/// Installed in-game reader, also available when launching a scene directly.
pub struct LogbookPlugin;
impl Plugin for LogbookPlugin {
    fn build(&self, app: &mut App) {
        super::menu::register_assets(app);
        app.init_asset::<LogbookArtwork>()
            .init_asset_loader::<Loader>()
            .init_resource::<LogbookScreen>()
            .add_systems(OnEnter(AppState::InGame), setup)
            .add_systems(OnExit(AppState::InGame), cleanup)
            .add_systems(Update, draw.run_if(in_state(AppState::InGame)));
    }
}
/// Render layer of the reader's scroll scene.
const READER_LAYER: usize = 6;

#[derive(Component)]
struct ReaderCamera;

/// The panel and camera `cleanup` removes.
type Teardown = Or<(With<Panel>, With<ReaderCamera>)>;
/// The entry dialog layer's image and visibility.
type EntryNode = (&'static mut ImageNode, &'static mut Visibility);
/// The transparent layer the manual-entry dialog is drawn on, above the scrolls.
#[derive(Component)]
struct EntryLayer;

fn setup(mut commands: Commands, server: Res<AssetServer>, mut images: ResMut<Assets<Image>>) {
    commands.insert_resource(Artwork {
        reader: server.load("cache://logbook/reader.logbook"),
        menu: server.load("cache://menu/book.menu"),
    });
    let target = images.add(target_image([0, 0, 0, 0]));
    commands.spawn((
        ReaderCamera,
        view_camera(target.clone(), READER_LAYER, Color::NONE),
    ));
    // The paper and text, the scrolls and the entry dialog stack in the same 640 by 480 frame.
    let layer = || Node {
        position_type: PositionType::Absolute,
        left: px(0),
        top: px(0),
        width: percent(100),
        height: percent(100),
        ..default()
    };
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
            parent
                .spawn((
                    Page,
                    ImageNode::new(Handle::default()),
                    Node {
                        height: percent(100),
                        max_width: percent(100),
                        aspect_ratio: Some(640. / 480.),
                        ..default()
                    },
                ))
                .with_children(|page| {
                    page.spawn((ImageNode::new(target), layer()));
                    page.spawn((
                        EntryLayer,
                        ImageNode::new(Handle::default()),
                        Visibility::Hidden,
                        layer(),
                    ));
                });
        });
}
fn cleanup(
    mut commands: Commands,
    panels: Query<Entity, Teardown>,
    meshes: Query<(Entity, &LitMesh)>,
    mut screen: ResMut<LogbookScreen>,
) {
    for entity in &panels {
        commands.entity(entity).despawn();
    }
    for (entity, mesh) in &meshes {
        if mesh.0 == READER_LAYER {
            commands.entity(entity).despawn();
        }
    }
    // The game lifecycle releases its clock pause on the next non-game update.
    screen.ready = false;
    screen.error = None;
}

/// Uploads `pixels` into the image an [`ImageNode`] shows, creating it on first use.
fn upload(images: &mut Assets<Image>, node: &mut ImageNode, pixels: RgbaImage) {
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
    if node.image == Handle::default() {
        node.image = images.add(image);
    } else if let Some(mut existing) = images.get_mut(&node.image) {
        *existing = image;
    } else {
        node.image = images.add(image);
    }
}

#[allow(clippy::too_many_arguments)] // Reads one asset and updates its presentation nodes and scene.
fn draw(
    server: Res<AssetServer>,
    artwork: Res<Artwork>,
    assets: Res<Assets<LogbookArtwork>>,
    menus: Res<Assets<MenuAsset>>,
    mut screen: ResMut<LogbookScreen>,
    mut commands: Commands,
    mut assets_3d: LitAssets,
    mut panels: Query<&mut Visibility, (With<Panel>, Without<EntryLayer>)>,
    mut pages: Query<&mut ImageNode, (With<Page>, Without<EntryLayer>)>,
    mut entries: Query<EntryNode, (With<EntryLayer>, Without<Panel>)>,
    mut cameras: Query<&mut Camera, With<ReaderCamera>>,
    mut model: Local<LitModel>,
    mut previous: Local<Option<ReaderFrame>>,
) {
    let Some(artwork_data) = assets.get(&artwork.reader) else {
        if let Some(bevy::asset::LoadState::Failed(error)) = server.get_load_state(&artwork.reader)
        {
            screen.ready = false;
            screen.error = Some(format!("logbook artwork: {error}"));
        }
        return;
    };
    if !screen.ready {
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
    for mut camera in &mut cameras {
        camera.is_active = screen.open;
    }
    if !screen.open {
        *previous = None;
        model.clear(&mut commands);
        return;
    }
    let pose = ReaderFrame {
        offset: screen.offset,
        rotation: screen.rotation,
        entry: screen.manual_entry().map(str::to_owned),
        cursor: screen.manual_entry_cursor_visible(),
    };
    if previous.as_ref() == Some(&pose)
        && pages
            .single()
            .is_ok_and(|page| page.image != Handle::default())
    {
        return;
    }
    let Some(book) = screen.book.as_ref() else {
        return;
    };
    let dialog = if pose.entry.is_some() {
        let Some(menu) = menus.get(&artwork.menu) else {
            if let Some(bevy::asset::LoadState::Failed(error)) =
                server.get_load_state(&artwork.menu)
            {
                screen.error = Some(format!("logbook entry artwork: {error}"));
            }
            return;
        };
        Some(&menu.confirmation)
    } else {
        None
    };
    if let Ok(mut page) = pages.single_mut() {
        upload(
            &mut assets_3d.images,
            &mut page,
            paint_paper(artwork_data, book),
        );
    }
    model.show(
        &mut commands,
        &mut assets_3d,
        &build_scene(artwork_data, screen.rotation),
        READER_LAYER,
        0,
    );
    if let Ok((mut node, mut visibility)) = entries.single_mut() {
        *visibility = Visibility::Hidden;
        if let (Some(text), Some(dialog)) = (&pose.entry, dialog) {
            let mut overlay = RgbaImage::new(640, 480);
            paint_entry(&mut overlay, dialog, text, pose.cursor);
            upload(&mut assets_3d.images, &mut node, overlay);
            *visibility = Visibility::Inherited;
        }
    }
    *previous = Some(pose);
}

/// Display-copy wrapping follows preservation's manual-entry dialog rules. CR/VT stay bytes;
/// only spaces selected by the original's character-offset scan become visible line breaks.
fn entry_display(text: &str, cursor: bool) -> String {
    let mut display = text.as_bytes().to_vec();
    if cursor {
        display.push(b'_');
    }
    let mut index = 13;
    while index + 1 < display.len() {
        if display[index] == b' ' {
            display[index] = b'\n';
            index += 14;
        } else {
            index += 1;
        }
    }
    // The game-owned editor guarantees ASCII, including its CR/VT insertions.
    display.into_iter().map(char::from).collect()
}

/// Paints over a freshly restored opaque reader frame, never over a previous dialog redraw.
/// Layout, font styles and backing alpha follow preservation's manual logbook entry section.
fn paint_entry(image: &mut RgbaImage, artwork: &ConfirmationArtwork, text: &str, cursor: bool) {
    let picture = &artwork.picture;
    for y in 140..=(140 + u32::from(picture.height)).min(479) {
        for x in 175..=(175 + u32::from(picture.width)).min(639) {
            entry_pixel(image, x as i32, y as i32, [10; 3], 125);
        }
    }
    for y in 0..u32::from(picture.height) {
        for x in 0..u32::from(picture.width) {
            let at = ((y * u32::from(picture.width) + x) * 4) as usize;
            let pixel = &picture.rgba[at..at + 4];
            entry_pixel(
                image,
                x as i32 + 175,
                y as i32 + 140,
                [pixel[0], pixel[1], pixel[2]],
                pixel[3],
            );
        }
    }
    let font = &artwork.font;
    let mut line = |text: &str, x: i32, y: i32, style: usize| {
        let [scale, alpha] = artwork.styles[style];
        let mut pen = x;
        let mut row = y;
        for ch in text.bytes() {
            if ch == b'\n' {
                pen = x;
                row += i32::from(font.line_height);
                if row >= 640 {
                    break;
                }
                continue;
            }
            // CR has no visible width, but keeps the shared wrap and one-pixel advance.
            let width = if ch == b'\r' {
                0
            } else if let Some(glyph) = font.glyph(ch) {
                i32::from(glyph.width)
            } else {
                continue;
            };
            if pen + width > 450 {
                pen = x;
                row += i32::from(font.line_height);
                if row >= 640 {
                    break;
                }
            }
            if ch == b'\r' {
                pen += 1;
                continue;
            }
            for (offset, shadow) in [(1, true), (0, false)] {
                font.draw_char(ch, [pen + offset, row + offset], |x, y, index| {
                    let colour = if shadow {
                        [0; 3]
                    } else {
                        scale_colour(font.colour(index), scale)
                    };
                    let alpha = if shadow {
                        (u16::from(alpha) * 200 / 256) as u8
                    } else {
                        alpha
                    };
                    entry_pixel(image, x, y, colour, alpha);
                });
            }
            pen += font.advance(ch);
        }
    };
    line("Log Entry", 320 - font.width("Log Entry") / 2, 170, 0);
    line(&entry_display(text, cursor), 200, 200, 1);
}

fn entry_pixel(image: &mut RgbaImage, x: i32, y: i32, colour: [u8; 3], alpha: u8) {
    super::confirmation::blend(image, x, y, colour, alpha);
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

fn paint_paper(artwork: &LogbookArtwork, book: &cyrus_script::logbook::Logbook) -> RgbaImage {
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
                // The shared glyph rule disables blending at B >= 254.
                let alpha = u32::from(if style.alpha >= 254 { 255 } else { style.alpha });
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
    image
}

/// The two rotating scrolls as lit triangles in the reader's view space.
fn build_scene(artwork: &LogbookArtwork, rotation: i32) -> LitScene {
    #[allow(clippy::approx_constant)] // Original table's decimal period.
    let sample = |angle: i32| (f64::from(wrap(angle)) * 6.2831852 / 2048.).sin() as f32;
    let (sin, cos) = (sample(rotation), sample(rotation + 512));
    let mut scene = LitScene::default();
    for texture in &artwork.textures {
        scene
            .textures
            .push((texture.width, texture.height, texture.rgba.clone()));
    }
    for y in [-212., 200.] {
        for (face_index, face) in artwork.scroll.faces.iter().enumerate() {
            let corners: Vec<([f32; 3], [f32; 2], f32)> = face
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
                    ([px, view_y, pz], corner.uv, level)
                })
                .collect();
            // A textured face is lit by its corner levels. A solid-colour face is drawn unlit, as
            // the original's draw ignores the single level for it.
            let (texture, bias) = match face.texture {
                Some(texture) => (texture, 1.0),
                None => (scene.solid_texture(face.color), 1.0),
            };
            for corner in 1..corners.len() - 1 {
                let triangle = [corners[0], corners[corner], corners[corner + 1]];
                scene.faces.push(LitFace {
                    vertices: triangle.map(|corner| corner.0),
                    uv: triangle.map(|corner| corner.1),
                    levels: triangle.map(|corner| {
                        if face.texture.is_some() {
                            [corner.2; 3]
                        } else {
                            [255.0; 3]
                        }
                    }),
                    bias,
                    texture,
                });
            }
        }
    }
    scene
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    fn standalone_asset_app(files: &[(String, Vec<u8>)], menu_first: bool) -> App {
        use bevy::asset::io::{
            AssetSourceBuilder,
            memory::{Dir, MemoryAssetReader},
        };
        let directory = Dir::default();
        for (path, bytes) in files {
            directory.insert_asset(std::path::Path::new(path), bytes.clone());
        }
        let mut app = App::new();
        app.register_asset_source(
            "cache",
            AssetSourceBuilder::new(move || {
                Box::new(MemoryAssetReader {
                    root: directory.clone(),
                })
            }),
        )
        .add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            bevy::state::app::StatesPlugin,
        ))
        .init_state::<AppState>()
        .init_asset::<Image>();
        if menu_first {
            super::super::menu::register(&mut app);
        }
        app.add_plugins(LogbookPlugin);
        app.world_mut()
            .resource_mut::<NextState<AppState>>()
            .set(AppState::InGame);
        app.update();
        app
    }

    #[test]
    fn standalone_logbook_requests_typed_menu_and_reports_invalid_cache_without_panicking() {
        let mut app = standalone_asset_app(
            &[("menu/book.menu".into(), b"invalid menu".to_vec())],
            false,
        );
        let handle = app.world().resource::<Artwork>().menu.clone();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            app.update();
            if let Some(bevy::asset::LoadState::Failed(error)) = app
                .world()
                .resource::<AssetServer>()
                .get_load_state(&handle)
            {
                assert!(error.to_string().contains("menu/book.menu"), "{error}");
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "typed menu load did not finish"
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    #[test]
    fn installed_standalone_logbook_loads_menu_and_shared_registration_preserves_it() {
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
        let mut files = cyrus_assets::game::prepare(&source, "menu/book.menu").unwrap();
        files.extend(cyrus_assets::game::prepare(&source, "logbook/reader.logbook").unwrap());
        let files = files.into_iter().collect::<Vec<_>>();
        for menu_first in [false, true] {
            let mut app = standalone_asset_app(&files, menu_first);
            let handle = app.world().resource::<Artwork>().menu.clone();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
            loop {
                app.update();
                if app
                    .world()
                    .resource::<Assets<MenuAsset>>()
                    .get(&handle)
                    .is_some()
                    && app.world().resource::<LogbookScreen>().ready
                {
                    break;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "standalone artwork load did not finish"
                );
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            assert_eq!(
                app.world()
                    .resource::<Assets<MenuAsset>>()
                    .get(&handle)
                    .unwrap()
                    .version,
                cyrus_assets::menu::MENU_VERSION
            );
            if !menu_first {
                super::super::menu::register(&mut app);
            }
            super::super::menu::register_assets(&mut app);
            assert!(
                app.world()
                    .resource::<Assets<MenuAsset>>()
                    .get(&handle)
                    .is_some()
            );
        }
    }

    #[test]
    fn installed_reader_publication_preserves_opaque_glyph_alpha_boundary() {
        let root = std::env::var_os("REDGUARD_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../redguard")
            });
        let Ok(install) = cyrus_assets::install::GameInstall::open(&root) else {
            eprintln!("skipped: no install (reader alpha boundary)");
            return;
        };
        let source = cyrus_assets::native::Files::open(&install).unwrap();
        let mut files = cyrus_assets::game::prepare(&source, "menu/book.menu").unwrap();
        files.extend(cyrus_assets::game::prepare(&source, "logbook/reader.logbook").unwrap());
        let mut artwork: LogbookArtwork =
            serde_json::from_slice(&files["logbook/reader.logbook"]).unwrap();
        for alpha in [254, 253, 255] {
            artwork.styles[0].as_mut().unwrap().alpha = alpha;
            files.insert(
                "logbook/reader.logbook".into(),
                serde_json::to_vec(&artwork).unwrap(),
            );
            let mut app =
                standalone_asset_app(&files.clone().into_iter().collect::<Vec<_>>(), false);
            {
                let mut screen = app.world_mut().resource_mut::<LogbookScreen>();
                screen.book = Some(cyrus_script::logbook::Logbook::new(&["0,A".into()]).unwrap());
                screen.open = true;
            }
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
            let data = loop {
                app.update();
                let handle = app
                    .world_mut()
                    .query_filtered::<&ImageNode, With<Page>>()
                    .single(app.world())
                    .unwrap()
                    .image
                    .clone();
                if let Some(image) = app.world().resource::<Assets<Image>>().get(&handle) {
                    break image.data.as_ref().unwrap().clone();
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "reader publication timed out"
                );
                std::thread::sleep(std::time::Duration::from_millis(1));
            };
            let style = artwork.styles[0].unwrap();
            let font = &artwork.fonts[style.font];
            let mut checked = 0;
            font.draw_char(b'A', [30, 70], |x, y, index| {
                let at = (y as usize * 640 + x as usize) * 4;
                let colour = scale_colour(font.colour(index), style.scale);
                let expected = if alpha >= 254 {
                    colour
                } else {
                    std::array::from_fn(|c| {
                        ((u32::from(colour[c]) * u32::from(alpha)
                            + u32::from(artwork.paper.rgba[at + c]) * u32::from(255 - alpha)
                            + 127)
                            / 255) as u8
                    })
                };
                assert_eq!(
                    &data[at..at + 3],
                    &expected,
                    "published reader alpha {alpha}, pixel {x},{y}"
                );
                checked += 1;
            });
            assert!(checked > 0);
            eprintln!("reader alpha {alpha}: {checked} published installed glyph pixels exact");
        }
    }

    #[test]
    fn entry_display_wraps_a_copy_and_counts_the_cursor_without_replacing_controls() {
        let text = "ABCDEFGHIJKLM ";
        assert_eq!(entry_display(text, false), text);
        assert_eq!(entry_display(text, true), "ABCDEFGHIJKLM\n_");
        assert_eq!(text, "ABCDEFGHIJKLM ");
        assert_eq!(entry_display("F\u{b}G\rH", true), "F\u{b}G\rH_");
        assert_eq!(
            entry_display(&"A".repeat(64), true),
            format!("{}_", "A".repeat(64))
        );
        assert_eq!(
            entry_display("ABCDEFGHIJKLM NOPQRSTUVWXYZ ABCDEFGHIJKLM N", false),
            "ABCDEFGHIJKLM\nNOPQRSTUVWXYZ\nABCDEFGHIJKLM\nN"
        );
    }

    #[test]
    fn entry_painter_places_text_and_cursor_and_restores_the_reader() {
        let font = cyrus_text::Font {
            start: 32,
            line_height: 8,
            glyphs: vec![
                cyrus_text::Glyph {
                    enabled: true,
                    width: 1,
                    height: 1,
                    offset: [0, 0],
                    pixels: vec![1],
                };
                96
            ],
            palette: vec![[0; 3], [63; 3]],
        };
        let artwork = ConfirmationArtwork {
            picture: cyrus_assets::menu::MenuPicture {
                width: 2,
                height: 2,
                rgba: vec![0; 16],
            },
            font,
            styles: [[128, 255], [255, 255]],
        };
        let background = RgbaImage::from_pixel(640, 480, image::Rgba([100, 100, 100, 255]));
        let mut entry = background.clone();
        paint_entry(&mut entry, &artwork, "A", true);
        assert_eq!(entry.get_pixel(200, 200).0, [252, 252, 252, 255]);
        assert_eq!(entry.get_pixel(202, 200).0, [252, 252, 252, 255]);
        assert_eq!(entry.get_pixel(311, 170).0, [126, 126, 126, 255]);
        assert_eq!(entry.get_pixel(177, 142).0, [56, 56, 56, 255]);
        assert_eq!(entry.get_pixel(178, 142).0, [100, 100, 100, 255]);
        let mut hidden = background.clone();
        paint_entry(&mut hidden, &artwork, "A", false);
        assert_eq!(hidden.get_pixel(202, 200).0, [100, 100, 100, 255]);
        assert_eq!(hidden.get_pixel(201, 201).0, [22, 22, 22, 255]);
        assert_eq!(hidden.get_pixel(202, 202).0, [100, 100, 100, 255]);
        let mut controls = background.clone();
        paint_entry(&mut controls, &artwork, "F\u{b}GH", false);
        let mut plain = background.clone();
        paint_entry(&mut plain, &artwork, "FGH", false);
        assert_eq!(controls, plain);
        let mut carriage_return = background.clone();
        paint_entry(&mut carriage_return, &artwork, "H\rI", false);
        assert_eq!(carriage_return.get_pixel(200, 200).0, [252, 252, 252, 255]);
        assert_eq!(carriage_return.get_pixel(202, 200).0, [100, 100, 100, 255]);
        assert_eq!(carriage_return.get_pixel(203, 200).0, [252, 252, 252, 255]);
        assert_eq!(background.get_pixel(175, 140).0, [100, 100, 100, 255]);
        let mut wide = artwork;
        wide.font.glyphs[usize::from(b'A' - b' ')].width = 14;
        wide.font.glyphs[usize::from(b'A' - b' ')].pixels = vec![1; 14];
        let mut wrapped = background.clone();
        paint_entry(&mut wrapped, &wide, &"A".repeat(64), false);
        for y in [200, 208, 216, 224] {
            assert_eq!(wrapped.get_pixel(200, y).0, [252, 252, 252, 255]);
            assert_eq!(wrapped.get_pixel(438, y).0, [252, 252, 252, 255]);
            assert_eq!(wrapped.get_pixel(440, y).0, [100, 100, 100, 255]);
        }
        wide.font.glyphs[usize::from(b'A' - b' ')].width = 249;
        wide.font.glyphs[usize::from(b'A' - b' ')].pixels = vec![1; 249];
        let mut carriage_return_wrap = background.clone();
        paint_entry(&mut carriage_return_wrap, &wide, "A\r\rB", false);
        assert_eq!(
            carriage_return_wrap.get_pixel(200, 208).0,
            [100, 100, 100, 255]
        );
        assert_eq!(
            carriage_return_wrap.get_pixel(201, 208).0,
            [252, 252, 252, 255]
        );
    }

    #[test]
    fn installed_entry_painter_uses_portable_menu_artwork() {
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
        let files = cyrus_assets::game::prepare(&source, "menu/book.menu").unwrap();
        let menu: MenuAsset = serde_json::from_slice(&files["menu/book.menu"]).unwrap();
        menu.validate().unwrap();
        let background = std::env::var_os("CYRUS_LOGBOOK_ENTRY_BACKGROUND").map_or_else(
            || RgbaImage::from_pixel(640, 480, image::Rgba([100, 100, 100, 255])),
            |path| image::open(path).unwrap().into_rgba8(),
        );
        assert_eq!(background.dimensions(), (640, 480));
        for (name, text) in [
            ("empty", ""),
            ("ac", "AC"),
            ("de", "DE"),
            ("vt", "F\u{b}G"),
            ("cr", "H\rI"),
            ("fg", "FG"),
            ("hi", "HI"),
            (
                "boundary",
                "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAB",
            ),
        ] {
            for cursor in [false, true] {
                let mut pixels = background.clone();
                paint_entry(&mut pixels, &menu.confirmation, text, cursor);
                assert_ne!(pixels, background);
                assert_eq!(pixels.get_pixel(0, 0), background.get_pixel(0, 0));
                // Optional exact painter output for retained original-dialog comparisons.
                if let Some(directory) = std::env::var_os("CYRUS_LOGBOOK_ENTRY_CAPTURES") {
                    pixels
                        .save(std::path::Path::new(&directory).join(format!("{name}-{cursor}.png")))
                        .unwrap();
                }
            }
        }
    }

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
            cyrus_script::logbook::Logbook::new(&vec!["7,A readable journal entry.".into(); 20])
                .unwrap();
        let bottom = paint_paper(&artwork, &book);
        book.update(cyrus_script::logbook::LogbookInput {
            home: true,
            ..default()
        });
        let top = paint_paper(&artwork, &book);
        assert_ne!(top, bottom);
        assert_eq!(top.dimensions(), (640, 480));
        // The scrolls turn with the reader's third angle, and their corners are lit.
        let still = build_scene(&artwork, 0);
        let turned = build_scene(&artwork, 100);
        assert!(!still.faces.is_empty());
        assert_eq!(still.faces.len(), turned.faces.len());
        assert_ne!(still.faces[0].vertices, turned.faces[0].vertices);
        assert!(still.faces.iter().all(|face| {
            face.levels
                .iter()
                .flatten()
                .all(|l| (0.0..=255.0).contains(l))
        }));
    }
}
