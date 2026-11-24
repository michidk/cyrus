//! Original logical sessions are constructed from installed sources off-live.
use super::*;
use cyrus_game::{preferences::Preferences, random::EngineRandom};
use rgpre::import::savegame::{
    SaveGameplayGlobals, SaveSessionGlobals, SaveSlot, parse_save_clock, parse_save_flags,
    parse_save_inventory,
};

/// Complete source and retained-session ownership for original logical assembly.
/// Original saves intentionally omit some process-wide owners. Those owners come
/// from `prior`, never from a fresh-controller initializer or a native baseline.
struct OriginalSessionInputs<'a> {
    pub slot: &'a SaveSlot,
    pub sources: &'a BTreeMap<u32, WorldSnapshotSources<'a>>,
    pub allocations: &'a BTreeMap<u32, OriginalWorldAllocations<'a>>,
    pub hud: &'a cyrus_assets::hud::HudAsset,
    pub prior_hud: &'a super::super::ScriptHud,
    pub preferences: &'a Preferences,
    pub random: &'a EngineRandom,
    pub toggles: crate::player::input::PlayerToggles,
    pub bios_tick: u32,
}

impl ScriptActors {
    /// Prepare all saved logical worlds and services without changing this session.
    ///
    /// Every world starts from its installed allocation census. Actor state, poses,
    /// navigation, family/camera caches and native continuations then join one
    /// candidate. Globals, HUD and clock validate before the candidate is returned.
    /// A failure, including the last saved shell or a late retained-global binding,
    /// leaves all prior worlds and services untouched. No callbacks or updates run.
    /// The host must publish this with its prepared audio/render/player owners;
    /// this constructor does not enable player-facing original-save admission.
    fn prepare_original_session(
        &self,
        inputs: OriginalSessionInputs<'_>,
    ) -> Result<LogicalSessionCandidate> {
        let OriginalSessionInputs {
            slot,
            sources,
            allocations,
            hud,
            prior_hud,
            preferences,
            random,
            toggles,
            bios_tick,
        } = inputs;
        let section = |name: [u8; 4]| -> Result<&[u8]> {
            let mut found = slot
                .game
                .sections
                .iter()
                .filter(|section| section.name == name);
            let data = &found
                .next()
                .with_context(|| {
                    format!(
                        "original session {} section {} absent",
                        slot.game.metadata.world_id,
                        String::from_utf8_lossy(&name)
                    )
                })?
                .data;
            anyhow::ensure!(
                found.next().is_none(),
                "original session duplicate section {}",
                String::from_utf8_lossy(&name)
            );
            Ok(data)
        };
        let globals_bytes = section(*b"SVG2")?;
        let session = SaveSessionGlobals::parse(globals_bytes)?;
        let gameplay = SaveGameplayGlobals::parse(section(*b"SVGV")?)?;
        let mut saves = BTreeMap::from([(slot.game.metadata.world_id, &slot.game)]);
        for saved in slot.maps.values() {
            anyhow::ensure!(
                saves.insert(saved.metadata.world_id, saved).is_none(),
                "original session repeats saved world {}",
                saved.metadata.world_id
            );
        }
        anyhow::ensure!(
            saves.keys().eq(sources.keys()) && saves.keys().eq(allocations.keys()),
            "original session saved/source/allocation world censuses differ"
        );
        let flags = super::super::GlobalFlagsSnapshot::from_original(
            &sources[&slot.game.metadata.world_id].scripts.definition,
            &parse_save_flags(section(*b"SVGF")?)?,
        )?;
        let mut generation = self.generation_high_water();
        let mut worlds = BTreeMap::new();
        for (&world_id, saved) in &saves {
            generation = generation
                .checked_add(1)
                .context("original session actor generation exhausted")?;
            let source = &sources[&world_id];
            source.identity()?;
            let scene = source
                .player
                .context("original session complete player/collision source absent")?;
            let models = scene
                .actor_collision
                .as_ref()
                .context("original session installed model allocations absent")?;
            // Saved loading rebuilds the shell list from SMSH. Fresh-entry shells
            // may already have ended in a visited world and must not reappear.
            let mut world = ScriptWorld::prepare_with_shells(source.scripts, generation, &[])?;
            world.prepare_spatial()?;
            world.prepare_markers(source.animations)?;
            world.prepare_navigation(source.animations)?;
            world.prepare_ropes(source.animations)?;
            if let Some(effects) = source.effects {
                world.prepare_environment_effects(effects)?;
            }
            world.prepare_model_resources(source.animations, models)?;
            for index in 0..world.actors.len() {
                if matches!(
                    world.actors[index].source,
                    ScriptActorSource::Player | ScriptActorSource::AlternatePlayer
                ) {
                    world.prepare_player_model_resources(world.id(index)?, source.animations)?;
                }
            }
            world.restore_flags(flags.clone())?;
            let mut world = world
                .with_original_world(saved, source, allocations[&world_id], globals_bytes)
                .with_context(|| format!("original session world {world_id}"))?;
            world.initialize_collision_shapes(source.animations, scene)?;
            for (index, actor) in world.actors.iter().enumerate() {
                anyhow::ensure!(
                    actor.update_failure.is_none(),
                    "original session world {world_id} actor {index} {}: {}",
                    actor.name(),
                    actor.update_failure.as_deref().unwrap_or_default()
                );
            }
            world.prepare_model_admission(
                source
                    .collision
                    .model_visibility
                    .as_ref()
                    .context("original session installed visibility absent")?,
                source.collision.placements.len(),
            )?;
            worlds.insert(world_id, world);
        }
        let actor_bindings =
            self.current
                .iter()
                .chain(self.visited.values())
                .flat_map(|old| {
                    worlds.get(&old.world).into_iter().flat_map(move |new| {
                        old.actors
                            .iter()
                            .enumerate()
                            .filter_map(move |(index, actor)| {
                                let replacement = new.actors.iter().enumerate().find(
                                    |(destination, candidate)| {
                                        (!matches!(actor.source, ScriptActorSource::Map(_))
                                            || *destination == index)
                                            && std::mem::discriminant(&candidate.source)
                                                == std::mem::discriminant(&actor.source)
                                            && candidate.name() == actor.name()
                                            && candidate.runtime_identifier
                                                == actor.runtime_identifier
                                    },
                                );
                                replacement.map(|(destination, _)| {
                                    (
                                        ScriptActorId {
                                            generation: old.generation,
                                            index,
                                        },
                                        ScriptActorId {
                                            generation: new.generation,
                                            index: destination,
                                        },
                                    )
                                })
                            })
                    })
                })
                .collect::<Vec<_>>();
        let prior = self
            .current
            .as_ref()
            .map_or(&self.globals, |world| &world.globals);
        let mut current = worlds
            .remove(&slot.game.metadata.world_id)
            .context("original current world absent")?;
        current.globals = super::super::GameplayGlobals::from_original_load(
            prior,
            &gameplay,
            &session,
            &sources[&current.world].scripts.dialogue,
            &actor_bindings,
            allocations[&current.world].ambient_channels,
        )?;
        current.globals.group_sync.register_actors(&current.actors);
        current.restore_session_rope_jump_latch(Some(gameplay.rope_jump_latch != 0))?;
        let inventory = parse_save_inventory(
            section(*b"SVIT")?,
            hud.inventory
                .len()
                .checked_sub(1)
                .context("original session installed inventory empty")?,
        )?;
        let text = slot
            .logbook
            .as_deref()
            .map(std::str::from_utf8)
            .transpose()
            .context("original LOGBOOK.TXT is not supported UTF-8/ASCII text")?;
        let hud = super::super::ScriptHud::from_original_load(
            prior_hud,
            hud,
            preferences,
            &inventory,
            &session,
            text,
        )?;
        let mut clock = crate::clock::SimulationClock::from_original(
            &parse_save_clock(section(*b"SVGV")?)?,
            bios_tick,
        )?;
        clock.restart_warmup();
        let actors = ScriptActors {
            restored_generation: Some(current.generation),
            current: Some(current),
            visited: worlds,
            generation,
            requested: None,
            retained_flags: Some(flags),
            retained_health: Some(
                session
                    .player_health
                    .try_into()
                    .context("original session health exceeds byte range")?,
            ),
            discard_current_on_release: false,
            world_loop_outcome: None,
            globals: Default::default(),
            error: None,
        };
        Ok(LogicalSessionCandidate {
            actors,
            hud,
            clock,
            random: EngineRandom::from_state(random.state()),
            toggles,
        })
    }
}

/// Original document and independently prepared sources for atomic logical admission.
/// Audio and rendered host owners must join the enclosing session publication.
pub struct OriginalSessionImport<'a> {
    pub slot: &'a SaveSlot,
    pub sources: &'a BTreeMap<u32, WorldSnapshotSources<'a>>,
    pub allocations: &'a BTreeMap<u32, OriginalWorldAllocations<'a>>,
    pub hud: &'a cyrus_assets::hud::HudAsset,
    pub preferences: &'a Preferences,
}

impl LogicalSessionCandidate {
    /// Admit every supported original logical owner in one transaction.
    /// All validation and construction use separate allocations. Rejection preserves
    /// every existing interpreter, native task, world, global, HUD, clock and RNG.
    /// The first explicit update belongs to the caller and is never run by import.
    /// This remains an off-live step of the complete runtime session publication.
    pub fn import_original(&mut self, inputs: OriginalSessionImport<'_>) -> Result<()> {
        let replacement = self
            .actors
            .prepare_original_session(OriginalSessionInputs {
                slot: inputs.slot,
                sources: inputs.sources,
                allocations: inputs.allocations,
                hud: inputs.hud,
                preferences: inputs.preferences,
                prior_hud: &self.hud,
                random: &self.random,
                toggles: self.toggles,
                bios_tick: self.clock.bios_sample(),
            })?;
        *self = replacement;
        Ok(())
    }
}
