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
pub const LIMITATION: &str = "Limited original-save import: world, player position, heading, health, inventory, global flags and clock continuation only. Animation playback with a bound ShowFrame pose and its saved panic-group attribute are restored. Script/task continuations, other actor state except the static supporting actor, unresolved animation/model or sound-handle bindings, controller state, item-use timers and journal are unsupported; the host supplies the native baseline for unsupported owners.";

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
    pub panic_group: Option<u8>,
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
                        let pose = object.pose();
                        OriginalActorPlayback {
                            identity: (object.object_id != -2)
                                .then_some(object.runtime_identifier() as i16),
                            playback: header.into(),
                            panic_group: object.attributes.as_ref().map(|attributes| {
                                attributes[cyrus_soup::Attribute::PanicGroup.index()]
                            }),
                            default: cyrus_soup::AnimationGroup::from_index(
                                pose.default_group as u16,
                            ),
                            model_frame: pose.current.frame,
                            last_model_frame: pose.previous.frame,
                            model_handle: pose.current.model_handle,
                            last_model_handle: pose.previous.model_handle,
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

/// Initialized actor ownership required for a semantic interpreter import.
/// The host establishes these identities from the matching installed world.
/// Task bindings must already identify the saved callback's native owner;
/// decoding never allocates a service or reissues a native call.
pub struct OriginalActorBinding {
    pub object_id: i32,
    pub runtime_identifier: u16,
    pub origin: cyrus_soup::CodeOrigin,
    pub tasks: [Option<cyrus_soup::persistence::ContinuationTask>; 6],
}

/// Complete actor, code-owner or shell interpreter candidate and all six native
/// invocation-word sets.
/// This is not world/session admission. Publish only with matching native task
/// variants, dialogue/audio/animation owners and the rest of the imported world.
pub struct OriginalOwnerContinuation {
    pub vm: cyrus_soup::persistence::Snapshot,
    pub invocation_words: [[u32; 4]; 6],
}

impl OriginalOwnerContinuation {
    /// Decode one actor through published semantic save APIs, validating owner,
    /// source, cursor and every callback before returning any replacement state.
    /// No baseline locals, task bytes or invocation words are substituted.
    pub fn decode(
        object: &rgpre::import::savegame::SaveObject,
        locals: &rgpre::import::savegame::SaveScriptLocals,
        binding: OriginalActorBinding,
        sources: &std::collections::BTreeMap<
            cyrus_soup::CodeOrigin,
            std::sync::Arc<cyrus_soup::program::Program>,
        >,
    ) -> Result<Self> {
        use cyrus_soup::ScriptValue;
        let context = format!(
            "original actor {} runtime {} continuation",
            binding.object_id, binding.runtime_identifier
        );
        let decode = || -> Result<Self> {
            ensure!(
                object.object_id == binding.object_id
                    && object.runtime_identifier() == binding.runtime_identifier,
                "saved actor owner differs from initialized binding"
            );
            ensure!(
                object.private_variable_words().is_empty(),
                "object-private variable owner is unsupported by this interpreter binding"
            );
            let program = sources
                .get(&binding.origin)
                .context("installed owning program missing")?;
            let values = locals.bind_actor(object, program.variables.len())?;
            let variables = values.map_or_else(Vec::new, |bound| {
                bound
                    .words
                    .iter()
                    .copied()
                    .map(ScriptValue::Integer)
                    .collect()
            });
            Self::decode_state(
                object.script_continuation(),
                object.task_slots(),
                variables,
                binding.origin,
                binding.tasks,
                sources,
            )
        };
        decode().with_context(|| context)
    }
}

/// Installed map-code identity and native task source bindings.
pub struct OriginalCodeBinding {
    pub owner_id: u32,
    pub origin: cyrus_soup::CodeOrigin,
    pub tasks: [Option<cyrus_soup::persistence::ContinuationTask>; 6],
}

/// Installed eight-byte shell identity and native task source bindings.
pub struct OriginalShellBinding {
    pub name: [u8; 8],
    pub origin: cyrus_soup::CodeOrigin,
    pub tasks: [Option<cyrus_soup::persistence::ContinuationTask>; 6],
}

impl OriginalOwnerContinuation {
    /// Stage a code owner's complete VM, binding SVRG words to its installed
    /// declaration. No task handler or initialized baseline state is used.
    pub fn decode_code(
        owner: &rgpre::import::savegame::SaveCodeOwner,
        locals: &rgpre::import::savegame::SaveScriptLocals,
        binding: OriginalCodeBinding,
        sources: &std::collections::BTreeMap<
            cyrus_soup::CodeOrigin,
            std::sync::Arc<cyrus_soup::program::Program>,
        >,
    ) -> Result<Self> {
        let decode = || -> Result<Self> {
            ensure!(
                owner.owner_id == binding.owner_id,
                "saved code owner differs from initialized binding"
            );
            let program = sources
                .get(&binding.origin)
                .context("installed owning program missing")?;
            let variables = locals
                .bind_code_owner(owner, program.variables.len())?
                .map_or_else(Vec::new, |bound| {
                    bound
                        .words
                        .iter()
                        .copied()
                        .map(cyrus_soup::ScriptValue::Integer)
                        .collect()
                });
            Self::decode_state(
                owner.script_continuation(),
                owner.task_slots(),
                variables,
                binding.origin,
                binding.tasks,
                sources,
            )
        };
        decode().with_context(|| format!("original code owner {} continuation", binding.owner_id))
    }

    /// Stage a shell using its named installed definition and private allocation.
    /// The published execution view supplies the cleared temporary-actor Anchor
    /// and pause values; these fields are not borrowed from a native baseline.
    pub fn decode_shell(
        owner: &rgpre::import::savegame::SaveShell,
        binding: OriginalShellBinding,
        sources: &std::collections::BTreeMap<
            cyrus_soup::CodeOrigin,
            std::sync::Arc<cyrus_soup::program::Program>,
        >,
    ) -> Result<Self> {
        let decode = || -> Result<Self> {
            ensure!(
                owner.name() == binding.name,
                "saved shell differs from initialized binding"
            );
            let program = sources
                .get(&binding.origin)
                .context("installed owning program missing")?;
            let variables = owner
                .bind_locals(program.variables.len())?
                .words
                .into_iter()
                .map(cyrus_soup::ScriptValue::Integer)
                .collect();
            Self::decode_state(
                owner.script_continuation(),
                owner.task_slots(),
                variables,
                binding.origin,
                binding.tasks,
                sources,
            )
        };
        decode().with_context(|| {
            format!(
                "original shell {:?} continuation",
                String::from_utf8_lossy(&binding.name)
            )
        })
    }

    fn decode_state(
        saved: rgpre::import::savegame::SaveScriptContinuation,
        slots: [rgpre::import::savegame::SaveTaskSlot; 6],
        variables: Vec<cyrus_soup::ScriptValue>,
        origin: cyrus_soup::CodeOrigin,
        tasks: [Option<cyrus_soup::persistence::ContinuationTask>; 6],
        sources: &std::collections::BTreeMap<
            cyrus_soup::CodeOrigin,
            std::sync::Arc<cyrus_soup::program::Program>,
        >,
    ) -> Result<Self> {
        use cyrus_soup::persistence::{ContinuationState, Snapshot};
        let program = sources
            .get(&origin)
            .context("installed owning program missing")?;
        ensure!(
            saved.data == origin.0,
            "saved code origin differs from installed binding"
        );
        let relative = |cursor: u32| {
            cursor
                .checked_sub(saved.data)
                .context("script cursor precedes owning code origin")
        };
        let main = relative(saved.cursor)?;
        let entry = if program.entry == 0 {
            ensure!(saved.entry == 0, "unexpected entry continuation");
            None
        } else {
            ensure!(saved.entry != 0, "missing entry continuation");
            Some(relative(saved.entry)?)
        };
        let return_to = (saved.return_to != 0)
            .then(|| relative(saved.return_to))
            .transpose()?;
        for (index, (slot, task)) in slots.iter().zip(&tasks).enumerate() {
            match task {
                None => ensure!(
                    slot.callback == 0,
                    "slot {index}: saved callback has no native binding"
                ),
                Some(task) => {
                    let operand = task.source.operand()?;
                    ensure!(
                        slot.callback != 0 && slot.callback == operand,
                        "slot {index}: saved callback differs from native source binding"
                    );
                }
            }
        }
        let vm = Snapshot::from_continuation(
            origin,
            ContinuationState {
                variables,
                cursors: [Some(main), entry],
                return_to,
                result: saved.result,
                saved_result: saved.saved_result,
                anchor: saved.anchor,
                tasks,
                status: slots.map(|slot| slot.status),
                pause: slots.map(|slot| slot.pause),
                timers: slots.map(|slot| slot.timer),
            },
            sources,
        )?;
        Ok(Self {
            vm,
            invocation_words: slots.map(|slot| slot.variables),
        })
    }
}

/// A saved code owner's exact type-two MPOB allocation in installed order.
/// Coloured lights use separate records but do not consume this identity counter
/// and are excluded from SMCO. Editor object IDs and save traversal order are
/// independent of this allocation identity (preservation's save owner contract).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OriginalCodeAllocation {
    pub owner_id: u32,
    pub placement_index: usize,
    pub origin: cyrus_soup::CodeOrigin,
}

impl OriginalCodeAllocation {
    /// Bind the complete saved population before changing any native state.
    /// Reject missing, duplicate or foreign owners and mismatched installed code.
    pub fn bind(
        scripts: &cyrus_assets::script::Scripts,
        owners: &[rgpre::import::savegame::SaveCodeOwner],
    ) -> Result<Vec<Self>> {
        use cyrus_assets::script::PlacementType;
        let allocations = scripts.instances.iter().enumerate()
            .filter(|(_, placement)| placement.object_type == PlacementType::SCRIPT_ONLY)
            .enumerate()
            .map(|(owner, (placement_index, placement))| {
                let definition = scripts.actors.get(&placement.actor)
                    .with_context(|| format!("original code allocation {owner} placement {placement_index} script {} absent", placement.actor))?;
                Ok(Self { owner_id: u32::try_from(owner)?, placement_index,
                    origin: cyrus_soup::CodeOrigin(definition.code_origin) })
            }).collect::<Result<Vec<_>>>()?;
        ensure!(
            allocations.len() == owners.len(),
            "original code allocation count {} differs from saved owners {}",
            allocations.len(),
            owners.len()
        );
        let mut seen = std::collections::BTreeSet::new();
        for owner in owners {
            let allocation = allocations.get(owner.owner_id as usize).with_context(|| {
                format!(
                    "original code owner {} has no installed allocation",
                    owner.owner_id
                )
            })?;
            ensure!(
                seen.insert(owner.owner_id),
                "original code owner {} duplicated",
                owner.owner_id
            );
            ensure!(
                allocation.origin.0 == owner.script_continuation().data,
                "original code owner {} placement {} source differs from installed allocation",
                owner.owner_id,
                allocation.placement_index
            );
        }
        Ok(allocations)
    }
}
