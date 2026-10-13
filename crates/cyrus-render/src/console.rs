//! Installed paper and bitmap-font presentation for the shared console editor.
use bevy::{
    asset::{AssetLoader, LoadContext, RenderAssetUsages, io::Reader},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use cyrus_assets::console::ConsoleArtwork;
use cyrus_console::Console;
use cyrus_game::AppState;

#[derive(Default, TypePath)]
struct Loader;
impl AssetLoader for Loader {
    type Asset = ConsoleArtwork;
    type Settings = ();
    type Error = anyhow::Error;
    async fn load(
        &self,
        reader: &mut dyn Reader,
        _: &(),
        _: &mut LoadContext<'_>,
    ) -> Result<Self::Asset, Self::Error> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        let asset: ConsoleArtwork = serde_json::from_slice(&bytes)?;
        asset.validate()?;
        Ok(asset)
    }
    fn extensions(&self) -> &[&str] {
        &["console"]
    }
}
#[derive(Resource)]
struct Artwork(Handle<ConsoleArtwork>);
#[derive(Component)]
struct Panel;
#[derive(Component)]
struct Page;

pub struct ConsoleRenderPlugin;
impl Plugin for ConsoleRenderPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<ConsoleArtwork>()
            .init_asset_loader::<Loader>()
            .init_resource::<Console>()
            .add_systems(OnEnter(AppState::InGame), setup)
            .add_systems(OnExit(AppState::InGame), cleanup)
            .add_systems(Update, draw.run_if(in_state(AppState::InGame)));
    }
}
fn cleanup(mut commands: Commands, panels: Query<Entity, With<Panel>>) {
    for entity in &panels {
        commands.entity(entity).despawn();
    }
}
fn setup(mut commands: Commands, server: Res<AssetServer>) {
    commands.insert_resource(Artwork(server.load("cache://console/paper.console")));
    commands
        .spawn((
            Panel,
            GlobalZIndex(100),
            Visibility::Hidden,
            Node {
                width: percent(100),
                height: percent(100),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::BLACK),
        ))
        .with_children(|parent| {
            parent.spawn((
                Page,
                ImageNode::default(),
                Node {
                    height: percent(100),
                    max_width: percent(100),
                    aspect_ratio: Some(640.0 / 480.0),
                    ..default()
                },
            ));
        });
}
fn draw(
    scene: Res<cyrus_world::lifecycle::CurrentScene>,
    mut console: ResMut<Console>,
    artwork: Res<Artwork>,
    assets: Res<Assets<ConsoleArtwork>>,
    mut images: ResMut<Assets<Image>>,
    mut panel: Query<&mut Visibility, With<Panel>>,
    mut page: Query<&mut ImageNode, With<Page>>,
) {
    let visible = console.open;
    let Ok(mut panel) = panel.single_mut() else {
        return;
    };
    *panel = if visible {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
    let Some(artwork) = assets.get(&artwork.0) else {
        return;
    };
    let Ok(mut page) = page.single_mut() else {
        return;
    };
    let Some(palette) = scene.world.and_then(|world| artwork.palettes.get(&world)) else {
        return;
    };
    if console.palette != Some(palette.gui) {
        console.palette = Some(palette.gui);
    }
    if !visible {
        return;
    }
    if !console.is_changed()
        && !scene.is_changed()
        && page.image != bevy::image::TRANSPARENT_IMAGE_HANDLE
    {
        return;
    }
    let image = raster(artwork, console.bypass_change_detection(), palette);
    if page.image != bevy::image::TRANSPARENT_IMAGE_HANDLE
        && let Some(mut existing) = images.get_mut(&page.image)
    {
        *existing = image;
    } else {
        page.image = images.add(image);
    }
}

fn raster(
    artwork: &ConsoleArtwork,
    console: &mut Console,
    palette: &cyrus_assets::console::ConsolePalette,
) -> Image {
    let width = usize::from(artwork.width);
    let height = usize::from(artwork.height);
    let ink = |index: u32| {
        palette
            .gui
            .get(index as usize)
            .map(|entry| palette.colors[usize::from(*entry)])
    };
    let editing = ink(console.preferences.colors[0]).unwrap_or(palette.colors[0]);
    let report = ink(console.preferences.colors[1]).unwrap_or(palette.colors[0]);
    let mut pixels = if console.preferences.paper_disabled == 0 {
        artwork.paper.clone()
    } else {
        let color = palette.colors[usize::from(console.preferences.background)];
        [color[0], color[1], color[2], 255].repeat(width * height)
    };
    let line_height = i32::from(artwork.font.line_height);
    let body_y = 8 + line_height * 4;
    console.page_lines = (height.saturating_sub(body_y as usize) / line_height as usize).max(1);
    let mut text = |line: &str, x: i32, y: i32, colour: [u8; 3]| {
        let mut pen = x;
        for ch in line.bytes() {
            let character = [ch];
            let character = std::str::from_utf8(&character).unwrap_or("?");
            for shadow in [true, false] {
                if shadow && console.preferences.shadow == 0 {
                    continue;
                }
                let offset = if shadow { 2 } else { 0 };
                artwork
                    .font
                    .draw(character, [pen + offset, y + offset], |x, y, _| {
                        if x >= 0 && y >= 0 && (x as usize) < width && (y as usize) < height {
                            let at = (y as usize * width + x as usize) * 4;
                            if shadow {
                                for channel in &mut pixels[at..at + 3] {
                                    *channel = (u16::from(*channel) * (255 - 199) / 255) as u8;
                                }
                            } else {
                                pixels[at..at + 4]
                                    .copy_from_slice(&[colour[0], colour[1], colour[2], 255]);
                            }
                        }
                    });
            }
            pen += artwork.font.advance(ch);
        }
    };
    text("RedGuard Development Console", 8, 8, editing);
    text(
        "cyrus - cheat state and command inspection",
        8,
        8 + line_height,
        editing,
    );
    for (row, line) in console
        .lines
        .iter()
        .skip(console.top)
        .take(console.page_lines)
        .enumerate()
    {
        let y = body_y + row as i32 * line_height;
        let value = if line.report {
            format!(" {}", line.text)
        } else {
            format!("> {}", line.text)
        };
        text(&value, 8, y, if line.report { report } else { editing });
        if console.top + row == console.selected && console.cursor_visible() {
            let prefix = if line.report { " " } else { "> " };
            let x = 8
                + artwork.font.width(prefix)
                + artwork.font.width(
                    &line.text[..line
                        .text
                        .floor_char_boundary(console.cursor.min(line.text.len()))],
                );
            text("|", x, y - 4, report);
            text("|", x, y + 4, report);
        }
    }
    let separator = (body_y - line_height) as usize;
    if separator < height {
        for x in 0..width {
            pixels[(separator * width + x) * 4..(separator * width + x) * 4 + 4]
                .copy_from_slice(&[report[0], report[1], report[2], 255]);
        }
    }
    let mut image = Image::new(
        Extent3d {
            width: width as u32,
            height: height as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        pixels,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.sampler = bevy::image::ImageSampler::nearest();
    image
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preference_changes_draw_gui_ink_shadow_and_solid_background_pixels() {
        let artwork = ConsoleArtwork {
            width: 64,
            height: 64,
            paper: vec![255; 64 * 64 * 4],
            font: cyrus_text::Font {
                start: 32,
                line_height: 9,
                glyphs: vec![
                    cyrus_text::Glyph {
                        enabled: true,
                        width: 1,
                        height: 1,
                        offset: [0, 0],
                        pixels: vec![1]
                    };
                    96
                ],
                palette: Vec::new(),
            },
            palettes: default(),
        };
        let mut palette = cyrus_assets::console::ConsolePalette {
            colors: vec![[0; 3]; 256],
            gui: [0; 16],
        };
        palette.gui[1] = 5;
        palette.colors[5] = [90, 80, 70];
        palette.colors[2] = [42, 43, 44];
        let mut console = Console::default();
        console.preferences.colors[0] = 1;
        console.preferences.shadow = 1;
        let page = raster(&artwork, &mut console, &palette);
        let pixels = page.data.unwrap();
        let at = |x: usize, y: usize| (y * 64 + x) * 4;
        assert_eq!(&pixels[at(8, 8)..at(8, 8) + 4], &[90, 80, 70, 255]);
        assert_eq!(&pixels[at(10, 10)..at(10, 10) + 4], &[56, 56, 56, 255]);
        assert_eq!(&pixels[..4], &[255; 4]);
        console.preferences.paper_disabled = 1;
        console.preferences.background = 2;
        let pixels = raster(&artwork, &mut console, &palette).data.unwrap();
        assert_eq!(&pixels[..4], &[42, 43, 44, 255]);
    }

    #[test]
    fn console_owns_its_texture_without_replacing_the_shared_default() {
        let mut app = App::new();
        let mut images = Assets::<Image>::default();
        images
            .insert(Handle::<Image>::default().id(), Image::default())
            .unwrap();
        images
            .insert(bevy::image::TRANSPARENT_IMAGE_HANDLE.id(), Image::default())
            .unwrap();
        let original_size = images.get(&Handle::<Image>::default()).unwrap().size();
        let mut artwork = Assets::<ConsoleArtwork>::default();
        let handle = artwork.add(ConsoleArtwork {
            width: 64,
            height: 48,
            paper: vec![255; 64 * 48 * 4],
            font: cyrus_text::Font {
                line_height: 9,
                ..default()
            },
            palettes: [(
                0,
                cyrus_assets::console::ConsolePalette {
                    colors: vec![[0, 168, 0]; 256],
                    gui: [0; 16],
                },
            )]
            .into(),
        });
        app.insert_resource(images)
            .insert_resource(artwork)
            .insert_resource(Artwork(handle))
            .insert_resource(cyrus_world::lifecycle::CurrentScene {
                name: "test".into(),
                world: Some(0),
            })
            .insert_resource(Console::default())
            .add_systems(Update, draw);
        app.world_mut().resource_mut::<Console>().open = true;
        app.world_mut().spawn((Panel, Visibility::Hidden));
        let page = app.world_mut().spawn((Page, ImageNode::default())).id();
        app.update();
        let image = app.world().get::<ImageNode>(page).unwrap().image.clone();
        assert_ne!(image, bevy::image::TRANSPARENT_IMAGE_HANDLE);
        let images = app.world().resource::<Assets<Image>>();
        assert_eq!(images.get(&image).unwrap().size(), UVec2::new(64, 48));
        assert_eq!(
            images.get(&Handle::<Image>::default()).unwrap().size(),
            original_size
        );
        assert_eq!(
            images
                .get(&bevy::image::TRANSPARENT_IMAGE_HANDLE)
                .unwrap()
                .size(),
            original_size
        );
        app.update();
        assert_eq!(app.world().get::<ImageNode>(page).unwrap().image, image);
    }
}
