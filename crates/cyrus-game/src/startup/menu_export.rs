//! Installed book geometry, text, widgets, and defaults for portable presentation.
use super::{
    menu::MenuItem,
    menu_data::{
        MenuAsset, MenuEntry, MenuFace, MenuFont, MenuGlyph, MenuPage, MenuPicture, PageTexture,
    },
    menu_preferences::MenuPreferences,
};
use crate::install::GameInstall;
use anyhow::{Context, ensure};
use rgpre::import::{
    bsi::parse_bsi_file,
    fnt::parse_fnt,
    gxa::parse_gxa_file,
    menu_ini,
    model3d::{Model3DFile, TextureData},
    palette::Palette,
    rob::parse_rob_file,
};
use std::{
    collections::BTreeMap,
    hash::{DefaultHasher, Hash, Hasher},
    path::Path,
};

/// Export data-defined menus without reading or writing save-game state.
pub fn export_menu(install: &GameInstall, cache: &Path) -> anyhow::Result<()> {
    let read = |name: &str| {
        std::fs::read(install.root.join(name)).with_context(|| format!("menu: reading {name}"))
    };
    let ini_bytes = read("MENU.INI")?;
    let ini = std::str::from_utf8(&ini_bytes).context("menu: MENU.INI is not UTF-8")?;
    let general = menu_ini::MenuGeneral::parse(ini).map_err(anyhow::Error::msg)?;
    let mut pages = BTreeMap::new();
    for id in [0, 3, 4, 5, 6, 7] {
        let mut page = menu_ini::MenuPage::parse(ini, id).map_err(anyhow::Error::msg)?;
        if id == 0 {
            for entry in &mut page.entries {
                if matches!(entry.action, 1 | -1) {
                    entry.grayed = true;
                }
            }
        }
        if id == 5 {
            page.entries.retain(|e| !matches!(e.action, 501..=503));
        }
        pages.insert(id as u8, page);
    }
    let keys_bytes = read("KEYS.INI")?;
    let keys = rgpre::import::keys_ini::KeysIni::parse(
        std::str::from_utf8(&keys_bytes).context("menu: KEYS.INI is not UTF-8")?,
    )
    .map_err(anyhow::Error::msg)?;
    let system_bytes = read("SYSTEM.INI")?;
    let preferences = rgpre::import::menu_settings::MenuSettings::parse(
        std::str::from_utf8(&system_bytes).context("menu: SYSTEM.INI is not UTF-8")?,
    )
    .map_err(anyhow::Error::msg)?;
    let defaults = MenuPreferences {
        values: [
            (504, i32::from(preferences.subtitles)),
            (505, i32::from(preferences.movie_interlace)),
            (601, preferences.sound_volume.into()),
            (602, preferences.music_volume.into()),
            (603, i32::from(preferences.voice)),
        ]
        .into(),
        bindings: keys.control_codes(),
    };
    let archive_bytes = read("fxart/MENU.ROB")?;
    let palette_bytes = read("fxart/REDGUARD.COL")?;
    let normal_bytes = read("fonts/FONTNORM.FNT")?;
    let selected_bytes = read("fonts/FONTSEL.FNT")?;
    let widget_bytes = [
        read("system/MM_CHECK.GXA")?,
        read("system/MM_SLIDE.GXA")?,
        read("system/MM_MOVIE.GXA")?,
    ];
    let archive = parse_rob_file(&archive_bytes).context("menu: MENU.ROB")?;
    let placements = [
        ("MENUA001", [384.0, 28.0, 396.0]),
        ("MB_PG01", [384.0, 28.0, 390.0]),
        ("MB_TABLE", [150.0, -72.0, 550.0]),
        ("MB_PG02", [384.0, 28.0, 392.0]),
        ("MB_PG03", [384.0, 28.0, 394.0]),
    ];
    let mut models = Vec::new();
    let mut banks = BTreeMap::new();
    for (name, position) in placements {
        let segment = archive
            .segments
            .iter()
            .find(|s| s.name().eq_ignore_ascii_case(name))
            .with_context(|| format!("menu: MENU.ROB model {name} missing"))?;
        let data = rgpre::import::model3d::parse_3d_file(&segment.data)
            .with_context(|| format!("menu: MENU.ROB model {name}"))?;
        ensure!(
            !data.frame_vertex_data.is_empty()
                && data.frame_vertex_data.len() == data.header.num_frames as usize
                && data
                    .frame_vertex_data
                    .iter()
                    .all(|f| f.coords.len() == data.vertex_coords.len()),
            "menu: MENU.ROB model {name} has incomplete frames"
        );
        for face in &data.face_data {
            ensure!(
                face.face_vertices
                    .iter()
                    .all(|v| (v.vertex_index as usize) < data.vertex_coords.len()),
                "menu: MENU.ROB model {name} invalid face vertex"
            );
            if let TextureData::Texture { texture_id, .. } = face.texture_data
                && !banks.contains_key(&texture_id)
            {
                banks.insert(texture_id, read(&format!("fxart/TEXBSI.{texture_id:03}"))?);
            }
        }
        models.push((data, position));
    }
    for page in pages.values() {
        for texture in page.textures.iter().flatten() {
            if let std::collections::btree_map::Entry::Vacant(entry) = banks.entry(texture.set) {
                entry.insert(read(&format!("fxart/TEXBSI.{:03}", texture.set))?);
            }
        }
    }
    let output = cache.join("menu");
    let mut fingerprint = DefaultHasher::new();
    (
        6u32,
        &ini_bytes,
        &keys_bytes,
        &system_bytes,
        &archive_bytes,
        &palette_bytes,
        &normal_bytes,
        &selected_bytes,
        &widget_bytes,
        &banks,
    )
        .hash(&mut fingerprint);
    let fingerprint = fingerprint.finish().to_string();
    if std::fs::read_to_string(output.join("source.txt"))
        .ok()
        .as_deref()
        == Some(&fingerprint)
        && output.join("book.menu").is_file()
        && output.join("font.ttf").is_file()
    {
        return Ok(());
    }
    std::fs::create_dir_all(&output)?;
    let palette = Palette::parse(&palette_bytes).context("menu: REDGUARD.COL")?;
    let mut textures = Vec::new();
    let mut references = BTreeMap::new();
    for (set, bytes) in banks {
        let bank = parse_bsi_file(&bytes).with_context(|| format!("menu: TEXBSI.{set:03}"))?;
        for (index, image) in bank.images.iter().enumerate() {
            references.insert((set, index as u16), textures.len());
            textures.push(PageTexture {
                width: image.width,
                height: image.height,
                indices: image.pixel_data.clone(),
                rgba: image.decode_rgba(Some(&palette)),
            });
        }
    }
    let mut exported_pages = BTreeMap::new();
    for (id, page) in pages {
        let surface = |slot: usize| -> anyhow::Result<usize> {
            let texture = page.textures[slot].context("menu: undefined page texture slot")?;
            references
                .get(&(texture.set, texture.index))
                .copied()
                .with_context(|| {
                    format!(
                        "menu: page {id}: missing TEXBSI.{} image {}",
                        texture.set, texture.index
                    )
                })
        };
        let mut entries = Vec::new();
        for entry in &page.entries {
            entries.push(MenuEntry {
                item: MenuItem {visible: id != 3 || !(302..=305).contains(&entry.action) || general.force_movies, text: entry.text.clone(), action: entry.action, selectable: entry.selectable, grayed: entry.grayed, implemented: matches!(entry.action, -2 | 3 | 4 | 300 | 400 | 405..=407 | 500 | 504 | 505 | 600..=603 | 700 | 710..=724)},
                position: entry.position, output_position: entry.output_position, justify: entry.justify, output_justify: entry.output_justify, slider: entry.slider, texture: surface(entry.texture)?,
            });
        }
        let default = if id == 5 {
            entries.iter().position(|e| e.item.action == 504)
        } else {
            page.entries.iter().position(|e| e.default)
        }
        .unwrap_or(0);
        let selected = (0..entries.len())
            .map(|step| (default + step) % entries.len())
            .find(|i| entries[*i].item.eligible());
        let camera = match id {
            3 => [336.0, 0.0, 0.0],
            5 => [336.0, -128.0, 0.0],
            6 => [336.0, 176.0, 0.0],
            _ => [0.0; 3],
        };
        let frames = match id {
            0 => [0; 5],
            3 => [0, 8, 0, 0, 0],
            7 => [0, 8, 0, 8, 8],
            _ => [0, 8, 0, 8, 0],
        };
        let mut faces = Vec::new();
        for (index, (model, placement)) in models.iter().enumerate() {
            let mut placement = *placement;
            if frames[index] == 8 {
                placement[2] += if index == 1 {
                    4.0
                } else if index == 4 {
                    -4.0
                } else {
                    0.0
                };
            }
            faces.extend(project(
                model,
                frames[index],
                placement,
                camera,
                &references,
                &palette,
            )?);
        }
        let surfaces = (0..2)
            .filter(|i| page.textures[*i].is_some())
            .map(surface)
            .collect::<anyhow::Result<Vec<_>>>()?;
        exported_pages.insert(
            id,
            MenuPage {
                entries,
                selected,
                faces,
                surfaces,
            },
        );
    }
    let font = |bytes: &[u8]| -> anyhow::Result<MenuFont> {
        let data = parse_fnt(bytes).context("menu: FNT font")?;
        Ok(MenuFont {
            start: data.header.character_start,
            glyphs: data
                .glyphs
                .iter()
                .map(|g| MenuGlyph {
                    enabled: g.enabled != 0,
                    width: g.width,
                    height: g.height,
                    offset: [g.offset_left.into(), g.offset_top.into()],
                    pixels: g.pixels.clone(),
                })
                .collect(),
        })
    };
    let pictures = |bytes: &[u8]| -> anyhow::Result<Vec<MenuPicture>> {
        Ok(parse_gxa_file(bytes)
            .context("menu: widget GXA")?
            .frames
            .into_iter()
            .map(|f| MenuPicture {
                width: f.width,
                height: f.height,
                rgba: f.rgba,
            })
            .collect())
    };
    let mut movies = pictures(&widget_bytes[2])?;
    for frame in &mut movies {
        for pixel in frame.rgba.as_chunks_mut::<4>().0 {
            pixel[3] = 255;
        }
    }
    let book = MenuAsset {
        pages: exported_pages,
        textures,
        palette: palette.colors.to_vec(),
        fonts: [font(&normal_bytes)?, font(&selected_bytes)?],
        checks: pictures(&widget_bytes[0])?,
        sliders: pictures(&widget_bytes[1])?,
        movies,
        key_names: keys
            .names
            .into_iter()
            .enumerate()
            .map(|(code, name)| {
                name.with_context(|| format!("menu: KEYS.INI [defined] key[{code}] missing"))
                    .map(|name| name.chars().take(15).collect())
            })
            .collect::<anyhow::Result<_>>()?,
        defaults,
        waiting_message: keys.waiting_message.chars().take(15).collect(),
        repeat_seconds: preferences.repeat_ticks as f32 / 18.2,
    };
    book.validate()?;
    std::fs::write(
        output.join("font.ttf"),
        rgpre::import::fnt_ttf::build_ttf_from_fnt(&parse_fnt(&normal_bytes)?, "Redguard menu")?,
    )?;
    std::fs::write(output.join("book.menu"), serde_json::to_vec(&book)?)?;
    std::fs::write(output.join("source.txt"), fingerprint)?;
    Ok(())
}

fn project(
    model: &Model3DFile,
    frame: usize,
    position: [f32; 3],
    camera: [f32; 3],
    references: &BTreeMap<(u16, u16), usize>,
    palette: &Palette,
) -> anyhow::Result<Vec<MenuFace>> {
    let mut faces = Vec::new();
    for face in &model.face_data {
        let vertices: Vec<_> = face
            .face_vertices
            .iter()
            .map(|vertex| {
                let point = model.frame_vertex_data[frame].coords[vertex.vertex_index as usize];
                let [x, y, z] = [
                    position[0] - point.x - camera[0],
                    position[1] + point.y - camera[1],
                    position[2] - point.z - camera[2],
                ];
                [
                    320.0 + x / z * (320.0 / 0.788),
                    240.0 + y / z * (320.0 / 0.788),
                    z.recip(),
                    vertex.u as f32 / 16.0,
                    vertex.v as f32 / 16.0,
                ]
            })
            .collect();
        let (texture, color) = match face.texture_data {
            TextureData::Texture {
                texture_id,
                image_id,
            } => (
                Some(
                    *references
                        .get(&(texture_id, image_id.into()))
                        .with_context(|| {
                            format!("menu: missing TEXBSI.{texture_id} image {image_id}")
                        })?,
                ),
                [0; 4],
            ),
            TextureData::SolidColor(i) => {
                let [r, g, b] = palette.colors[usize::from(i)];
                (None, [r, g, b, 255])
            }
        };
        for corner in 1..vertices.len().saturating_sub(1) {
            faces.push(MenuFace {
                vertices: [vertices[0], vertices[corner], vertices[corner + 1]],
                texture,
                color,
            });
        }
    }
    Ok(faces)
}
