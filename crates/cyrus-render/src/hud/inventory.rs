//! Inventory overlay setup, artwork sizing and ordered bitmap/text projection.
use super::*;

struct InventoryArtworkSizes {
    border: [u16; 2],
    up_arrow: [u16; 2],
    down_arrow: [u16; 2],
}

impl From<[[u16; 2]; 3]> for InventoryArtworkSizes {
    fn from([border, up_arrow, down_arrow]: [[u16; 2]; 3]) -> Self {
        Self {
            border,
            up_arrow,
            down_arrow,
        }
    }
}

pub(super) fn spawn_inventory(mut commands: Commands, viewports: Query<Entity, With<HudViewport>>) {
    let Ok(viewport) = viewports.single() else {
        return;
    };
    commands.entity(viewport).with_child((
        InventoryOverlay,
        Visibility::Hidden,
        // Artwork and text use the same 640×480 coordinates as the world HUD.
        place([0, 0], [640, 480]),
    ));
    commands.entity(viewport).with_child((
        ExamineTextOverlay,
        Visibility::Hidden,
        // Above the examine view's model canvas.
        GlobalZIndex(106),
        ImageNode::default(),
        place([0, 0], [640, 480]),
    ));
}

pub(super) fn draw_inventory_text(
    rgba: &mut [u8],
    font: &cyrus_text::Font,
    text: &str,
    at: [i32; 2],
    style: cyrus_text::FontStyle,
) {
    font.draw_shadowed_by(rgba, 640, text, at, style, 2);
}

#[allow(clippy::too_many_arguments)] // The inventory and examine states, the HUD asset and the overlay it rebuilds.
pub(super) fn inventory_draw(
    screen: Option<Res<cyrus_script::inventory_screen::InventoryScreen>>,
    examine: Option<Res<cyrus_script::item_viewer::ExamineScreen>>,
    hud_handle: Option<Res<HudHandle>>,
    huds: Res<Assets<HudAsset>>,
    environment: Res<crate::environment::EnvironmentRequest>,
    environments: Res<Assets<crate::environment::Environment>>,
    assets: Res<AssetServer>,
    mut images: ResMut<Assets<Image>>,
    mut overlays: Query<(Entity, &mut Visibility, Option<&Children>), With<InventoryOverlay>>,
    mut commands: Commands,
) {
    let (Some(screen), Some(handle), Ok((overlay, mut visibility, children))) =
        (screen, hud_handle, overlays.single_mut())
    else {
        return;
    };
    if !screen.open {
        visibility.set_if_neq(Visibility::Hidden);
        return;
    }
    visibility.set_if_neq(Visibility::Inherited);
    let Some(hud) = huds.get(&handle.0) else {
        return;
    };
    let Some(environment) = environments.get(&environment.0) else {
        return;
    };
    // From the View request until the examine view returns, the cursor item's bitmap, count and
    // name are not drawn.
    let viewing =
        examine.is_some_and(|examine| examine.suspended() || examine.inventory_request.is_some());
    let cells: Vec<_> = screen
        .layout
        .cells
        .iter()
        .filter(|cell| !(viewing && cell.cursor))
        .collect();
    if let Some(children) = children {
        for child in children.iter() {
            commands.entity(child).despawn();
        }
    }
    let sizes = InventoryArtworkSizes::from(hud.inventory_screen);
    commands.entity(overlay).with_children(|parent| {
        parent.spawn((
            BackgroundColor(Color::srgba(
                10.0 / 255.0,
                10.0 / 255.0,
                10.0 / 255.0,
                180.0 / 255.0,
            )),
            place([90, 49], [460, 384]),
            ZIndex(0),
        ));
        let mut image = |path: String, at: [i32; 2], size: [u16; 2]| {
            parent.spawn((
                StoredArtwork,
                artwork::ArtworkFog(environment.background),
                ImageNode::new(assets.load(path)),
                place(at, size),
                ZIndex(1),
            ));
        };
        image("cache://hud/invbordr-00.png".into(), [90, 49], sizes.border);
        if screen.layout.up_arrow {
            image(
                "cache://hud/invarrow-00.png".into(),
                [313, 54],
                sizes.up_arrow,
            );
        }
        if screen.layout.down_arrow {
            image(
                "cache://hud/invarrow-01.png".into(),
                [313, 419],
                sizes.down_arrow,
            );
        }
        for cell in &cells {
            let Some(item) = hud.items.iter().find(|item| item.id == cell.id) else {
                continue;
            };
            let (path, at, size) = if cell.cursor {
                (
                    format!("cache://hud/selected-{:02}.png", item.bitmap),
                    [cell.at[0] - 2, cell.at[1] - 2],
                    item.selected_size.unwrap_or(item.size),
                )
            } else {
                (
                    format!("cache://hud/item-{:02}.png", item.bitmap),
                    cell.at,
                    item.size,
                )
            };
            image(path, at, size);
        }
        let mut rgba = inventory_text(hud, &cells);
        artwork::store_pixels(&mut rgba, 640, [0, 0], Some(environment.background));
        let mut text_image = Image::new(
            Extent3d {
                width: 640,
                height: 480,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            rgba,
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::default(),
        );
        text_image.sampler = ImageSampler::nearest();
        parent.spawn((
            ImageNode::new(images.add(text_image)),
            place([0, 0], [640, 480]),
            ZIndex(2),
        ));
    });
}

fn inventory_text(hud: &HudAsset, cells: &[&cyrus_script::inventory_screen::Cell]) -> Vec<u8> {
    let mut rgba = vec![0; 640 * 480 * 4];
    let bottom = hud
        .font
        .glyph(b'a')
        .map_or(0, |g| i32::from(g.offset[1]) + i32::from(g.height));
    for cell in cells {
        let Some(item) = hud.items.iter().find(|item| item.id == cell.id) else {
            continue;
        };
        // Every visible item with more than one unit shows its count, centred on x + 84.
        if cell.count > 1 {
            let text = cell.count.to_string();
            let at = [cell.at[0] + 84 - hud.font.width(&text) / 2, cell.at[1] + 10];
            draw_inventory_text(&mut rgba, &hud.font, &text, at, hud.text_style);
        }
        if !cell.cursor {
            continue;
        }
        // `name[x]` is an RTX label; text starting with byte 0x9E counts as absent.
        let name = item
            .name
            .as_deref()
            .and_then(|label| {
                hud.labels
                    .iter()
                    .find(|(key, _)| key.eq_ignore_ascii_case(label))
            })
            .map(|(_, text)| text.as_str())
            .filter(|text| !text.is_empty() && !text.starts_with('\u{9e}'));
        let top = cell.at[1] + 92 - bottom;
        let lines = name.map(cyrus_script::inventory_screen::name_lines);
        let centre = cell.at[0]
            + if lines.is_some() {
                i32::from(item.selected_size.unwrap_or(item.size)[0]) / 2
            } else {
                52
            };
        let fallback = format!("INDEX {}", item.id);
        for (line, text) in lines
            .unwrap_or_else(|| vec![fallback.as_str()])
            .iter()
            .enumerate()
        {
            let y = top + (line as i32 * i32::from(hud.font.line_height) * 9) / 10;
            draw_inventory_text(
                &mut rgba,
                &hud.font,
                text,
                [centre - hud.font.width(text) / 2, y],
                hud.text_style,
            );
        }
    }
    rgba
}
