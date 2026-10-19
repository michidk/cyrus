//! Installed weapon model identities and original signed ROB length inputs.
//! Verified source rules: [preservation attachment documentation].
//!
//! [preservation attachment documentation]: https://github.com/michidk/redguard-preservation/blob/079a83339850bf0e3e40c85fb78ccac3f74d48b7/docs/engine/attachment.md
use serde::{Deserialize, Serialize};
#[cfg(feature = "parsers")]
use std::collections::BTreeMap;

/// Original item geometry, independent of presentation meshes and renderer bounds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Weapon {
    /// Uppercase model stem selected by ITEM.INI hand_object_file.
    pub hand_model: String,
    /// Uppercase model stem selected by ITEM.INI hilt_object_file.
    pub hilt_model: String,
    /// Hand-model ROB Y extent interpreted as signed map units, before Q8 conversion.
    pub length: i32,
}

/// Prepare weapon geometry from the selected world's primary ROB registry.
/// World loading replaces the configured map extension before item initialization.
/// Reuse loaded registry bytes when supplied; standalone exports read the source.
/// Missing models fail explicitly; presentation's secondary registry is host-owned.
#[cfg(feature = "parsers")]
pub fn prepare_world_source(
    source: &dyn crate::source::ReadAssets,
    system: &rgpre::import::system_ini::SystemIni,
    world: u32,
    registry: Option<&rgpre::import::registry::Registry>,
) -> anyhow::Result<BTreeMap<u16, Weapon>> {
    use anyhow::Context;
    let item_path = system
        .system
        .item_ini
        .as_deref()
        .context("weapon geometry: SYSTEM.INI [system] item_ini absent")?;
    let items =
        rgpre::import::item_ini::ItemIni::parse(&crate::preparation::text(source, item_path)?)?;
    if !items
        .items
        .values()
        .any(|item| item.item_type == Some(rgpre::import::item_ini::ItemType::Weapon))
    {
        return Ok(BTreeMap::new());
    }
    let archive = world_archive_path(source, world)?;
    let loaded;
    let bytes = if let Some(registry) = registry {
        let entry = registry
            .files
            .values()
            .find(|entry| {
                entry
                    .path
                    .to_string_lossy()
                    .replace('\\', "/")
                    .eq_ignore_ascii_case(&archive)
            })
            .with_context(|| {
                format!("world {world} weapon geometry: archive {archive} absent from registry")
            })?;
        entry.data.as_deref().with_context(|| {
            format!("world {world} weapon geometry: archive {archive} bytes absent")
        })?
    } else {
        loaded = source.read(&archive)?;
        &loaded
    };
    let models = rgpre::import::rob::parse_rob_file(bytes)
        .with_context(|| format!("world {world} weapon geometry: {archive}"))?;
    prepare(&items, &models.segments)
        .with_context(|| format!("world {world} weapon geometry: {archive}"))
}

#[cfg(feature = "parsers")]
pub(crate) fn world_archive_path(
    source: &dyn crate::source::ReadAssets,
    world: u32,
) -> anyhow::Result<String> {
    use anyhow::Context;
    let worlds =
        rgpre::import::world_ini::WorldIni::parse(&crate::preparation::text(source, "WORLD.INI")?);
    let entry = worlds
        .entries
        .iter()
        .find(|entry| entry.index == world)
        .with_context(|| format!("world {world}: WORLD.INI map absent for weapon geometry"))?;
    let map = entry.map.replace('\\', "/");
    let filename = std::path::Path::new(&map)
        .file_name()
        .context("weapon geometry: world map filename absent")?;
    let archive = std::path::Path::new(filename).with_extension("ROB");
    let archive = archive
        .to_str()
        .context("weapon archive path is not UTF-8")?;
    Ok(format!("fxart/{archive}"))
}

#[cfg(feature = "parsers")]
fn prepare(
    items: &rgpre::import::item_ini::ItemIni,
    segments: &[rgpre::import::rob::RobSegment],
) -> anyhow::Result<BTreeMap<u16, Weapon>> {
    use anyhow::Context;
    use rgpre::import::item_ini::ItemType;
    let mut weapons = BTreeMap::new();
    for (&id, item) in &items.items {
        if item.item_type != Some(ItemType::Weapon) {
            continue;
        }
        let resolve = |field: &str, path: Option<&str>| {
            let path = path.with_context(|| format!("item {id}: {field} absent"))?;
            let name = crate::animation::model_key(path);
            let segment = segments
                .iter()
                .find(|segment| crate::animation::model_key(&segment.name()) == name)
                .with_context(|| format!("item {id}: {field} {path:?}: ROB model {name} absent"))?;
            Ok::<_, anyhow::Error>((name, segment))
        };
        let (hand_model, hand) = resolve("hand_object_file", item.hand_object_file.as_deref())?;
        let (hilt_model, _) = resolve("hilt_object_file", item.hilt_object_file.as_deref())?;
        weapons.insert(
            u16::try_from(id).with_context(|| format!("item {id}: ID outside u16 range"))?,
            Weapon {
                hand_model,
                hilt_model,
                length: hand.bbox.extent_y as i32,
            },
        );
    }
    Ok(weapons)
}

#[cfg(all(test, feature = "parsers"))]
mod tests {
    use super::*;
    use rgpre::import::{
        item_ini::ItemIni,
        rob::{RobSegment, SegmentBBox},
    };

    #[test]
    fn world_archive_uses_portable_source_key() {
        use crate::source::{FileInfo, Manifest, ReadAssets};
        struct Source(Manifest);
        impl ReadAssets for Source {
            fn manifest(&self) -> &Manifest {
                &self.0
            }
            fn read_range(&self, path: &str, start: u64, length: usize) -> anyhow::Result<Vec<u8>> {
                assert_eq!(path, "WORLD.INI");
                let bytes =
                    b"[World]\nworld_map[0]=maps\\start.RGM\nworld_palette[0]=3dart/test.COL\n";
                Ok(bytes[start as usize..start as usize + length].to_vec())
            }
        }
        let source = Source(Manifest {
            files: vec![
                FileInfo {
                    path: "WORLD.INI".into(),
                    bytes:
                        b"[World]\nworld_map[0]=maps\\start.RGM\nworld_palette[0]=3dart/test.COL\n"
                            .len() as u64,
                    digest: "0".repeat(64),
                },
                FileInfo {
                    path: "fxart/START.ROB".into(),
                    bytes: 0,
                    digest: "0".repeat(64),
                },
            ],
        });
        let path = world_archive_path(&source, 0).unwrap();
        assert_eq!(path, "fxart/start.ROB");
        assert!(source.manifest().find(&path).is_some());
    }

    fn segment(name: [u8; 8], y: u32) -> RobSegment {
        RobSegment {
            total_size: 0,
            segment_name: name,
            segment_type: 0,
            segment_flags: 0,
            segment_attribs: 0,
            _face_count_low: [0; 3],
            _unused_14: 0,
            bbox: SegmentBBox {
                extent_x: 999,
                extent_y: y,
                extent_z: 777,
                positive_x: 0,
                positive_y: 0,
                positive_z: 0,
                negative_x: 0,
                negative_y: 0,
                negative_z: 0,
            },
            data_size: 0,
            data: vec![],
        }
    }

    #[test]
    fn original_names_and_signed_y_extent_prepare_without_renderer_bounds() {
        let ini = ItemIni::parse("[ITEMS]\ntype[1]=1\nhand_object_file[1]=3DART\\BLADE.3D\nhilt_object_file[1]=3dart\\HILT.3D\ntype[2]=0\n").unwrap();
        let models = [
            segment(*b"BLADE\0\0\0", u32::MAX),
            segment(*b"HILT\0\0\0\0", 42),
        ];
        assert_eq!(
            prepare(&ini, &models).unwrap(),
            BTreeMap::from([(
                1,
                Weapon {
                    hand_model: "BLADE".into(),
                    hilt_model: "HILT".into(),
                    length: -1,
                }
            )])
        );
        let error = prepare(&ini, &models[..1]).unwrap_err();
        assert!(error.to_string().contains("item 1: hilt_object_file"));
        let mut later = models[0].clone();
        later.bbox.extent_y = 42;
        let duplicate = [models[0].clone(), later, models[1].clone()];
        assert_eq!(prepare(&ini, &duplicate).unwrap()[&1].length, -1);
    }
}
