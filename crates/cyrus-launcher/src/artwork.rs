//! The original launcher's artwork, read from the player's own `REDGUARD.EXE`.
//!
//! cyrus ships none of it. The launcher program (the CD's and GOG's `REDGUARD.EXE`, a Win32 program)
//! keeps its background, its six buttons in three states each and its sounds as ordinary PE
//! resources, so they are read from the player's copy when the launcher starts. Ported from the
//! maiko launcher (MIT, same author); the reader belongs in redguard-preservation, which has no
//! launcher-resource reader yet.
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Resource names of the six buttons, in the original's top-to-bottom order.
pub const BUTTONS: [&str; 6] = [
    "PLAYREDGUARD",
    "GAMEOPTIONS",
    "TECHSUPPORT",
    "BETHSOFT",
    "UNINSTALL",
    "CANCEL",
];
/// The top-left corner of each button on the 640 by 480 background.
pub const BUTTON_POSITIONS: [(u32, u32); 6] = [
    (240, 184),
    (345, 243),
    (276, 287),
    (278, 335),
    (442, 384),
    (484, 426),
];
/// The resource id of the background bitmap.
const BACKGROUND: u32 = 130;

/// A decoded bitmap: width, height and RGBA rows from the top.
#[derive(Debug, Clone, PartialEq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// One button's bitmaps: up, focused (the mouse is over it) and down.
#[derive(Debug, Clone)]
pub struct Button {
    pub up: Image,
    pub focus: Image,
    pub down: Image,
}

#[derive(Debug, Clone)]
pub struct Artwork {
    pub background: Image,
    pub buttons: Vec<Button>,
    /// `RATTLE1`..`RATTLE3` (focus moves to a button) and `SCRAPE2`, as RIFF WAVE files.
    pub sounds: HashMap<String, Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Name {
    Id(u32),
    Text(String),
}

/// The original launcher in a game folder, whatever the case of its name.
pub fn launcher_in(game: &Path) -> Option<PathBuf> {
    std::fs::read_dir(game).ok()?.flatten().find_map(|entry| {
        let name = entry.file_name();
        name.to_string_lossy()
            .eq_ignore_ascii_case("REDGUARD.EXE")
            .then(|| entry.path())
    })
}

impl Artwork {
    /// Read the artwork from the game folder's `REDGUARD.EXE`; `None` if it is missing or is not
    /// the original launcher.
    pub fn load(game: &Path) -> Option<Self> {
        Self::parse(&std::fs::read(launcher_in(game)?).ok()?)
    }

    pub fn parse(exe: &[u8]) -> Option<Self> {
        let resources = resources(exe)?;
        let bitmap = |name: Name| decode_bitmap(resources.get(&(2, name))?);
        let background = bitmap(Name::Id(BACKGROUND))?;
        if (background.width, background.height) != (640, 480) {
            return None;
        }
        let buttons = BUTTONS
            .iter()
            .map(|name| {
                let state = |suffix| bitmap(Name::Text(format!("{name}{suffix}")));
                Some(Button {
                    up: state("U")?,
                    focus: state("F")?,
                    down: state("D")?,
                })
            })
            .collect::<Option<Vec<_>>>()?;
        let sounds = resources
            .iter()
            .filter_map(|((kind, name), data)| match (kind, name) {
                (0, Name::Text(name)) => Some((name.clone(), data.clone())),
                _ => None,
            })
            .filter(|(_, data)| data.starts_with(b"RIFF"))
            .collect();
        Some(Self {
            background,
            buttons,
            sounds,
        })
    }
}

fn u16_at(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(data.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(data.get(at..at + 4)?.try_into().ok()?))
}

/// Every resource as ((type, name), bytes). Bitmaps are type 2. Named types (the launcher's
/// `WAVE`) are reported as type 0.
fn resources(exe: &[u8]) -> Option<HashMap<(u32, Name), Vec<u8>>> {
    if exe.get(..2)? != b"MZ" {
        return None;
    }
    let pe = u32_at(exe, 0x3c)? as usize;
    if exe.get(pe..pe + 4)? != b"PE\0\0" {
        return None;
    }
    let sections = u16_at(exe, pe + 6)? as usize;
    let optional = pe + 24;
    let optional_size = u16_at(exe, pe + 20)? as usize;
    // PE32: the data directories start 96 bytes into the optional header; resources are entry 2.
    if u16_at(exe, optional)? != 0x10b {
        return None;
    }
    let resource_rva = u32_at(exe, optional + 96 + 2 * 8)?;
    let table: Vec<(u32, u32, u32)> = (0..sections)
        .map(|i| {
            let at = optional + optional_size + i * 40;
            Some((
                u32_at(exe, at + 12)?,
                u32_at(exe, at + 8)?.max(u32_at(exe, at + 16)?),
                u32_at(exe, at + 20)?,
            ))
        })
        .collect::<Option<_>>()?;
    let offset = |rva: u32| {
        table
            .iter()
            .find(|(start, size, _)| rva >= *start && rva - start < *size)
            .map(|(start, _, raw)| (rva - start + raw) as usize)
    };
    let base = offset(resource_rva)?;
    let name = |entry: u32| -> Option<Name> {
        if entry & 0x8000_0000 == 0 {
            return Some(Name::Id(entry));
        }
        let at = base + (entry & 0x7fff_ffff) as usize;
        let length = u16_at(exe, at)? as usize;
        let units: Vec<u16> = (0..length)
            .map(|i| u16_at(exe, at + 2 + i * 2))
            .collect::<Option<_>>()?;
        Some(Name::Text(String::from_utf16(&units).ok()?))
    };
    let entries = |directory: usize| -> Option<Vec<(u32, u32)>> {
        let count = u16_at(exe, directory + 12)? as usize + u16_at(exe, directory + 14)? as usize;
        (0..count.min(4096))
            .map(|i| {
                let at = directory + 16 + i * 8;
                Some((u32_at(exe, at)?, u32_at(exe, at + 4)?))
            })
            .collect()
    };
    let mut found = HashMap::new();
    // Three levels: type, name, language. The first language of each resource is used.
    for (kind, kind_at) in entries(base)? {
        let kind = match name(kind)? {
            Name::Id(id) => id,
            Name::Text(_) => 0,
        };
        if kind_at & 0x8000_0000 == 0 {
            continue;
        }
        for (resource, resource_at) in entries(base + (kind_at & 0x7fff_ffff) as usize)? {
            if resource_at & 0x8000_0000 == 0 {
                continue;
            }
            let Some(&(_, data_at)) = entries(base + (resource_at & 0x7fff_ffff) as usize)?.first()
            else {
                continue;
            };
            let at = base + data_at as usize;
            let (rva, size) = (u32_at(exe, at)?, u32_at(exe, at + 4)? as usize);
            let start = offset(rva)?;
            found.insert(
                (kind, name(resource)?),
                exe.get(start..start + size)?.to_vec(),
            );
        }
    }
    Some(found)
}

/// Decode a resource bitmap (a BITMAPINFOHEADER without the file header): 4- and 8-bit indexed
/// and 24-bit, uncompressed, bottom-up or top-down.
fn decode_bitmap(data: &[u8]) -> Option<Image> {
    let header = u32_at(data, 0)? as usize;
    let width = i32::from_le_bytes(data.get(4..8)?.try_into().ok()?);
    let height = i32::from_le_bytes(data.get(8..12)?.try_into().ok()?);
    let bits = u16_at(data, 14)?;
    if u32_at(data, 16)? != 0 || width <= 0 || height == 0 || width > 4096 || height.abs() > 4096 {
        return None;
    }
    let (width, rows) = (width as usize, height.unsigned_abs() as usize);
    let colors = match (bits, u32_at(data, 32)?) {
        (4 | 8, 0) => 1 << bits,
        (4 | 8, used) => used as usize,
        (24, _) => 0,
        _ => return None,
    };
    let palette = data.get(header..header + colors * 4)?;
    let pixels = header + colors * 4;
    let stride = (width * bits as usize).div_ceil(32) * 4;
    let mut rgba = Vec::with_capacity(width * rows * 4);
    for y in 0..rows {
        let source = if height > 0 { rows - 1 - y } else { y };
        let row = data.get(pixels + source * stride..pixels + (source + 1) * stride)?;
        for x in 0..width {
            let (b, g, r) = match bits {
                24 => (row[x * 3], row[x * 3 + 1], row[x * 3 + 2]),
                _ => {
                    let index = if bits == 8 {
                        row[x]
                    } else {
                        (row[x / 2] >> if x % 2 == 0 { 4 } else { 0 }) & 15
                    } as usize;
                    let color = palette.get(index * 4..index * 4 + 3)?;
                    (color[0], color[1], color[2])
                }
            };
            rgba.extend([r, g, b, 255]);
        }
    }
    Some(Image {
        width: width as u32,
        height: rows as u32,
        rgba,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An 8-bit bitmap resource, bottom-up, two colours.
    fn bitmap(width: u32, height: u32) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend(40u32.to_le_bytes());
        data.extend((width as i32).to_le_bytes());
        data.extend((height as i32).to_le_bytes());
        data.extend(1u16.to_le_bytes());
        data.extend(8u16.to_le_bytes());
        data.extend([0; 16]);
        data.extend(2u32.to_le_bytes());
        data.extend([0; 4]);
        data.extend([0, 0, 255, 0, 255, 0, 0, 0]); // red, blue (BGRA)
        let stride = (width as usize).div_ceil(4) * 4;
        for y in 0..height {
            // The bottom row (stored first) is colour 1.
            let mut row = vec![u8::from(y == 0); width as usize];
            row.resize(stride, 0);
            data.extend(row);
        }
        data
    }

    #[test]
    fn bitmaps_decode_top_down_with_their_palette() {
        let image = decode_bitmap(&bitmap(3, 2)).unwrap();
        assert_eq!((image.width, image.height), (3, 2));
        assert_eq!(&image.rgba[..4], &[255, 0, 0, 255], "top row is colour 0");
        assert_eq!(
            &image.rgba[12..16],
            &[0, 0, 255, 255],
            "bottom row is colour 1"
        );
    }

    /// A minimal PE32 with one section holding the resource tree built from `items`.
    fn pe(items: &[(Name, Name, Vec<u8>)]) -> Vec<u8> {
        const RVA: u32 = 0x1000;
        // Resource section: root, then one type directory per item (one name, one language).
        let mut tree = Vec::new();
        let dir = |count: u16, entries: &[(u32, u32)]| {
            let mut out = vec![0; 12];
            let named = entries.iter().filter(|(n, _)| n & 0x8000_0000 != 0).count() as u16;
            out.extend(named.to_le_bytes());
            out.extend((count - named).to_le_bytes());
            for (name, at) in entries {
                out.extend(name.to_le_bytes());
                out.extend(at.to_le_bytes());
            }
            out
        };
        // Layout: root | per item: name directory, language directory, data entry | strings | data.
        let root_size = 16 + 8 * items.len();
        let block = 2 * 24 + 16;
        let strings_at = root_size + block * items.len();
        let mut strings = Vec::new();
        let mut string = |name: &Name| match name {
            Name::Id(id) => *id,
            Name::Text(text) => {
                let at = strings_at + strings.len();
                let units: Vec<u16> = text.encode_utf16().collect();
                strings.extend((units.len() as u16).to_le_bytes());
                units.iter().for_each(|u| strings.extend(u.to_le_bytes()));
                at as u32 | 0x8000_0000
            }
        };
        let names: Vec<(u32, u32)> = items
            .iter()
            .map(|(t, n, _)| (string(t), string(n)))
            .collect();
        let data_at = strings_at + strings.len();
        let mut data: Vec<u8> = Vec::new();
        tree.extend(dir(
            items.len() as u16,
            &names
                .iter()
                .enumerate()
                .map(|(i, (t, _))| (*t, (root_size + block * i) as u32 | 0x8000_0000))
                .collect::<Vec<_>>(),
        ));
        for (i, (_, name)) in names.iter().enumerate() {
            let at = root_size + block * i;
            tree.extend(dir(1, &[(*name, (at + 24) as u32 | 0x8000_0000)]));
            tree.extend(dir(1, &[(1033, (at + 48) as u32)]));
            let bytes = &items[i].2;
            tree.extend((RVA + (data_at + data.len()) as u32).to_le_bytes());
            tree.extend((bytes.len() as u32).to_le_bytes());
            tree.extend([0; 8]);
            data.extend(bytes);
        }
        tree.extend(strings);
        tree.extend(data);
        let mut exe = vec![0u8; 0x400];
        exe[..2].copy_from_slice(b"MZ");
        exe[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        exe[0x80..0x84].copy_from_slice(b"PE\0\0");
        exe[0x86..0x88].copy_from_slice(&1u16.to_le_bytes());
        exe[0x94..0x96].copy_from_slice(&224u16.to_le_bytes());
        let optional = 0x98;
        exe[optional..optional + 2].copy_from_slice(&0x10bu16.to_le_bytes());
        exe[optional + 112..optional + 116].copy_from_slice(&RVA.to_le_bytes());
        let section = optional + 224;
        exe[section + 8..section + 12].copy_from_slice(&(tree.len() as u32).to_le_bytes());
        exe[section + 12..section + 16].copy_from_slice(&RVA.to_le_bytes());
        exe[section + 16..section + 20].copy_from_slice(&(tree.len() as u32).to_le_bytes());
        exe[section + 20..section + 24].copy_from_slice(&0x400u32.to_le_bytes());
        exe.extend(tree);
        exe
    }

    #[test]
    fn the_launcher_artwork_is_read_from_its_resources() {
        let mut items = vec![(Name::Id(2), Name::Id(BACKGROUND), bitmap(640, 480))];
        for button in BUTTONS {
            for state in ["U", "F", "D"] {
                items.push((
                    Name::Id(2),
                    Name::Text(format!("{button}{state}")),
                    bitmap(4, 2),
                ));
            }
        }
        items.push((
            Name::Text("WAVE".into()),
            Name::Text("RATTLE1".into()),
            b"RIFFdata".to_vec(),
        ));
        let art = Artwork::parse(&pe(&items)).unwrap();
        assert_eq!(art.background.width, 640);
        assert_eq!(art.buttons.len(), 6);
        assert_eq!(art.buttons[5].down.width, 4);
        assert_eq!(art.sounds["RATTLE1"], b"RIFFdata");
        // Without every button the file is not the original launcher.
        items.remove(3);
        assert!(Artwork::parse(&pe(&items)).is_none());
        assert!(Artwork::parse(b"MZ not a launcher").is_none());
    }

    #[test]
    fn the_original_launcher_is_found_whatever_its_case() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(launcher_in(dir.path()), None);
        std::fs::write(dir.path().join("redguard.exe"), b"x").unwrap();
        assert_eq!(
            launcher_in(dir.path()),
            Some(dir.path().join("redguard.exe"))
        );
    }

    /// With the GOG copy present, its real artwork parses (run with REDGUARD_DIR set).
    #[test]
    fn the_installed_launcher_parses() {
        let Some(game) = std::env::var_os("REDGUARD_DIR") else {
            return;
        };
        let art = Artwork::load(Path::new(&game)).expect("REDGUARD.EXE artwork");
        assert_eq!(art.buttons[0].up.width, 395);
        assert!(art.sounds.contains_key("SCRAPE2"));
    }
}
