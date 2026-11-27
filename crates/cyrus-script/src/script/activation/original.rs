//! Original save fields and load resets for the gameplay-global owner.
use super::GameplayGlobals;
use anyhow::{Context, Result, ensure};
use cyrus_assets::script::DialogueSettings;
use cyrus_soup::ActorReference as ScriptActorId;
use rgpre::import::savegame::{SaveGameplayGlobals, SaveSessionGlobals};
use std::collections::BTreeMap;

impl GameplayGlobals {
    /// Refresh retained instance references against replacement process pools.
    /// The temporary record survives Load; its offsets do not snapshot the pools.
    /// Validate every range before changing the unpublished global owner.
    pub fn resolve_original_source_pools(
        &mut self,
        locals: &rgpre::import::savegame::SaveScriptLocals,
        scripts: &cyrus_assets::script::Scripts,
    ) -> Result<()> {
        let Some(source) = self.separate_native_source.as_ref() else {
            return Ok(());
        };
        let context = format!(
            "world {} retained separate source {} from world {}",
            scripts.world, source.name, source.world
        );
        let mut replacement = source.clone();
        if !source.locals.is_empty() {
            let offset = source
                .pool_offsets
                .and_then(|p| p.locals)
                .with_context(|| format!("{context}: variable pool offset unavailable"))?;
            replacement.locals = locals
                .bind_offset(offset, source.locals.len())
                .with_context(|| format!("{context}: current variable pool"))?
                .iter()
                .copied()
                .map(cyrus_soup::ScriptValue::Integer)
                .collect();
        }
        if !source.strings.is_empty() {
            let base = source
                .pool_offsets
                .and_then(|p| p.strings)
                .with_context(|| format!("{context}: string pool offset unavailable"))?;
            let mut strings = BTreeMap::new();
            for actor in scripts.actors.values() {
                let Some(start) = actor.pool_offsets.and_then(|p| p.strings) else {
                    continue;
                };
                for (index, value) in actor.program.strings.iter().enumerate() {
                    let offset = u32::try_from(index)
                        .ok()
                        .and_then(|i| i.checked_mul(4))
                        .and_then(|i| start.checked_add(i))
                        .with_context(|| format!("{context}: string pool range overflow"))?;
                    if let Some(previous) = strings.insert(offset, value) {
                        ensure!(
                            previous == value,
                            "{context}: conflicting current string pool entry {offset}"
                        );
                    }
                }
            }
            replacement.strings = (0..source.strings.len())
                .map(|index| {
                    let offset = u32::try_from(index)
                        .ok()
                        .and_then(|i| i.checked_mul(4))
                        .and_then(|i| base.checked_add(i))
                        .with_context(|| {
                            format!("{context}: retained string pool range overflow")
                        })?;
                    strings.get(&offset).map(|s| (*s).clone()).with_context(|| {
                        format!("{context}: current string pool entry {offset} unavailable")
                    })
                })
                .collect::<Result<_>>()?;
        }
        self.separate_native_source = Some(replacement);
        Ok(())
    }
    /// Prepare an unpublished original-load global owner from an established session.
    ///
    /// Saved words and verified load resets replace their prior values. Unsaved
    /// owners retain their prior values, with actor references rebound explicitly.
    /// States whose original load ownership is not established are rejected when
    /// populated. The enclosing replacement still owns execution and host admission.
    pub fn from_original_load(
        prior: &Self,
        saved: &SaveGameplayGlobals,
        session: &SaveSessionGlobals,
        settings: &DialogueSettings,
        actor_bindings: &[(ScriptActorId, ScriptActorId)],
        samples: &BTreeMap<u8, u8>,
    ) -> Result<Self> {
        ensure!(
            prior.actor_result.is_none(),
            "original load: pending actor expression result unsupported"
        );
        ensure!(
            prior.player_contacts.actors.is_empty() && prior.player_contacts.statics.is_empty(),
            "original load: retained player contact ownership unsupported"
        );
        ensure!(
            prior.turn.is_idle(),
            "original load: accumulated turn input ownership unsupported"
        );
        ensure!(
            prior.host_requests == super::HostRequests::default(),
            "original load: pending main-loop request ownership unsupported"
        );
        ensure!(
            !prior.camera_reset_request && !prior.island_map_request,
            "original load: pending camera or island-map request ownership unsupported"
        );
        ensure!(
            prior.printed.is_empty(),
            "original load: retained console line ownership unsupported"
        );
        ensure!(
            prior.legacy_ledge_anchor.is_none(),
            "original load: legacy ledge anchor unsupported"
        );
        ensure!(
            prior.native_command.continuation == 0,
            "original load: retained shared native continuation resource ownership unsupported"
        );
        ensure!(
            prior.walk_route.target_exclusion == prior.sound_maintenance.inhibited(),
            "original load: route exclusion and sound inhibition aliases disagree"
        );
        ensure!(
            prior
                .highlighted_choice
                .as_ref()
                .is_none_or(String::is_empty),
            "original load: highlighted menu choice load ownership unsupported"
        );
        // Ordinary actor sources require a replacement identity. The separate
        // temporary is a process-wide allocation: Load retains its copied contents
        // even when its semantic record is absent (preservation SOUP, separate map
        // object script state). Snapshot validation requires those contents below.
        if let super::NativeSource::Actor { generation, index } = prior.native_source {
            ensure!(
                actor_bindings
                    .iter()
                    .any(|(old, _)| old.generation == generation && old.index == index),
                "original load: retained native source actor {generation}/{index} replacement absent"
            );
        }
        let mut snapshot = prior.snapshot();
        // The original load clears dialogue before restoring actor continuations.
        snapshot.dialogue = None;
        // World entry rebuilds rendezvous membership from the restored actors.
        snapshot.group_sync = Default::default();
        let mut replacement = Self::from_snapshot(snapshot, settings, actor_bindings, samples)?;
        replacement.item_use_latch = prior.item_use_latch;
        replacement.item_viewer = prior.item_viewer;
        replacement.restore_original_saved_fields(saved, session, settings)?;
        Ok(replacement)
    }

    /// Restore the supported saved fields and load resets on an unpublished owner.
    ///
    /// The caller must independently restore interpreter, route, message, menu,
    /// contact and input ownership before admitting the replacement session.
    /// This operation alone does not establish a complete original session.
    /// Health maximum, conditional/secondary countdowns, ledge result, menu mode,
    /// potion counters and the queued movie retain their established session values.
    /// Their original globals have no SVGV or SVG2 record.
    pub fn restore_original_saved_fields(
        &mut self,
        saved: &SaveGameplayGlobals,
        session: &SaveSessionGlobals,
        dialogue_settings: &DialogueSettings,
    ) -> Result<()> {
        let controller = self
            .controller_prelude
            .as_ref()
            .context("original load: prior selected-player controller unavailable")?;
        let secondary = u8::try_from(controller.secondary_ticks).context(
            "original load: retained secondary countdown exceeds native ledge owner range",
        )?;
        ensure!(
            self.selected_action_inhibition == Some(secondary),
            "original load: prior ledge inhibition and secondary countdown disagree"
        );
        self.selected_health
            .as_ref()
            .context("original load: prior selected health unavailable")?
            .validate()?;
        self.sound_maintenance.reset()?;
        let dialogue = super::super::DialogueState::from_original_load(dialogue_settings)?;
        self.selected_health
            .as_mut()
            .expect("validated prior selected health")
            .restore_original_load(saved, session)?;
        let controller = self
            .controller_prelude
            .as_mut()
            .expect("validated prior controller");
        controller.override_group = 0;
        controller.pending_action = false;
        controller.action_ticks = saved.controller.action_ticks;
        self.activation_consumed_use = saved.activation_consumed_use != 0;
        self.primary_body_contacts = saved.body_response_contacts;
        self.lineup_result = Some(saved.lineup_result);
        self.selected_combat_inhibition = Some(saved.lineup_result);
        self.lockout = Some(saved.lockout);
        self.player_idle_counter = Some(saved.controller.idle_counter);
        self.world_loop_frame = 0;
        self.dragon_breath_cooldown = 0;
        self.player_swapped = session.selected_player_switch != 0;
        self.dialogue = Some(dialogue);
        self.sound_maintenance.restore_original_load()?;
        Ok(())
    }
}
