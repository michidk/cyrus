//! Native current/retained-world authority for complete original-slot assembly.
use super::{
    CompleteSlot, CompleteSlotValidator,
    assembly::{SavedImages, assemble_with_images},
};
use anyhow::{Context, Result, ensure};
use rgpre::import::{savegame::SaveSlot, world_ini::WorldIni};
use std::collections::{BTreeMap, BTreeSet};

/// File/world identities resolved from a native session and the installed catalog.
///
/// This admits only the resource census. All record contents, journal state and
/// cross-record inverse mappings still require a complete semantic validator.
#[derive(Debug)]
pub struct WorldFiles {
    current: u32,
    files: BTreeMap<String, u32>,
}

impl WorldFiles {
    /// Capture the active and retained world identities from an immutable native session.
    pub fn from_session(
        session: &cyrus_session::RuntimeSessionSnapshot,
        worlds: &WorldIni,
    ) -> Result<Self> {
        Self::resolve(session.current_world(), session.world_ids(), worlds)
    }

    /// Resolve an authoritative complete world census, including the current world.
    ///
    /// Off-live producers must supply the native retained-world census, never infer
    /// it from the proposed slot. Installed map paths determine TSG filenames;
    /// numeric world identity is retained even when catalog map names alias.
    pub fn resolve(
        current: u32,
        ids: impl IntoIterator<Item = u32>,
        worlds: &WorldIni,
    ) -> Result<Self> {
        let resolve = || -> Result<Self> {
            let mut seen = BTreeSet::new();
            let mut files = BTreeMap::from([("SAVEGAME.SAV".to_owned(), current)]);
            for id in ids {
                ensure!(
                    seen.insert(id),
                    "native world {id}: duplicate retained identity"
                );
                let mut matches = worlds.entries.iter().filter(|entry| entry.index == id);
                let entry = matches
                    .next()
                    .with_context(|| format!("WORLD.INI world {id}: native world unavailable"))?;
                ensure!(
                    matches.next().is_none(),
                    "WORLD.INI world {id}: ambiguous numeric identity"
                );
                let map = entry.map.replace('\\', "/");
                let path = std::path::Path::new(&map);
                ensure!(
                    path.extension()
                        .and_then(|ext| ext.to_str())
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("rgm")),
                    "WORLD.INI world {id}: unsupported map path {:?}",
                    entry.map
                );
                let stem = path
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .with_context(|| format!("WORLD.INI world {id}: map stem unavailable"))?;
                let filename = format!("{}.TSG", stem.to_ascii_uppercase());
                super::validate_map_name(&filename)
                    .with_context(|| format!("WORLD.INI world {id}: map snapshot filename"))?;
                if id != current {
                    ensure!(
                        !files.contains_key(&filename),
                        "{filename} world {id}: ambiguous retained world filename already owned by world {:?}",
                        files.get(&filename)
                    );
                    files.insert(filename, id);
                }
            }
            ensure!(
                seen.contains(&current),
                "native current world {current} absent from complete retained census"
            );
            Ok(Self { current, files })
        };
        resolve().with_context(|| format!("SAVEGAME.SAV world {current}: native world-file census"))
    }

    /// Validate native world/file ownership, then assemble images and all remaining state.
    ///
    /// Missing, extra and misidentified resources reject before destination I/O.
    /// Historical snapshots outside the native census are unsupported rather than
    /// silently discarded. This does not create records or bypass any residual
    /// inverse validator. Pass the result through `ExportRequest` for player consent.
    pub fn assemble(
        &self,
        slot: SaveSlot,
        images: SavedImages,
        remaining: &impl CompleteSlotValidator,
    ) -> Result<CompleteSlot> {
        (|| {
            ensure!(
                slot.game.metadata.world_id == self.current,
                "SAVEGAME.SAV: supplied active world {} differs from native current world {}",
                slot.game.metadata.world_id,
                self.current
            );
            let actual = std::iter::once(("SAVEGAME.SAV", &slot.game))
                .chain(slot.maps.iter().map(|(name, game)| (name.as_str(), game)))
                .map(|(name, game)| (name.to_owned(), game.metadata.world_id))
                .collect::<BTreeMap<_, _>>();
            for (file, world) in &self.files {
                let supplied = actual.get(file).with_context(|| {
                    format!("{file} world {world}: required native snapshot missing")
                })?;
                ensure!(
                    supplied == world,
                    "{file} world {world}: snapshot metadata identifies world {supplied}"
                );
            }
            for (file, world) in &actual {
                ensure!(
                    self.files.contains_key(file),
                    "{file} world {world}: snapshot has no native retained owner"
                );
            }
            assemble_with_images(slot, images, remaining)
        })()
        .with_context(|| {
            format!(
                "SAVEGAME.SAV world {}: native world-file assembly",
                self.current
            )
        })
    }
}
