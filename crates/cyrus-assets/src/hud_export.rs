//! Installed HUD artwork, font and layout exported for portable presentation.
use crate::hud::{HudAsset, HudItem, InventoryItem};
use anyhow::Context;
use rgpre::import::{
    fnt::parse_fnt,
    gxa::parse_gxa_file,
    item_ini::{ItemIni, ItemType},
    keys_ini::KeysIni,
    rtx::{RtxEntry, parse_rtx_file},
    system_ini::SystemIni,
};
use std::{
    collections::BTreeMap,
    hash::{DefaultHasher, Hash, Hasher},
    path::Path,
};

type Fields = BTreeMap<(String, String), String>;

fn rtx_labels(entries: Vec<RtxEntry>) -> std::collections::BTreeMap<String, String> {
    // Activation needs entry presence; build_label_map omits empty captions.
    entries
        .into_iter()
        .map(|entry| {
            let tag = entry.tag_str();
            let text = match entry {
                RtxEntry::Text { text, .. } => text,
                RtxEntry::Audio { label, .. } => label,
            };
            (tag, text)
        })
        .collect()
}

/// Writes `hud/overlay.hud` with the candle, compass, logbook, power-up and item PNGs.
/// The candle, compass, logbook and font file names are fixed by the engine; positions,
/// modes, items and the power-up atlas come from the INI files.
#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
pub fn export_hud(install: &crate::install::GameInstall, cache: &Path) -> anyhow::Result<()> {
    let source = crate::native::Files::scan(install, false)?;
    prepare(&source, &mut crate::preparation::DiskOutput(cache))
}

pub(crate) fn prepare(
    source: &dyn crate::source::ReadAssets,
    output: &mut dyn crate::preparation::Output,
) -> anyhow::Result<()> {
    let read =
        |name: &str| crate::preparation::read(source, name).with_context(|| format!("hud: {name}"));
    let system = read("SYSTEM.INI")?;
    let settings =
        SystemIni::parse(std::str::from_utf8(&system).context("hud: SYSTEM.INI is not UTF-8")?)
            .context("hud: SYSTEM.INI")?;
    let item_path = settings
        .system
        .item_ini
        .as_deref()
        .context("hud: SYSTEM.INI [system] item_ini is missing")?;
    let items = read(item_path)?;
    let definitions = ItemIni::parse(
        std::str::from_utf8(&items).with_context(|| format!("hud: {item_path} is not UTF-8"))?,
    )
    .with_context(|| format!("hud: {item_path}"))?;
    let keys = read("KEYS.INI")?;
    let worlds = read("WORLD.INI")?;
    let system_fields = ini_fields(&system, "SYSTEM.INI")?;
    let atlas_name = definitions
        .settings
        .bitmap_file
        .clone()
        .with_context(|| format!("hud: {item_path} bitmap_file missing"))?;
    let powerup_name = text(&system_fields, "SYSTEM.INI", "system", "game_bitmap")?.to_owned();
    let candle_bytes = read("system/SNUFF.GXA")?;
    let compass_bytes = read("system/COMPASS2.GXA")?;
    let logbook_bytes = read("system/SCROLL.GXA")?;
    let powerup_bytes = read(&powerup_name)?;
    let atlas_bytes = read(&atlas_name)?;
    let font_bytes = read("fonts/REDSEL.FNT")?;
    let rtx_path = crate::preparation::path(
        source,
        text(&system_fields, "SYSTEM.INI", "system", "rtx_filename")?,
    )?;
    let rtx_stamp = &source
        .manifest()
        .find(&rtx_path)
        .context("hud: RTX missing")?
        .digest;

    let directory = Path::new("hud");
    let stamp = directory.join("source.txt");
    let mut fingerprint = DefaultHasher::new();
    (
        11u32,
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
    if output
        .read(&stamp)
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .as_deref()
        == Some(&fingerprint)
        && output.exists(&directory.join("overlay.hud"))
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

    let player_text_alpha =
        cyrus_text::font_alpha(text(&system_fields, "SYSTEM.INI", "3dfx", "font_norm")?)
            .map_err(|e| anyhow::anyhow!("hud: SYSTEM.INI [3dfx] font_norm: {e}"))?;

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

    let rtx = source
        .read(&rtx_path)
        .with_context(|| format!("hud: reading {}", rtx_path))?;
    let labels = rtx_labels(
        parse_rtx_file(&rtx)
            .with_context(|| format!("hud: {}", rtx_path))?
            .entries,
    );
    let label = |tag: &str| -> anyhow::Result<String> {
        labels
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(tag))
            .map(|(_, text)| text.clone())
            .with_context(|| format!("hud: {}: no {tag} text", rtx_path))
    };
    let atlas = parse_gxa_file(&atlas_bytes).with_context(|| format!("hud: {atlas_name}"))?;
    let mut inventory = BTreeMap::new();
    let mut artwork = Vec::new();
    for (id, item) in &definitions.items {
        let id =
            u16::try_from(*id).with_context(|| format!("hud: {item_path} item ID exceeds u16"))?;

        let unsigned = |value: Option<i32>, field: &str| -> anyhow::Result<u32> {
            u32::try_from(value.unwrap_or(0))
                .with_context(|| format!("hud: {item_path} {field}[{id}] must be nonnegative"))
        };
        let count = unsigned(item.player_total, "player_total")?;
        let list = |values: &Option<Vec<u32>>, field: &str| -> anyhow::Result<Vec<u16>> {
            values
                .as_deref()
                .unwrap_or_default()
                .iter()
                .map(|&value| {
                    u16::try_from(value).with_context(|| {
                        format!("hud: {item_path} {field}[{id}]: item {value} exceeds u16")
                    })
                })
                .collect()
        };
        inventory.insert(
            id,
            InventoryItem {
                weapon: matches!(item.item_type, Some(ItemType::Weapon)),
                count,
                total: unsigned(item.total, "total")?,
                player_max: unsigned(item.player_max, "player_max")?,
                hidden: item.hide.unwrap_or(false),
                flags: item.flags.unwrap_or(0),
                required_items: list(&item.required_item_list, "required_item_list")?,
                drop_add_items: list(&item.drop_add_item_list, "drop_add_item_list")?,
                add_items: list(&item.add_item_list, "add_item_list")?,
                remove_drop_items: list(&item.remove_drop_item_list, "remove_drop_item_list")?,
            },
        );
        if item.hide.unwrap_or(false) {
            continue;
        }
        let bitmap = item
            .bitmap
            .with_context(|| format!("hud: {item_path} bitmap[{id}] missing"))?;
        let frame = atlas.frames.get(bitmap as usize).with_context(|| {
            format!("hud: {item_path} bitmap[{id}] = {bitmap} is not in {atlas_name}")
        })?;
        artwork.push(HudItem {
            id,
            bitmap: u16::try_from(bitmap).context("hud: item bitmap exceeds u16")?,
            count,
            size: [frame.width, frame.height],
        });
    }
    let selected = definitions
        .settings
        .start_item_select
        .map(u16::try_from)
        .transpose()
        .with_context(|| format!("hud: {item_path} initial item ID exceeds u16"))?;

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
        items: artwork,
        inventory,
        start_items: definitions
            .settings
            .start_item_list
            .as_deref()
            .unwrap_or_default()
            .iter()
            .map(|&id| u16::try_from(id).context("hud: starting item ID exceeds u16"))
            .collect::<anyhow::Result<_>>()?,
        selected,
        bindings: keys.control_codes(),
        font,
        text_alpha,
        player_text_alpha,
        labels,
    };

    output.remove(&stamp)?;
    let mut save = |name: String, width: u32, height: u32, rgba: Vec<u8>| -> anyhow::Result<()> {
        let path = directory.join(name);
        let image = image::RgbaImage::from_raw(width, height, rgba)
            .context("hud: frame pixel count is invalid")?;
        output.image(&path, &image)
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
    output.publish(&directory.join("overlay.hud"), &serde_json::to_vec(&hud)?)?;
    output.publish(&stamp, fingerprint.as_bytes())?;
    Ok(())
}

fn text<'a>(fields: &'a Fields, file: &str, section: &str, key: &str) -> anyhow::Result<&'a str> {
    fields
        .get(&(section.to_owned(), key.to_owned()))
        .map(String::as_str)
        .with_context(|| format!("hud: {file} [{section}] {key} is missing"))
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
    #[test]
    fn activation_label_export_retains_empty_text_entries() {
        let labels = super::rtx_labels(vec![rgpre::import::rtx::RtxEntry::Text {
            tag: *b"#nil",
            text: String::new(),
        }]);
        assert_eq!(labels.get("#nil").map(String::as_str), Some(""));
    }

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
    fn preparation_reads_configured_item_database() {
        use crate::source::{FileInfo, Manifest, ReadAssets};
        struct Source {
            manifest: Manifest,
            files: BTreeMap<String, Vec<u8>>,
        }
        impl ReadAssets for Source {
            fn manifest(&self) -> &Manifest {
                &self.manifest
            }
            fn read_range(&self, path: &str, start: u64, length: usize) -> anyhow::Result<Vec<u8>> {
                Ok(self.files[path][start as usize..start as usize + length].to_vec())
            }
        }
        let files: BTreeMap<String, Vec<u8>> = [
            (
                "SYSTEM.INI".into(),
                b"[system]\nitem_ini=alternate.ini\n".to_vec(),
            ),
            (
                "alternate.ini".into(),
                b"[items]\nplayer_total[1]=broken\n".to_vec(),
            ),
            ("ITEM.INI".into(), b"[items]\n".to_vec()),
        ]
        .into();
        let source = Source {
            manifest: Manifest {
                files: files
                    .iter()
                    .map(|(path, bytes)| FileInfo {
                        path: path.clone(),
                        bytes: bytes.len() as u64,
                        digest: "0".repeat(64),
                    })
                    .collect(),
            },
            files,
        };
        let mut output = BTreeMap::<String, Vec<u8>>::new();
        let error = prepare(&source, &mut output).unwrap_err();
        let error = format!("{error:#}");
        assert!(error.contains("alternate.ini"), "{error}");
        assert!(error.contains("player_total"), "{error}");
    }
}
