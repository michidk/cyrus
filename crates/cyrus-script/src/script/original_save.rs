//! Explicitly limited original-save import into initialized, off-live native state.
use super::{GlobalFlagsSnapshot, Inventory, ScriptHud, ScriptWorld};
use anyhow::{Context, Result, ensure};
use cyrus_assets::hud::HudAsset;
use cyrus_soup::program::Definition;
use rgpre::import::savegame::{
    SaveClock, SaveGame, parse_save_clock, parse_save_flags, parse_save_inventory,
    parse_save_objects,
};

/// This import stages gameplay scalars, not an original interpreter continuation.
/// Hosts must show this limitation and must not advertise a full session restore.
pub const LIMITATION: &str = "Limited original-save import: world, player position, heading, health, inventory, global flags and clock continuation only. Animation playback with a bound ShowFrame pose is restored. Script/task continuations, other actor state except the static supporting actor, unresolved animation/model or sound-handle bindings, controller state, item-use timers and journal are unsupported; the host supplies the native baseline for unsupported owners.";

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
    support: Option<OriginalSupport>,
    clock: SaveClock,
    animations: Vec<OriginalActorPlayback>,
}

pub(super) struct OriginalActorPlayback {
    pub identity: Option<i16>,
    pub playback: rgpre::import::savegame::SaveAnimationPlayback,
    pub default: cyrus_soup::AnimationGroup,
    pub model_frame: i16,
    pub last_model_frame: i16,
    pub model_handle: i16,
    pub last_model_handle: i16,
}

/// Saved static support, resolved against the initialized world's runtime identities.
pub(super) struct OriginalSupport {
    pub identity: i16,
    pub model: String,
    pub position: [i32; 3],
    pub angles: [i32; 3],
    pub attributes: [u8; 256],
}

fn support_identity(object: &rgpre::import::savegame::SaveObject) -> i32 {
    // Published SMOB framing exposes the packed body. Semantic accessors are
    // proposed in preservation PR #362; use them after its crates.io release (#342).
    // This is the supporting actor identity, not an array index.
    i32::from_le_bytes(object.body[468..472].try_into().unwrap())
}

fn decode_support(
    player: &rgpre::import::savegame::SaveObject,
    objects: &[rgpre::import::savegame::SaveObject],
) -> Result<Option<OriginalSupport>> {
    let identity = support_identity(player);
    if identity == -1 {
        return Ok(None);
    }
    let mut matching = objects.iter().filter(|object| object.object_id == identity);
    let owner = matching
        .next()
        .with_context(|| format!("original save SMOB support {identity}: missing record"))?;
    ensure!(
        matching.next().is_none(),
        "original save SMOB support {identity}: duplicate record"
    );
    let runtime = u16::from_le_bytes(owner.body[20..22].try_into()?);
    ensure!(
        i32::from(runtime) == identity,
        "original save SMOB support {identity}: runtime identity {runtime} differs"
    );
    ensure!(
        owner.script_state.is_none()
            && owner.animation_header.is_none()
            && owner.attachment.is_none()
            && owner.body[33] == 0
            && support_identity(owner) == -1,
        "original save SMOB support {identity}: scripted, animated, attached or chained support unsupported"
    );
    let resource = owner
        .named_resource
        .as_ref()
        .context("original save SMOB support: named static model missing")?;
    ensure!(
        resource.len() == 10 && resource[0] != 0,
        "original save SMOB support {identity}: static model name absent"
    );
    let name = resource[1..].split(|byte| *byte == 0).next().unwrap();
    let model = std::str::from_utf8(name)
        .context("original save SMOB support: invalid model name")?
        .to_owned();
    Ok(Some(OriginalSupport {
        identity: runtime
            .try_into()
            .context("original save SMOB support: runtime identity outside native range")?,
        model,
        position: owner.position_q8(),
        angles: std::array::from_fn(|axis| {
            i32::from_le_bytes(
                owner.body[288 + axis * 4..292 + axis * 4]
                    .try_into()
                    .unwrap(),
            )
        }),
        attributes: owner
            .attributes
            .context("original save SMOB support: attributes missing")?,
    }))
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
        let clock = parse_save_clock(section(*b"SVGV")?)?;
        crate::clock::SimulationClock::from_original(&clock, 0)?;
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
            support: decode_support(player, &objects)?,
            clock,
            animations: objects
                .iter()
                .filter(|object| object.object_id == -2 || object.object_id >= 0)
                .filter_map(|object| {
                    object.animation_header.as_ref().map(|header| {
                        let word = |offset| {
                            i16::from_le_bytes(object.body[offset..offset + 2].try_into().unwrap())
                        };
                        OriginalActorPlayback {
                            identity: (object.object_id != -2)
                                .then_some(object.runtime_identifier() as i16),
                            playback: header.into(),
                            default: cyrus_soup::AnimationGroup::from_index(word(0x257) as u16),
                            model_frame: word(0x24f),
                            last_model_frame: word(0x253),
                            model_handle: word(0x251),
                            last_model_handle: word(0x255),
                        }
                    })
                })
                .collect(),
            world: game.metadata.world_id,
            position_q8: player.position_q8(),
            heading,
            health,
            inventory: Inventory::from_original(hud, &inventory)?,
            flags: GlobalFlagsSnapshot::from_original(definition, &flags)?,
        })
    }

    /// Build the imported clock independently, excluding host time spent loading.
    pub fn restore_clock(&self, bios_tick: u32) -> Result<crate::clock::SimulationClock> {
        crate::clock::SimulationClock::from_original(&self.clock, bios_tick)
    }

    /// Apply to the matching initialized candidate. The returned message must be
    /// presented by the host; success does not mean continuations were restored.
    pub fn apply(
        self,
        world: &mut ScriptWorld,
        hud: &ScriptHud,
        animations: &cyrus_assets::animation::Animations,
    ) -> Result<&'static str> {
        ensure!(
            world.world == self.world,
            "original save world {}: candidate world {} differs",
            self.world,
            world.world
        );
        world.restore_original_player(self.position_q8, self.heading, self.health, self.flags)?;
        if let Some(support) = self.support {
            world.restore_original_support(support)?;
        }
        world.restore_original_animations(&self.animations, animations)?;
        hud.replace_original_inventory(self.inventory)?;
        Ok(LIMITATION)
    }
}
