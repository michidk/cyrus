//! Saved-image assembly composed with mandatory complete-session inverse validation.
use super::{CompleteSlot, CompleteSlotValidator, SlotLayout};
use anyhow::{Context, Result, ensure};
use rgpre::import::savegame::{SaveGame, SaveImageKind, SaveSection, SaveSlot};
use std::collections::BTreeMap;

/// A complete saved-screen/preview pair admitted by preservation's typed decoder.
///
/// Both images must be nonempty and have their format-defined dimensions. Bytes
/// retain their packed framebuffer representation; this does not infer a colour
/// conversion from a native renderer. The capture world cannot be rebound later.
#[derive(Debug)]
pub struct SavedImages {
    world: u32,
    screen: SaveSection,
    thumbnail: SaveSection,
}

impl SavedImages {
    /// Admit SCR3 and THMB from one captured save document without filesystem I/O.
    ///
    /// Each must occur once. Other records are not accepted by this operation.
    /// Dimensions, word layout and duplicate rejection use the published save
    /// image API; see preservation's `docs/formats/SAVEGAME.md#saved-images`.
    pub fn decode(capture: &SaveGame) -> Result<Self> {
        let world = capture.metadata.world_id;
        let record = |kind, name: [u8; 4]| -> Result<SaveSection> {
            let label = String::from_utf8_lossy(&name);
            capture
                .image(kind)
                .with_context(|| format!("SAVEGAME.SAV world {world} {label}: captured image"))?
                .with_context(|| {
                    format!("SAVEGAME.SAV world {world} {label}: complete captured image required")
                })?;
            capture
                .sections
                .iter()
                .find(|record| record.name == name)
                .cloned()
                .context("validated captured image record absent")
        };
        Ok(Self {
            world,
            screen: record(SaveImageKind::Screen, *b"SCR3")?,
            thumbnail: record(SaveImageKind::Thumbnail, *b"THMB")?,
        })
    }
}

/// Assemble the saved-image group and admit a complete immutable slot for export.
///
/// `remaining` must supply the authoritative full file/world/ordered-record census
/// and semantic inverse validation for **every other record**, all cross-record
/// references and journal state. There is no default residual validator. A
/// retained original document alone does not prove a live native inverse mapping.
/// The layout is captured once and shared by assembly and final admission.
///
/// Missing image placeholders are filled; no other missing record is synthesized.
/// Existing duplicate/extra records reject. Visited snapshots and optional journal
/// bytes are retained, and their full census and remaining semantics are checked.
/// Snapshot image chunks may be empty, but nonempty images must decode correctly.
/// No files are read or written; return this payload through `ExportRequest` so
/// publication still requires the matching explicit player decision.
pub fn assemble_with_images(
    mut slot: SaveSlot,
    images: SavedImages,
    remaining: &impl CompleteSlotValidator,
) -> Result<CompleteSlot> {
    let world = slot.game.metadata.world_id;
    (|| {
        ensure!(
            world == images.world,
            "captured image world {} differs from current world {world}",
            images.world
        );
        let layout = remaining.layout().context("complete session census")?;
        let main = layout
            .files
            .get("SAVEGAME.SAV")
            .context("complete census missing SAVEGAME.SAV")?;
        for name in [*b"THMB", *b"SCR3"] {
            ensure!(
                main.records
                    .iter()
                    .filter(|record| **record == name)
                    .count()
                    == 1,
                "{}: complete census must require exactly one saved image",
                String::from_utf8_lossy(&name)
            );
        }
        let mut supplied = BTreeMap::new();
        for record in std::mem::take(&mut slot.game.sections) {
            let name = record.name;
            ensure!(
                main.records.contains(&name),
                "{}: record outside complete census",
                String::from_utf8_lossy(&name)
            );
            ensure!(
                supplied.insert(name, record).is_none(),
                "{}: duplicate supplied record",
                String::from_utf8_lossy(&name)
            );
        }
        let mut replacements =
            BTreeMap::from([(*b"SCR3", images.screen), (*b"THMB", images.thumbnail)]);
        for name in &main.records {
            let record = replacements
                .remove(name)
                .or_else(|| supplied.remove(name))
                .with_context(|| {
                    format!(
                        "{}: required record missing; no inverse state synthesized",
                        String::from_utf8_lossy(name)
                    )
                })?;
            slot.game.sections.push(record);
        }
        for (file, game) in &slot.maps {
            for kind in [SaveImageKind::Screen, SaveImageKind::Thumbnail] {
                game.image(kind).with_context(|| {
                    format!("{file} world {}: snapshot image", game.metadata.world_id)
                })?;
            }
        }
        CompleteSlot::validate(&slot, &ImageValidator { layout, remaining })
    })()
    .with_context(|| format!("SAVEGAME.SAV world {world}: saved-image assembly"))
}

struct ImageValidator<'a, V> {
    layout: SlotLayout,
    remaining: &'a V,
}

impl<V: CompleteSlotValidator> CompleteSlotValidator for ImageValidator<'_, V> {
    fn layout(&self) -> Result<SlotLayout> {
        Ok(self.layout.clone())
    }

    fn validate_record(&self, file: &str, world: u32, record: &SaveSection) -> Result<()> {
        match &record.name {
            // Main images are privately admitted above; snapshot images were
            // decoded before this complete-slot admission. Census is still exact.
            b"THMB" | b"SCR3" => Ok(()),
            _ => self.remaining.validate_record(file, world, record),
        }
    }

    fn validate_slot(&self, slot: &SaveSlot) -> Result<()> {
        self.remaining.validate_slot(slot)
    }
}
