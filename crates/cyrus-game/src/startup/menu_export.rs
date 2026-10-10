//! Installed menu surfaces and geometry; rules are in preservation's engine/menu.md.
use super::menu::{MenuAsset, MenuItem};
use crate::install::GameInstall;
use anyhow::{Context, ensure};
use image::RgbaImage;
use rgpre::import::{
    bsi::parse_bsi_file,
    fnt::{FntFile, parse_fnt},
    menu_ini::MenuPage,
    model3d::{Model3DFile, TextureData},
    palette::Palette,
    rob::parse_rob_file,
};
use std::{
    collections::BTreeMap,
    hash::{DefaultHasher, Hash, Hasher},
    path::Path,
};

pub(super) struct BookModel {
    pub data: Model3DFile,
    pub position: [f32; 3],
}

/// Export the installed main menu without reading or writing save-game state.
pub fn export_menu(install: &GameInstall, cache: &Path) -> anyhow::Result<()> {
    let read = |name: &str| {
        std::fs::read(install.root.join(name)).with_context(|| format!("menu: reading {name}"))
    };
    let ini_bytes = read("MENU.INI")?;
    let ini = std::str::from_utf8(&ini_bytes).context("menu: MENU.INI is not UTF-8")?;
    let mut page = MenuPage::parse(ini, 0).map_err(anyhow::Error::msg)?;
    // No resumable world exists in the startup-only menu.
    for entry in &mut page.entries {
        if matches!(entry.action, 1 | -1) {
            entry.grayed = true;
        }
    }
    let default = page
        .entries
        .iter()
        .position(|entry| entry.default)
        .unwrap_or(0);
    let eligible = |index: usize| {
        let entry = &page.entries[index];
        entry.selectable && !entry.grayed && !entry.text.is_empty()
    };
    let selected = (0..page.entries.len())
        .map(|step| (default + step) % page.entries.len())
        .find(|index| eligible(*index));
    let keys_bytes = read("KEYS.INI")?;
    let keys = rgpre::import::keys_ini::KeysIni::parse(
        std::str::from_utf8(&keys_bytes).context("menu: KEYS.INI is not UTF-8")?,
    )
    .map_err(anyhow::Error::msg)?;
    let bindings = crate::player::input::PlayerBindings::from_codes(keys.keyboard_codes())
        .map_err(anyhow::Error::msg)?;
    let system_bytes = read("SYSTEM.INI")?;
    let system = std::str::from_utf8(&system_bytes).context("menu: SYSTEM.INI is not UTF-8")?;
    let mut dialog = false;
    let mut repeat_ticks = None;
    for line in system.lines().map(str::trim) {
        if line.starts_with('[') {
            dialog = line.eq_ignore_ascii_case("[dialog]");
        } else if dialog
            && let Some((key, value)) = line.split_once('=')
            && key.trim().eq_ignore_ascii_case("menu_traverse_delay")
        {
            repeat_ticks = Some(
                value
                    .split(';')
                    .next()
                    .unwrap_or_default()
                    .trim()
                    .parse::<u32>()
                    .context("menu: SYSTEM.INI [dialog] menu_traverse_delay")?,
            );
        }
    }
    let repeat_ticks =
        repeat_ticks.context("menu: SYSTEM.INI [dialog] menu_traverse_delay is missing")?;
    ensure!(
        repeat_ticks > 0,
        "menu: menu_traverse_delay must be positive"
    );
    let archive_bytes = read("fxart/MENU.ROB")?;
    let palette_bytes = read("fxart/REDGUARD.COL")?;
    let normal_bytes = read("fonts/FONTNORM.FNT")?;
    let selected_bytes = read("fonts/FONTSEL.FNT")?;
    let mut banks = BTreeMap::new();
    let archive = parse_rob_file(&archive_bytes).context("menu: MENU.ROB")?;
    // These placements are recovered engine facts; entries/layout come from MENU.INI.
    let placements = [
        ("MENUA001", [384.0, 28.0, 396.0]),
        ("MB_PG01", [384.0, 28.0, 390.0]),
        ("MB_TABLE", [150.0, -72.0, 550.0]),
        ("MB_PG02", [384.0, 28.0, 392.0]),
        ("MB_PG03", [384.0, 28.0, 394.0]),
    ];
    let mut models = Vec::new();
    for (name, position) in placements {
        let segment = archive
            .segments
            .iter()
            .find(|segment| segment.name().eq_ignore_ascii_case(name))
            .with_context(|| format!("menu: MENU.ROB model {name} is missing"))?;
        let data = rgpre::import::model3d::parse_3d_file(&segment.data)
            .with_context(|| format!("menu: MENU.ROB model {name}"))?;
        ensure!(
            !data.frame_vertex_data.is_empty()
                && data.frame_vertex_data.len() == data.header.num_frames as usize
                && data
                    .frame_vertex_data
                    .iter()
                    .all(|frame| frame.coords.len() == data.vertex_coords.len()),
            "menu: MENU.ROB model {name} has incomplete frames"
        );
        for face in &data.face_data {
            if let TextureData::Texture { texture_id, .. } = face.texture_data
                && let std::collections::btree_map::Entry::Vacant(entry) = banks.entry(texture_id)
            {
                entry.insert(read(&format!("fxart/TEXBSI.{texture_id:03}"))?);
            }
            ensure!(
                face.face_vertices
                    .iter()
                    .all(|v| (v.vertex_index as usize) < data.vertex_coords.len()),
                "menu: MENU.ROB model {name} has invalid face vertex"
            );
        }
        models.push(BookModel { data, position });
    }
    for texture in &page.textures {
        if let std::collections::btree_map::Entry::Vacant(entry) = banks.entry(texture.set) {
            entry.insert(read(&format!("fxart/TEXBSI.{:03}", texture.set))?);
        }
    }
    let output = cache.join("menu");
    let stamp = output.join("source.txt");
    let mut fingerprint = DefaultHasher::new();
    (
        3u32,
        &keys_bytes,
        &ini_bytes,
        &system_bytes,
        &archive_bytes,
        &palette_bytes,
        &normal_bytes,
        &selected_bytes,
        &banks,
    )
        .hash(&mut fingerprint);
    let fingerprint = fingerprint.finish().to_string();
    if std::fs::read_to_string(&stamp).ok().as_deref() == Some(&fingerprint)
        && output.join("main.menu").is_file()
        && output.join("font.ttf").is_file()
        && (0..page.entries.len())
            .all(|index| output.join(format!("selection-{index}.png")).is_file())
    {
        return Ok(());
    }
    std::fs::create_dir_all(&output)?;
    if stamp.exists() {
        std::fs::remove_file(&stamp)?;
    }
    let palette = Palette::parse(&palette_bytes).context("menu: REDGUARD.COL")?;
    let normal_font = parse_fnt(&normal_bytes).context("menu: FONTNORM.FNT")?;
    let selected_font = parse_fnt(&selected_bytes).context("menu: FONTSEL.FNT")?;
    let mut textures = BTreeMap::new();
    let mut page_pixels = BTreeMap::new();
    for (set, bytes) in banks {
        let bank = parse_bsi_file(&bytes).with_context(|| format!("menu: TEXBSI.{set:03}"))?;
        for (index, texture) in bank.images.iter().enumerate() {
            ensure!(
                texture.width > 0
                    && texture.height > 0
                    && texture.pixel_data.len()
                        == usize::from(texture.width) * usize::from(texture.height),
                "menu: TEXBSI.{set:03} image {index}: invalid dimensions/pixel count"
            );
            if page
                .textures
                .iter()
                .any(|reference| reference.set == set && usize::from(reference.index) == index)
            {
                page_pixels.insert((set, index as u16), texture.pixel_data.clone());
            }
            let image = RgbaImage::from_raw(
                texture.width.into(),
                texture.height.into(),
                texture.decode_rgba(Some(&palette)),
            )
            .with_context(|| format!("menu: TEXBSI.{set:03} image {index}: invalid pixel count"))?;
            textures.insert((set, index as u16), image);
        }
    }
    for texture in &page.textures {
        ensure!(
            textures.contains_key(&(texture.set, texture.index)),
            "menu: missing TEXBSI.{} image {}",
            texture.set,
            texture.index
        );
    }
    for model in &models {
        for face in &model.data.face_data {
            if let TextureData::Texture {
                texture_id,
                image_id,
            } = face.texture_data
            {
                ensure!(
                    textures.contains_key(&(texture_id, image_id.into())),
                    "menu: missing TEXBSI.{texture_id} image {image_id}"
                );
            }
        }
    }
    let mut images = Vec::new();
    for selection in 0..page.entries.len() {
        let mut surfaces = textures.clone();
        let mut page_pixels = page_pixels.clone();
        for (index, entry) in page.entries.iter().enumerate() {
            let texture = page.textures[entry.texture];
            let image = surfaces.get_mut(&(texture.set, texture.index)).unwrap();
            let selected = selected.is_some() && index == selection && eligible(index);
            let font = if selected {
                &selected_font
            } else {
                &normal_font
            };
            let pixels = page_pixels
                .get_mut(&(texture.set, texture.index))
                .context("menu: missing indexed page surface")?;
            paint_text(image, pixels, font, &palette, entry, selected)?;
        }
        let name = format!("menu/selection-{selection}.png");
        super::menu_raster::render(&models, &surfaces, &palette)?.save(cache.join(&name))?;
        images.push(name);
    }
    std::fs::write(
        output.join("font.ttf"),
        rgpre::import::fnt_ttf::build_ttf_from_fnt(&normal_font, "Redguard menu")?,
    )?;
    let asset = MenuAsset {
        entries: page
            .entries
            .iter()
            .map(|entry| MenuItem {
                text: entry.text.clone(),
                action: entry.action,
                selectable: entry.selectable,
                grayed: entry.grayed,
                implemented: entry.action == -2,
            })
            .collect(),
        images,
        selected,
        repeat_seconds: repeat_ticks as f32 / 18.2,
        activation_key: bindings.menu_activation_key(),
    };
    std::fs::write(output.join("main.menu"), serde_json::to_vec(&asset)?)?;
    std::fs::write(stamp, fingerprint)?;
    Ok(())
}

fn paint_text(
    image: &mut RgbaImage,
    page_pixels: &mut [u8],
    font: &FntFile,
    palette: &Palette,
    entry: &rgpre::import::menu_ini::MenuEntry,
    selected: bool,
) -> anyhow::Result<()> {
    let glyph = |ch: u8| {
        usize::from(ch)
            .checked_sub(usize::from(font.header.character_start))
            .and_then(|i| font.glyphs.get(i))
            .filter(|g| g.enabled != 0)
    };
    ensure!(
        entry.text.is_ascii(),
        "menu: unsupported non-ASCII text {:?}",
        entry.text
    );
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
    let width: i32 = entry.text.bytes().map(advance).sum();
    let mut cursor = entry.position[0]
        - match entry.justify {
            1 => width / 2,
            2 => width,
            _ => 0,
        };
    for ch in entry.text.bytes() {
        let glyph = glyph(ch)
            .with_context(|| format!("menu: text {:?} has unsupported glyph {ch}", entry.text))?;
        for y in 0..glyph.height {
            for x in 0..glyph.width {
                let pixel =
                    glyph.pixels[usize::from(y) * usize::from(glyph.width) + usize::from(x)];
                if pixel == 0 {
                    continue;
                }
                let draw_x = cursor + i32::from(glyph.offset_left) + i32::from(x);
                let draw_y = entry.position[1] + i32::from(glyph.offset_top) + i32::from(y);
                if draw_x >= 0
                    && draw_y >= 0
                    && draw_x < image.width() as i32
                    && draw_y < image.height() as i32
                {
                    let offset = draw_y as usize * image.width() as usize + draw_x as usize;
                    let index = if selected {
                        pixel
                    } else if !entry.selectable {
                        (pixel & 15) + 144
                    } else if entry.grayed {
                        page_pixels[offset].wrapping_add(4)
                    } else {
                        pixel.wrapping_add(5)
                    };
                    page_pixels[offset] = index;
                    let [r, g, b] = palette.colors[usize::from(index)];
                    image.put_pixel(draw_x as u32, draw_y as u32, image::Rgba([r, g, b, 255]));
                }
            }
        }
        cursor += advance(ch);
    }
    Ok(())
}
