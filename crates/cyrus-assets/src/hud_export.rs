//! Installed HUD artwork, font and layout exported for portable presentation.
use crate::{
    hud::{HudAsset, HudItem},
    install::GameInstall,
};
use anyhow::Context;
use rgpre::import::{fnt::parse_fnt, gxa::parse_gxa_file, keys_ini::KeysIni, rtx::parse_rtx_file};
use std::{
    collections::BTreeMap,
    hash::{DefaultHasher, Hash, Hasher},
    path::Path,
};

type Fields = BTreeMap<(String, String), String>;

/// Writes `hud/overlay.hud` with the candle, compass, logbook, power-up and item PNGs.
/// The candle, compass, logbook and font file names are fixed by the engine; positions,
/// modes, items and the power-up atlas come from the INI files.
pub fn export_hud(install: &GameInstall, cache: &Path) -> anyhow::Result<()> {
    let locate = |name: &str| -> anyhow::Result<std::path::PathBuf> {
        let mut path = install.root.clone();
        for part in name.split(['\\', '/']) {
            anyhow::ensure!(
                !part.is_empty() && part != ".." && part != ".",
                "hud: invalid path {name}"
            );
            path = crate::scene::find_file(&path, part)
                .with_context(|| format!("hud: {name} is missing"))?;
        }
        Ok(path)
    };
    let read = |name: &str| -> anyhow::Result<Vec<u8>> {
        let path = locate(name)?;
        std::fs::read(&path).with_context(|| format!("hud: reading {}", path.display()))
    };
    let system = read("SYSTEM.INI")?;
    let items = read("ITEM.INI")?;
    let keys = read("KEYS.INI")?;
    let worlds = read("WORLD.INI")?;
    let system_fields = ini_fields(&system, "SYSTEM.INI")?;
    let item_fields = ini_fields(&items, "ITEM.INI")?;
    let atlas_name = text(&item_fields, "ITEM.INI", "items", "bitmap_file")?.to_owned();
    let powerup_name = text(&system_fields, "SYSTEM.INI", "system", "game_bitmap")?.to_owned();
    let candle_bytes = read("system/SNUFF.GXA")?;
    let compass_bytes = read("system/COMPASS2.GXA")?;
    let logbook_bytes = read("system/SCROLL.GXA")?;
    let powerup_bytes = read(&powerup_name)?;
    let atlas_bytes = read(&atlas_name)?;
    let font_bytes = read("fonts/REDSEL.FNT")?;
    // The RTX file is large; its size and modification time stand in for its bytes.
    let rtx_path = locate(text(
        &system_fields,
        "SYSTEM.INI",
        "system",
        "rtx_filename",
    )?)?;
    let rtx_stamp = std::fs::metadata(&rtx_path)
        .and_then(|m| Ok((m.len(), m.modified()?)))
        .with_context(|| format!("hud: reading {}", rtx_path.display()))?;

    let output = cache.join("hud");
    let stamp = output.join("source.txt");
    let mut fingerprint = DefaultHasher::new();
    (
        5u32,
        (&system, &items, &keys, &worlds),
        (
            &candle_bytes,
            &compass_bytes,
            &logbook_bytes,
            &powerup_bytes,
        ),
        &atlas_bytes,
        &font_bytes,
        rtx_stamp,
    )
        .hash(&mut fingerprint);
    let fingerprint = fingerprint.finish().to_string();
    if std::fs::read_to_string(&stamp).ok().as_deref() == Some(&fingerprint)
        && output.join("overlay.hud").is_file()
    {
        return Ok(());
    }

    let number = |fields: &Fields, file: &str, section: &str, key: &str| -> anyhow::Result<i64> {
        let value = text(fields, file, section, key)?;
        value.parse().with_context(|| {
            format!("hud: {file} [{section}] {key}: expected a number, got {value:?}")
        })
    };
    let system_number =
        |section: &str, key: &str| number(&system_fields, "SYSTEM.INI", section, key);
    // `{key}_xco{slot}` and `{key}_yco{slot}`; only the power-up slots have a suffix.
    let position = |key: &str, slot: &str| -> anyhow::Result<[i32; 2]> {
        Ok([
            i32::try_from(system_number("system", &format!("{key}_xco{slot}"))?)?,
            i32::try_from(system_number("system", &format!("{key}_yco{slot}"))?)?,
        ])
    };
    let candle_mode = u8::try_from(system_number("screen", "candle_mode")?)
        .ok()
        .filter(|mode| *mode <= 2)
        .context("hud: SYSTEM.INI [screen] candle_mode: expected 0, 1 or 2")?;
    let text_alpha =
        cyrus_text::font_alpha(text(&system_fields, "SYSTEM.INI", "3dfx", "font_sel")?)
            .map_err(|e| anyhow::anyhow!("hud: SYSTEM.INI [3dfx] font_sel: {e}"))?;

    let flag = |section: &str, key: &str| -> anyhow::Result<bool> {
        Ok(system_number(section, key)? != 0)
    };
    // Fixed-size atlases: the runtime holds every frame and draws them at one size.
    let uniform = |bytes: &[u8], name: &str, count: usize| {
        let atlas = parse_gxa_file(bytes).with_context(|| format!("hud: {name}"))?;
        anyhow::ensure!(
            atlas.frames.len() == count
                && atlas
                    .frames
                    .iter()
                    .all(|f| (f.width, f.height) == (atlas.frames[0].width, atlas.frames[0].height)),
            "hud: {name}: expected {count} equally sized frames"
        );
        Ok(atlas)
    };
    let candle = uniform(&candle_bytes, "system/SNUFF.GXA", 81)?;
    let compass = uniform(&compass_bytes, "system/COMPASS2.GXA", 32)?;
    let logbook = uniform(&logbook_bytes, "system/SCROLL.GXA", 32)?;
    let powerup = parse_gxa_file(&powerup_bytes).with_context(|| format!("hud: {powerup_name}"))?;
    anyhow::ensure!(
        (1..=256).contains(&powerup.frames.len()),
        "hud: {powerup_name}: expected 1 to 256 frames"
    );
    let size = |frame: &rgpre::import::gxa::GxaFrame| [frame.width, frame.height];

    let mut world_compass = BTreeMap::new();
    for ((section, key), value) in ini_fields(&worlds, "WORLD.INI")? {
        if let Some(id) = key
            .strip_prefix("world_compass[")
            .and_then(|k| k.strip_suffix(']'))
            .filter(|_| section == "world")
        {
            let context = || format!("hud: WORLD.INI [world] {key} = {value:?}");
            world_compass.insert(
                id.parse::<u32>().with_context(context)?,
                value.parse::<i32>().with_context(context)?,
            );
        }
    }

    let rtx =
        std::fs::read(&rtx_path).with_context(|| format!("hud: reading {}", rtx_path.display()))?;
    let labels = parse_rtx_file(&rtx)
        .with_context(|| format!("hud: {}", rtx_path.display()))?
        .build_label_map();
    let label = |tag: &str| -> anyhow::Result<String> {
        labels
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(tag))
            .map(|(_, text)| text.clone())
            .with_context(|| format!("hud: {}: no {tag} text", rtx_path.display()))
    };
    let atlas = parse_gxa_file(&atlas_bytes).with_context(|| format!("hud: {atlas_name}"))?;
    let item_number = |key: &str| number(&item_fields, "ITEM.INI", "items", key);
    let mut owned = Vec::new();
    for (id, count) in starting_inventory(&item_fields)? {
        if item_number(&format!("hide[{id}]")).unwrap_or(0) == 1 {
            continue;
        }
        let bitmap = item_number(&format!("bitmap[{id}]"))?;
        let frame = usize::try_from(bitmap)
            .ok()
            .and_then(|b| atlas.frames.get(b))
            .with_context(|| {
                format!("hud: ITEM.INI bitmap[{id}] = {bitmap} is not in {atlas_name}")
            })?;
        owned.push(HudItem {
            id,
            bitmap: bitmap as u16,
            count,
            size: [frame.width, frame.height],
        });
    }
    let selected = item_number("start_item_select")
        .ok()
        .and_then(|id| u16::try_from(id).ok())
        .filter(|id| owned.iter().any(|i| i.id == *id));

    let font = cyrus_text::Font::from(&parse_fnt(&font_bytes).context("hud: fonts/REDSEL.FNT")?);
    let keys = KeysIni::parse(std::str::from_utf8(&keys).context("hud: KEYS.INI is not UTF-8")?)
        .map_err(|e| anyhow::anyhow!("hud: KEYS.INI: {e}"))?;
    let hud = HudAsset {
        images: Vec::new(),
        candle_mode,
        candle: position("candle", "")?,
        candle_size: [candle.frames[0].width, candle.frames[0].height],
        pickup: position("pickup", "")?,
        prompt: [
            position("pickup", "")?[0],
            i32::try_from(system_number("system", "pickup_text_yco")?)?,
        ],
        override_ticks: u32::try_from(system_number("game", "rtx_pickup_override_time")?)?,
        compass: position("compass", "")?,
        compass_size: size(&compass.frames[0]),
        world_compass,
        powerup_slots: [position("game", "1")?, position("game", "2")?],
        powerup_sizes: powerup.frames.iter().map(size).collect(),
        logbook: position("logbook", "")?,
        logbook_size: size(&logbook.frames[0]),
        walk_mode: flag("cyrus", "walk_mode")?,
        auto_defend: flag("cyrus", "auto_defend")?,
        notices: [
            [label("#wof")?, label("#won")?],
            [label("#dof")?, label("#don")?],
        ],
        items: owned,
        selected,
        bindings: keys.control_codes(),
        font,
        text_alpha,
    };

    std::fs::create_dir_all(&output)
        .with_context(|| format!("hud: creating {}", output.display()))?;
    if stamp.exists() {
        std::fs::remove_file(&stamp)?;
    }
    let save = |name: String, width: u32, height: u32, rgba: Vec<u8>| -> anyhow::Result<()> {
        let path = output.join(name);
        image::RgbaImage::from_raw(width, height, rgba)
            .context("hud: frame pixel count is invalid")?
            .save(&path)
            .with_context(|| format!("hud: writing {}", path.display()))
    };
    for (name, atlas) in [
        ("candle", candle),
        ("compass", compass),
        ("logbook", logbook),
        ("powerup", powerup),
        ("item", atlas),
    ] {
        for (index, frame) in atlas.frames.into_iter().enumerate() {
            save(
                format!("{name}-{index:02}.png"),
                frame.width.into(),
                frame.height.into(),
                frame.rgba,
            )?;
        }
    }
    std::fs::write(output.join("overlay.hud"), serde_json::to_vec(&hud)?)?;
    std::fs::write(&stamp, fingerprint)?;
    Ok(())
}

fn text<'a>(fields: &'a Fields, file: &str, section: &str, key: &str) -> anyhow::Result<&'a str> {
    fields
        .get(&(section.to_owned(), key.to_owned()))
        .map(String::as_str)
        .with_context(|| format!("hud: {file} [{section}] {key} is missing"))
}

/// New-game counts by item ID, owned items only. Every item starts at `player_total[x]`;
/// each `start_item_list` entry then adds one, capped by `total[x]` and `player_max[x]`.
fn starting_inventory(fields: &Fields) -> anyhow::Result<BTreeMap<u16, u32>> {
    let field = |key: String| -> anyhow::Result<Option<u32>> {
        fields
            .get(&("items".into(), key.clone()))
            .map(|v| v.parse::<u32>())
            .transpose()
            .with_context(|| format!("hud: ITEM.INI [items] {key}"))
    };
    let mut counts = BTreeMap::new();
    for (section, key) in fields.keys() {
        if let Some(id) = key
            .strip_prefix("player_total[")
            .and_then(|k| k.strip_suffix(']'))
            .filter(|_| section == "items")
        {
            let id = id
                .parse::<u16>()
                .with_context(|| format!("hud: ITEM.INI [items] {key}"))?;
            counts.insert(id, field(key.clone())?.unwrap_or(0));
        }
    }
    let list = text(fields, "ITEM.INI", "items", "start_item_list")?;
    for id in list.split(',') {
        let id = id
            .trim()
            .parse::<u16>()
            .with_context(|| format!("hud: ITEM.INI [items] start_item_list: {list:?}"))?;
        // Pickup side effects need the item system; refuse rather than skip them.
        for list in ["required_item_list", "add_item_list", "drop_add_item_list"] {
            anyhow::ensure!(
                text(fields, "ITEM.INI", "items", &format!("{list}[{id}]"))
                    .map_or(true, str::is_empty),
                "hud: ITEM.INI {list}[{id}] on a starting item is unsupported"
            );
        }
        let count = counts.entry(id).or_insert(0);
        let room = |key: &str| -> anyhow::Result<u32> {
            Ok(field(format!("{key}[{id}]"))?
                .with_context(|| format!("hud: ITEM.INI [items] {key}[{id}] is missing"))?
                .saturating_sub(*count))
        };
        *count += room("total")?.min(room("player_max")?).min(1);
    }
    counts.retain(|_, count| *count > 0);
    Ok(counts)
}

/// `(section, key)` → value, lowercased names, `;` comments removed; the first occurrence wins.
fn ini_fields(bytes: &[u8], file: &str) -> anyhow::Result<Fields> {
    let text = std::str::from_utf8(bytes).with_context(|| format!("hud: {file} is not UTF-8"))?;
    let mut section = String::new();
    let mut fields = BTreeMap::new();
    for line in text.lines() {
        let line = line.split(';').next().unwrap_or_default().trim();
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            section = name.to_ascii_lowercase();
        } else if let Some((key, value)) = line.split_once('=') {
            fields
                .entry((section.clone(), key.trim().to_ascii_lowercase()))
                .or_insert_with(|| value.trim().to_owned());
        }
    }
    Ok(fields)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ini_fields_ignore_comments_and_keep_the_first_value() {
        let fields = ini_fields(
            b"[Items]\n;start_item_select = 9\nstart_item_select = 1 ; note\nstart_item_select = 2\n",
            "ITEM.INI",
        )
        .unwrap();
        assert_eq!(fields[&("items".into(), "start_item_select".into())], "1");
    }

    #[test]
    fn starting_items_add_one_to_player_totals_within_their_caps() {
        let fields = ini_fields(
            b"[items]\nstart_item_list = 1, 2, 4\n\
              player_total[1] = 0\ntotal[1] = 1\nplayer_max[1] = 1\n\
              player_total[2] = 52\ntotal[2] = 1000000\nplayer_max[2] = 500\n\
              player_total[3] = 0\n\
              player_total[4] = 5\ntotal[4] = 5\nplayer_max[4] = 10\n\
              player_total[5] = 2\n",
            "ITEM.INI",
        )
        .unwrap();
        let counts = starting_inventory(&fields).unwrap();
        assert_eq!(counts, [(1, 1), (2, 53), (4, 5), (5, 2)].into());
        let fields = ini_fields(
            b"[items]\nstart_item_list = 1\nadd_item_list[1] = 2\n",
            "ITEM.INI",
        )
        .unwrap();
        assert!(starting_inventory(&fields).is_err());
    }
}
