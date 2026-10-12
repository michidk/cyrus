//! Installed book geometry, text, widgets, and defaults for portable presentation.
use crate::install::GameInstall;
use crate::{
    menu::MenuItem,
    menu::{
        MenuAsset, MenuCorner, MenuEntry, MenuModel, MenuModelFace, MenuPage, MenuPicture,
        PageTexture,
    },
    preferences::MenuPreferences,
};
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
    crate::sound_export::export(install, cache).context("menu: exporting sound cues")?;
    let read = |name: &str| {
        std::fs::read(install.root.join(name)).with_context(|| format!("menu: reading {name}"))
    };
    let ini_bytes = read("MENU.INI")?;
    let world_path = rgpre::import::world_ini::find_world_ini(&install.root)
        .context("menu: missing WORLD.INI")?;
    let world_bytes = std::fs::read(&world_path)
        .with_context(|| format!("menu: reading {}", world_path.display()))?;
    let world_text = std::str::from_utf8(&world_bytes).context("menu: WORLD.INI is not UTF-8")?;
    let new_game =
        rgpre::import::world_ini::NewGameEntry::parse(world_text).map_err(anyhow::Error::msg)?;
    ensure!(
        rgpre::import::world_ini::WorldIni::parse(world_text)
            .entries
            .iter()
            .any(|entry| entry.index == new_game.world),
        "menu: WORLD.INI start_world {} has no map/palette entry",
        new_game.world
    );
    let flash_name = new_game
        .flash_filename
        .as_deref()
        .context("menu: WORLD.INI new-game transition artwork is missing")?;
    let mut flash_path = install.root.clone();
    for part in flash_name.split(['\\', '/']) {
        ensure!(
            !part.is_empty() && !matches!(part, "." | "..") && !part.contains(':'),
            "menu: WORLD.INI invalid transition path {flash_name}"
        );
        flash_path = crate::scene::find_file(&flash_path, part).with_context(|| {
            format!("menu: WORLD.INI transition artwork {flash_name} is missing")
        })?;
    }
    let flash_bytes = std::fs::read(&flash_path)
        .with_context(|| format!("menu: reading {}", flash_path.display()))?;
    let decoded = parse_gxa_file(&flash_bytes)
        .with_context(|| format!("menu: transition artwork {flash_name}"))?;
    ensure!(
        !decoded.frames.is_empty()
            && decoded
                .frames
                .iter()
                .all(|frame| (frame.width, frame.height) == (640, 480)),
        "menu: transition artwork {flash_name}: expected 640x480 frames"
    );
    let loading_font_bytes = read("fonts/REDSEL.FNT")?;
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
        12u32,
        &ini_bytes,
        (&world_bytes, &flash_bytes, &loading_font_bytes),
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
                item: MenuItem {visible: id != 3 || !(302..=305).contains(&entry.action) || general.force_movies, text: entry.text.clone(), action: entry.action, selectable: entry.selectable, grayed: entry.grayed, implemented: matches!(entry.action, -2 | 2 | 3 | 4 | 10 | 300..=305 | 400 | 405..=407 | 500 | 504 | 505 | 600..=603 | 700 | 710..=724)},
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
        let surfaces = (0..2)
            .filter(|i| page.textures[*i].is_some())
            .map(surface)
            .collect::<anyhow::Result<Vec<_>>>()?;
        exported_pages.insert(
            id,
            MenuPage {
                entries,
                selected,
                surfaces,
            },
        );
    }
    let font = |bytes: &[u8]| -> anyhow::Result<cyrus_text::Font> {
        Ok(cyrus_text::Font::from(
            &parse_fnt(bytes).context("menu: FNT font")?,
        ))
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
        new_game: crate::menu::NewGameEntry {
            world: new_game.world,
            marker: new_game.marker,
            loading: decoded
                .frames
                .iter()
                .map(|frame| MenuPicture {
                    width: frame.width,
                    height: frame.height,
                    rgba: frame.rgba.clone(),
                })
                .collect(),
            font: font(&loading_font_bytes)?,
        },
        pages: exported_pages,
        models: models
            .into_iter()
            .map(|(model, position)| export_model(model, position, &references, &palette))
            .collect::<anyhow::Result<_>>()?,
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

fn export_model(
    model: Model3DFile,
    position: [f32; 3],
    references: &BTreeMap<(u16, u16), usize>,
    palette: &Palette,
) -> anyhow::Result<MenuModel> {
    let mut faces = Vec::new();
    for face in &model.face_data {
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
        faces.push(MenuModelFace {
            corners: face
                .face_vertices
                .iter()
                .map(|vertex| MenuCorner {
                    vertex: vertex.vertex_index as usize,
                    uv: [vertex.u as f32 / 16.0, vertex.v as f32 / 16.0],
                })
                .collect(),
            texture,
            color,
        });
    }
    Ok(MenuModel {
        position,
        frames: model
            .frame_vertex_data
            .iter()
            .map(|frame| {
                frame
                    .coords
                    .iter()
                    .map(|point| [point.x, point.y, point.z])
                    .collect()
            })
            .collect(),
        faces,
    })
}
