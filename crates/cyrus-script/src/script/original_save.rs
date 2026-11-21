//! Explicitly limited original-save import into initialized, off-live native state.
use super::{GlobalFlagsSnapshot, Inventory, ScriptHud, ScriptWorld};
use anyhow::{Context, Result, ensure};
use cyrus_assets::hud::HudAsset;
use cyrus_soup::program::Definition;
use rgpre::import::savegame::{
    SaveGame, parse_save_flags, parse_save_inventory, parse_save_objects,
};

/// This import stages gameplay scalars, not an original interpreter continuation.
/// Hosts must show this limitation and must not advertise a full session restore.
pub const LIMITATION: &str = "Limited original-save import: world, player position, heading, health, inventory and global flags only. Script/task continuations, other actors, animation, controller state, item-use timers, clock and journal are unsupported; the host supplies the native baseline for unsupported owners.";

/// Validated replacement values. Apply only to an initialized off-live world and
/// HUD, and discard both on error. No files are written and no scripts are run.
pub struct OriginalSaveSubset {
    /// Installed WORLD.INI identity required by the candidate.
    pub world: u32,
    /// Signed map coordinates with eight fractional bits.
    pub position_q8: [i32; 3],
    /// Heading wrapped into one revolution of 2048 units.
    pub heading: u16,
    /// Retained player health, validated to fit the native attribute byte.
    pub health: u8,
    inventory: Inventory,
    flags: GlobalFlagsSnapshot,
}

impl OriginalSaveSubset {
    /// Decode through the published save container/object/inventory/flag readers.
    /// Only the base player and unlatched flags are currently supported.
    pub fn decode(game: &SaveGame, hud: &HudAsset, definition: &Definition) -> Result<Self> {
        let section = |name: [u8; 4]| -> Result<&[u8]> {
            let mut found = game.sections.iter().filter(|section| section.name == name);
            let data = &found
                .next()
                .with_context(|| {
                    format!(
                        "original save world {}: missing {}",
                        game.metadata.world_id,
                        String::from_utf8_lossy(&name)
                    )
                })?
                .data;
            ensure!(
                found.next().is_none(),
                "original save world {}: duplicate {}",
                game.metadata.world_id,
                String::from_utf8_lossy(&name)
            );
            Ok(data)
        };
        let globals = section(*b"SVG2")?;
        ensure!(
            globals.len() == 1666,
            "original save world {} SVG2: expected 1666 bytes, got {}",
            game.metadata.world_id,
            globals.len()
        );
        // Packed retained health and selected-player fields, SAVEGAME engine globals.
        let health = i32::from_le_bytes(globals[..4].try_into()?)
            .try_into()
            .context("original save SVG2 health: outside native attribute range")?;
        let selection = i32::from_le_bytes(globals[1622..1626].try_into()?);
        ensure!(
            selection == 0,
            "original save SVG2 selected player {selection}: transformed player unsupported by limited import"
        );
        let objects = parse_save_objects(section(*b"SMOB")?)?;
        let mut players = objects.iter().filter(|object| object.object_id == -2);
        let player = players
            .next()
            .context("original save SMOB: base player missing")?;
        ensure!(
            players.next().is_none(),
            "original save SMOB: duplicate base player"
        );
        // Packed heading is the retained orientation's middle signed word.
        // SAVEGAME object orientation is modulo 2048, like the initialized actor.
        let heading = (i32::from_le_bytes(player.body[292..296].try_into()?) & 2047) as u16;
        let inventory = section(*b"SVIT")?;
        let inventory = parse_save_inventory(
            inventory,
            hud.inventory
                .len()
                .checked_sub(1)
                .context("original save SVIT: empty installed item list")?,
        )?;
        let flags = parse_save_flags(section(*b"SVGF")?)?;
        Ok(Self {
            world: game.metadata.world_id,
            position_q8: player.position_q8(),
            heading,
            health,
            inventory: Inventory::from_original(hud, &inventory)?,
            flags: GlobalFlagsSnapshot::from_original(definition, &flags)?,
        })
    }

    /// Apply to the matching initialized candidate. The returned message must be
    /// presented by the host; success does not mean continuations were restored.
    pub fn apply(self, world: &mut ScriptWorld, hud: &ScriptHud) -> Result<&'static str> {
        ensure!(
            world.world == self.world,
            "original save world {}: candidate world {} differs",
            self.world,
            world.world
        );
        world.restore_original_player(self.position_q8, self.heading, self.health, self.flags)?;
        hud.replace_original_inventory(self.inventory)?;
        Ok(LIMITATION)
    }
}
