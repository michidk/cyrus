//! Shared activation ownership and eligibility follow preservation's HUD prompt rules.
use super::ScriptActorId;
use anyhow::{Context, Result};
use cyrus_script::Argument;
use serde::Serialize;
use std::collections::BTreeMap;

/// Existing animation state, which may not have a current group yet.
#[derive(Clone, Copy, Debug)]
pub struct ActivationAnimation {
    pub group: Option<u16>,
}

/// Caller-established actor state; absence of a record means unknown, not eligible.
#[derive(Clone, Copy, Debug)]
pub struct ActivationActor {
    pub state: u8,
    pub on_rope: bool,
    pub water_state: u8,
    pub torch_burning: u8,
    pub weapon_assigned: bool,
    pub weapon_drawn: bool,
    /// None means no animation state; an existing state may have no current group.
    pub animation: Option<ActivationAnimation>,
}
impl ActivationActor {
    /// Item-zero actor gates from preservation's HUD contract; unknown actors must not call this.
    pub fn compass_eligible(
        self,
        holding_attribute: u8,
        item_system_ready: bool,
        shell_execution: bool,
    ) -> bool {
        let actor = self;
        let group = actor.animation.and_then(|animation| animation.group);
        let animated = item_system_ready && !shell_execution && actor.animation.is_some();
        let climbing =
            animated && (matches!(actor.state, 33 | 34) || matches!(group, Some(149 | 150)));
        item_system_ready
            && !shell_execution
            && !matches!(actor.water_state, 2 | 4)
            && !weapon_predicates(
                actor.state,
                actor.animation,
                actor.weapon_assigned,
                actor.weapon_drawn,
                holding_attribute,
                item_system_ready,
                shell_execution,
            )
            .0
            && !climbing
            && actor.torch_burning == 0
    }

    fn on_edge(self) -> bool {
        (10..=19).contains(&self.state)
    }
}

/// Shared weapon gates for activation and ordinary default selection.
pub(super) fn weapon_predicates(
    state: u8,
    animation: Option<ActivationAnimation>,
    weapon_assigned: bool,
    weapon_drawn: bool,
    holding_attribute: u8,
    item_system_ready: bool,
    shell_execution: bool,
) -> (bool, bool, bool) {
    let ready = item_system_ready && !shell_execution && animation.is_some();
    let group = animation.and_then(|animation| animation.group);
    let drawing = ready && (state == 20 || group == Some(91));
    let sheathing = ready && (state == 21 || group == Some(129));
    let holding =
        holding_attribute != 0 || (ready && weapon_assigned && weapon_drawn && !sheathing);
    (holding, drawing, sheathing)
}

/// Explicit inspection inputs. Actor indices belong to the selected player's generation.
/// RTX labels include empty and text-only entries; keys retain their installed spelling.
#[derive(bevy::prelude::Resource, Debug)]
pub struct ActivationInputs {
    pub player: ScriptActorId,
    pub actors: BTreeMap<usize, ActivationActor>,
    pub labels: BTreeMap<String, String>,
    pub lockout: u16,
    pub dialogue_open: bool,
    pub lineup_result: i32,
    pub use_held: bool,
    pub item_system_ready: bool,
    pub shell_execution: bool,
    pub render_disabled: bool,
}
impl ActivationInputs {
    /// Known actor gates for owned compass use, from preservation's HUD item-use rules.
    /// Lockout/ownership/selection and interaction routing belong to the caller.
    pub fn compass_eligible(&self, player: &super::ScriptActor) -> Result<bool> {
        let actor = self.player_state(player)?;
        Ok(actor.compass_eligible(
            player.attributes()[110],
            self.item_system_ready,
            self.shell_execution,
        ))
    }

    fn player_state(&self, player: &super::ScriptActor) -> Result<ActivationActor> {
        let mut state = self.actor(self.player.index)?;
        if let Some(group) = player.bound_animation_group() {
            state.animation = Some(ActivationAnimation { group });
        }
        Ok(state)
    }
    fn actor(&self, index: usize) -> Result<ActivationActor> {
        self.actors.get(&index).copied().with_context(|| {
            format!("activation actor {index}: caller-established state unavailable")
        })
    }
}

/// Item-area decision for one supplied overlay update; drawing consumes a prompt.
#[derive(Debug, PartialEq, Eq)]
pub enum ActivationOverlay {
    Hidden,
    Prompt(String),
    Item { visible: bool, cycle_allowed: bool },
}

/// Retained preliminary controller state, reset by standalone player construction.
#[derive(Clone, Debug, Serialize, serde::Deserialize)]
struct ControllerPrelude {
    override_group: i32,
    pending_action: bool,
    conditional_ticks: u32,
    action_ticks: u32,
    secondary_ticks: u32,
}

/// Retained native source is unknown until an assigning interpreter entry.
/// World replacement does not establish a new source.
#[derive(Default, Clone, Copy, Debug, Serialize, serde::Deserialize)]
pub(super) enum NativeSource {
    #[default]
    Unknown,
    Actor {
        generation: u64,
        index: usize,
    },
}

/// Gameplay globals retain engine initialization and native-call assignments.
/// Unknown startup values remain None.
/// The host retains these globals while replacing the logical world registry.
#[derive(Default, Debug, Serialize)]
pub struct GameplayGlobals {
    /// Current native command context, shared by successive actor invocations.
    pub(super) native_command: super::NativeCommandState,
    pub(super) native_source: NativeSource,
    pub(super) native_command_return: u8,
    pub(super) walk_route: super::WalkRouteState,
    /// Shared direction-called flags, reset at ordinary wrapper entry.
    pub(super) locomotion_called: u8,
    pub(super) ground_prevalidation_used: bool,
    pub(super) ground_prevalidation: Option<super::OrdinaryGroundPrevalidation>,
    /// Engine message queues persist independently of a logical world registry.
    pub(super) combat_messages: super::CombatMessages,
    pub(super) dialogue: Option<super::DialogueState>,
    lockout: Option<u16>,
    pub(super) lineup_result: Option<i32>,
    player_idle_counter: Option<i32>,
    controller_prelude: Option<ControllerPrelude>,
    pub(super) selected_combat_inhibition: Option<i32>,
    pub(super) selected_health: Option<super::SelectedHealthState>,
    pub(super) selected_action_inhibition: Option<u8>,
    pub(super) selected_ledge_result: Option<i32>,
    /// World-loop iterations, reset on entry and incremented before ordinary traversal.
    pub(super) world_loop_frame: u32,
    pub(super) sound_maintenance: super::SoundMaintenance,
    menu_state: u16,
    dialogue_state: Option<u16>,
    highlighted_choice: Option<String>,
    candle_show_requested: bool,
    /// Script potion counters (`SetPotion`, `GetPotion`); shipped scripts use indices 0 to 3.
    /// The engine table starts zero-filled.
    pub(super) potions: [i32; 4],
    /// Swap state of the two player records: clear selects CYRUS, set selects GREMLIN. Retained across
    /// world replacement and saves; nothing sets it yet (`PushPlayer`/`PopPlayer` need the host's player
    /// controller, camera and entity binding to follow the selection first).
    pub(super) player_swapped: bool,
    /// Group synchronization channels; cleared on world entry.
    pub(super) group_sync: super::GroupSync,
    /// Requests for the host's main loop; transient, so saves neither store nor restore them.
    host_requests: HostRequests,
}

/// Session-level requests raised by scripts. The original stores each in an engine global that
/// its main loop consumes; the host takes them with [`GameplayGlobals::take_host_requests`].
#[derive(Default, Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HostRequests {
    /// `LoadWorld` arguments, unchanged: the three integers the original retains.
    pub load_world: Option<[i32; 3]>,
    /// `WonGame` was called.
    pub won_game: bool,
    /// `PushPlayer` was called.
    pub push_player: bool,
    /// `PopPlayer` was called.
    pub pop_player: bool,
}
impl GameplayGlobals {
    /// Whether the swap state selects GREMLIN instead of CYRUS as the selected player.
    pub fn player_swapped(&self) -> bool {
        self.player_swapped
    }

    /// Takes and clears the pending script requests.
    pub fn take_host_requests(&mut self) -> HostRequests {
        std::mem::take(&mut self.host_requests)
    }

    pub(super) fn request_mut(&mut self) -> &mut HostRequests {
        &mut self.host_requests
    }

    /// Fresh executable-session values from preservation's player initialization
    /// rules. World replacement and save restoration must retain their own values.
    pub(super) fn new_session() -> Self {
        Self {
            lockout: Some(0),
            dialogue_state: Some(0),
            player_idle_counter: Some(0),
            ..Self::default()
        }
    }

    /// Retained animation group from preservation's selected-player death override.
    /// Zero means no override; None means controller initialization is unknown.
    pub fn player_death_override_group(&self) -> Option<i32> {
        self.controller_prelude
            .as_ref()
            .map(|state| state.override_group)
    }

    pub(super) fn set_player_death_override_group(&mut self, group: i32) -> Result<()> {
        self.controller_prelude
            .as_mut()
            .context("selected death override owner unavailable")?
            .override_group = group;
        Ok(())
    }

    /// Shared selected-health hit inhibition, independent of the animation override group.
    /// None means the selected-health owner has not been initialized.
    pub fn player_hit_inhibited(&self) -> Option<bool> {
        self.selected_health
            .as_ref()
            .map(|state| state.hit_inhibited())
    }

    /// Run only at the explicit standalone-player initializer, never merely on a
    /// world swap or save restore. The sound-maintenance owner resets separately.
    pub(super) fn initialize_player_controller(&mut self) {
        self.walk_route.target_exclusion = false;
        self.lineup_result = Some(0);
        self.selected_combat_inhibition = Some(0);
        if let Some(health) = self.selected_health.as_mut() {
            health.set_hit_inhibited(false);
            health.initialize_death();
        }
        self.selected_action_inhibition = Some(0);
        self.sound_maintenance.initialize_player();
        self.controller_prelude = Some(ControllerPrelude {
            override_group: 0,
            pending_action: false,
            conditional_ticks: 0,
            action_ticks: 0,
            secondary_ticks: 0,
        });
    }

    /// Advance preliminary timers before idle reset or lockout admission.
    /// Units and ordering follow preservation's player controller contract.
    /// Unsupported pending/override execution stops at its actual stage; earlier
    /// timer changes remain retained. This does not perform ordinary dispatch.
    pub fn prepare_player_controller(
        &mut self,
        nominal_ticks: u32,
        queued_motion: u8,
        falling: u8,
        sliding: u8,
    ) -> Result<()> {
        let state = self
            .controller_prelude
            .as_mut()
            .context("player controller: explicit initialization unavailable")?;
        if queued_motion == 0 && falling == 0 {
            state.conditional_ticks = state.conditional_ticks.saturating_sub(nominal_ticks);
        }
        let waiting = if self.menu_state != 0 {
            false
        } else {
            self.dialogue
                .as_ref()
                .context("player controller: dialogue records unavailable")?
                .waiting()
        };
        let lockout = if self.menu_state != 0 || waiting {
            0
        } else {
            self.lockout
                .context("player controller: lockout unavailable")?
        };
        self.sound_maintenance
            .ordinary_actor(self.menu_state, waiting, lockout);
        anyhow::ensure!(
            !state.pending_action || sliding != 0,
            "player controller: pending action 74 execution unavailable"
        );
        state.action_ticks = state.action_ticks.saturating_sub(nominal_ticks);
        state.secondary_ticks = state.secondary_ticks.saturating_sub(nominal_ticks);
        anyhow::ensure!(
            !self
                .player_hit_inhibited()
                .context("player controller: hit inhibition unavailable")?,
            "player controller: death execution unavailable"
        );
        Ok(())
    }

    /// Last established signed idle counter; startup ownership may still be unknown.
    pub fn player_idle_counter(&self) -> Option<i32> {
        self.player_idle_counter
    }

    /// Apply completed BIOS measurement intervals in the admitted normal clock mode.
    /// Unknown initialization stays unknown until the controller's reset establishes it.
    pub fn advance_player_idle_clock(&mut self, intervals: u32) {
        if let Some(counter) = &mut self.player_idle_counter {
            *counter = counter.wrapping_add(intervals as i32);
        }
    }

    /// Select the idle request before dialogue/menu admission, following player.md.
    /// The caller has excluded the controller override and owns the normal animation
    /// request. Reset precedes lockout; a selected request stores -1 before execution.
    pub fn prepare_player_idle(
        &mut self,
        on_rope: bool,
        current_group: Option<u16>,
        health: u8,
        random: &mut crate::random::EngineRandom,
    ) -> Result<Option<u16>> {
        if on_rope || current_group != Some(0) {
            self.player_idle_counter = Some(0);
        }
        if self
            .lockout
            .context("player idle: lockout state unavailable")?
            != 0
            || on_rope
        {
            return Ok(None);
        }
        let counter = self
            .player_idle_counter
            .context("player idle: counter initialization unavailable")?;
        let group = if health < 40 && counter > 4 {
            4
        } else if health >= 40 && counter > 12 {
            loop {
                let draw = random.draw() & 3;
                if draw != 3 {
                    break draw as u16 + 1;
                }
            }
        } else {
            return Ok(None);
        };
        self.player_idle_counter = Some(-1);
        Ok(Some(group))
    }

    /// Consume an explicit selected-player weapon transition's 25-tick candle reset.
    /// This is independent of whether the projected drawn flag changed.
    pub fn take_candle_show_request(&mut self) -> bool {
        std::mem::take(&mut self.candle_show_requested)
    }

    pub(super) fn request_candle_show(&mut self) {
        self.candle_show_requested = true;
    }

    /// Controller dialogue-mode word, independent of active speech records.
    /// Fresh executable sessions establish zero; an uninitialized host stays unknown.
    /// See preservation engine/sound.md. World replacement retains this owner.
    pub fn dialogue_state(&self) -> Option<u16> {
        self.dialogue_state
    }

    /// `DialogBegin` and `DialogEnd` write one and zero to the dialogue-mode word.
    pub(super) fn set_dialogue_state(&mut self, active: bool) {
        self.dialogue_state = Some(u16::from(active));
    }

    /// Retained SOUP topic-menu state, distinct from the startup book menu and
    /// dialogue state. Engine initialization starts it at zero; world replacement
    /// retains it. See preservation engine/sound.md.
    pub fn menu_state(&self) -> u16 {
        self.menu_state
    }

    /// The topic-menu task establishes active state before processing choices.
    /// This is an engine lifecycle operation, not an installed script function.
    pub fn begin_topic_menu(&mut self) {
        self.menu_state = 1;
    }

    /// Accepting a topic selection clears the engine menu-state word.
    pub fn end_topic_menu(&mut self) {
        self.menu_state = 0;
    }

    /// Shared dialogue owner, absent until installed settings are prepared.
    pub fn dialogue(&self) -> Option<&super::DialogueState> {
        self.dialogue.as_ref()
    }

    /// Mutable dialogue owner for the host's supported speech lifecycle.
    pub fn dialogue_mut(&mut self) -> Option<&mut super::DialogueState> {
        self.dialogue.as_mut()
    }

    pub(super) fn prepare_dialogue(
        &mut self,
        settings: &cyrus_assets::script::DialogueSettings,
    ) -> Result<()> {
        if let Some(dialogue) = &self.dialogue {
            anyhow::ensure!(
                dialogue.settings() == settings,
                "dialogue settings differ from the active game's installed settings"
            );
        } else {
            self.dialogue = Some(super::DialogueState::new(settings)?);
        }
        Ok(())
    }

    /// Last retained LockoutPlayer low word, or unknown before an established call.
    pub fn lockout(&self) -> Option<u16> {
        self.lockout
    }
    /// Saved topic-menu selection text, distinct from the HUD activation prompt.
    pub fn highlighted_choice(&self) -> Option<&str> {
        self.highlighted_choice.as_deref()
    }
    /// The topic-menu owner supplies the text selected from its installed entries.
    pub fn remember_choice(&mut self, text: String) {
        self.highlighted_choice = Some(text);
    }
    pub(super) fn lockout_player(&mut self, value: i32) -> u16 {
        let value = value as u16;
        self.lockout = Some(value);
        self.highlighted_choice = Some(String::new());
        value
    }
}

/// One activation owner and pending prompt for a logical world generation.
#[derive(Default, Debug, Serialize)]
pub struct ActivationState {
    owner: Option<ScriptActorId>,
    prompt: Option<String>,
    holdoff: u32,
    prompt_override: u32,
}
impl ActivationState {
    pub fn owner(&self) -> Option<ScriptActorId> {
        self.owner
    }
    pub fn prompt(&self) -> Option<&str> {
        self.prompt.as_deref()
    }

    pub(super) fn clear_prompt_override(&mut self) {
        self.prompt_override = 0;
    }

    pub(super) fn call(
        &mut self,
        receiver: ScriptActorId,
        name: &str,
        arguments: &[Argument],
        inputs: Option<&ActivationInputs>,
        receiver_attributes: &[u8],
        selected_player: Option<(&super::ScriptActor, u16)>,
    ) -> Result<Option<i32>> {
        if name.eq_ignore_ascii_case("Deactivate") {
            anyhow::ensure!(arguments.is_empty(), "Deactivate: wrong argument count");
            self.holdoff = 0;
            if self.owner == Some(receiver) {
                self.owner = None;
            }
            return Ok(Some(0));
        }
        let torch = name.eq_ignore_ascii_case("TorchActivate");
        if !torch && !name.eq_ignore_ascii_case("Activate") {
            return Ok(None);
        }
        anyhow::ensure!(arguments.len() == 1, "{name}: wrong argument count");
        let bytes = arguments[0].integer()?.to_le_bytes();
        anyhow::ensure!(
            bytes.is_ascii(),
            "{name}: four-byte ASCII RTX label required"
        );
        let label = std::str::from_utf8(&bytes)?;
        let inputs = inputs.context("activation inputs unavailable")?;
        let (selected_player, lockout) =
            selected_player.context("activation selected player absent")?;
        let player_attributes = selected_player.attributes();
        let actor = inputs.actor(receiver.index)?;
        let player = inputs.player_state(selected_player)?;
        let (holding, drawing, sheathing) = weapon_predicates(
            player.state,
            player.animation,
            player.weapon_assigned,
            player.weapon_drawn,
            player_attributes[110],
            inputs.item_system_ready,
            inputs.shell_execution,
        );
        if lockout == 1
            || inputs.dialogue_open
            || actor.on_rope
            || actor.on_edge()
            || (player_attributes[105] == 1 && receiver_attributes[106] == 0)
            || self.owner.is_some_and(|owner| owner != receiver)
            || self.prompt.is_some()
            || holding
            || drawing
            || sheathing
            || inputs.lineup_result != 0
            || self.holdoff != 0
            || (player.torch_burning != 0) != torch
        {
            return Ok(Some(0));
        }
        let Some(text) = inputs.labels.get(&label.to_ascii_lowercase()) else {
            return Ok(Some(0));
        };
        self.prompt = Some(text.clone());
        if inputs.use_held {
            self.owner = Some(receiver);
            self.holdoff = 22;
            return Ok(Some(1));
        }
        Ok(Some(0))
    }

    pub(super) fn overlay(
        &mut self,
        inputs: &ActivationInputs,
        selected_player: &super::ScriptActor,
        ticks: u32,
        cycle_held: bool,
        override_ticks: u32,
        lockout: Option<u16>,
    ) -> Result<ActivationOverlay> {
        let player = inputs.player_state(selected_player)?;
        if self.prompt.is_none() {
            self.prompt_override = 0;
        }
        if lockout.unwrap_or(inputs.lockout) == 1 {
            return Ok(ActivationOverlay::Hidden);
        }
        if self.holdoff != 0 {
            self.holdoff = self.holdoff.saturating_sub(ticks);
            return Ok(ActivationOverlay::Hidden);
        }
        if self.prompt_override != 0 {
            self.prompt_override = self.prompt_override.saturating_sub(ticks);
        } else if self.prompt.is_some() {
            if inputs.dialogue_open
                || player.on_rope
                || player.on_edge()
                || matches!(player.water_state, 2 | 4)
                || inputs.render_disabled
            {
                return Ok(ActivationOverlay::Hidden);
            }
            let prompt = self.prompt.take().expect("pending prompt checked above");
            if cycle_held {
                self.prompt_override = override_ticks;
            }
            return Ok(ActivationOverlay::Prompt(prompt));
        }
        Ok(
            if selected_player.attributes()[105] == 1 || !inputs.item_system_ready {
                ActivationOverlay::Hidden
            } else {
                ActivationOverlay::Item {
                    visible: !inputs.render_disabled,
                    cycle_allowed: !weapon_predicates(
                        player.state,
                        player.animation,
                        player.weapon_assigned,
                        player.weapon_drawn,
                        selected_player.attributes()[110],
                        inputs.item_system_ready,
                        inputs.shell_execution,
                    )
                    .0,
                }
            },
        )
    }
}

#[cfg(test)]
mod idle_tests {
    use super::GameplayGlobals;
    use super::{NativeSource, ScriptActorId};
    use crate::random::EngineRandom;

    fn controller_settings() -> cyrus_assets::script::DialogueSettings {
        cyrus_assets::script::DialogueSettings {
            menu_start: [0, 0],
            max_menu_items: 300,
            max_dialog_lines: 16,
            max_text_width: 320,
            menu_traverse_delay: 1,
            print_text: true,
            use_speech: false,
            max_distance: 1024,
            rtx_filename: "TEST.RTX".into(),
        }
    }

    fn controller_globals() -> GameplayGlobals {
        let mut globals = GameplayGlobals::new_session();
        globals.prepare_dialogue(&controller_settings()).unwrap();
        globals.selected_health = Some(
            serde_json::from_value(serde_json::json!({
                "maximum":100,"displayed":0,"hit_inhibited":false
            }))
            .unwrap(),
        );
        globals
    }

    #[test]
    fn native_source_snapshot_rebinds_identity_and_legacy_remains_unknown() {
        let mut globals = GameplayGlobals::new_session();
        globals.native_source = NativeSource::Actor {
            generation: 7,
            index: 2,
        };
        let saved = globals.snapshot();
        let actors = [(
            ScriptActorId {
                generation: 7,
                index: 2,
            },
            ScriptActorId {
                generation: 11,
                index: 4,
            },
        )];
        let restored = GameplayGlobals::from_snapshot(
            saved.clone(),
            &controller_settings(),
            &actors,
            &Default::default(),
        )
        .unwrap();
        assert!(matches!(
            restored.native_source,
            NativeSource::Actor {
                generation: 11,
                index: 4
            }
        ));
        assert!(
            GameplayGlobals::from_snapshot(
                saved.clone(),
                &controller_settings(),
                &[],
                &Default::default()
            )
            .is_err()
        );
        let mut legacy = serde_json::to_value(saved).unwrap();
        legacy.as_object_mut().unwrap().remove("native_source");
        let restored = GameplayGlobals::from_snapshot(
            serde_json::from_value(legacy).unwrap(),
            &controller_settings(),
            &[],
            &Default::default(),
        )
        .unwrap();
        assert!(matches!(restored.native_source, NativeSource::Unknown));
    }

    #[test]
    fn session_snapshot_retains_prevalidation_admission_and_results() {
        for used in [false, true] {
            for result in [
                None,
                Some(super::super::OrdinaryGroundPrevalidation {
                    rollback: true,
                    sliding: false,
                }),
                Some(super::super::OrdinaryGroundPrevalidation {
                    rollback: false,
                    sliding: true,
                }),
            ] {
                let mut globals = controller_globals();
                globals.ground_prevalidation_used = used;
                globals.ground_prevalidation = result;
                let saved = serde_json::to_vec(&globals.snapshot()).unwrap();
                let restored = GameplayGlobals::from_snapshot(
                    serde_json::from_slice(&saved).unwrap(),
                    &controller_settings(),
                    &[],
                    &(0..16).map(|channel| (channel, channel)).collect(),
                )
                .unwrap();
                assert_eq!(restored.ground_prevalidation_used, used);
                assert_eq!(restored.ground_prevalidation, result);
            }
        }
    }

    #[test]
    fn replacement_retains_separate_action_inhibition_and_ledge_result() {
        for inhibition in [None, Some(0), Some(16), Some(255)] {
            for result in [None, Some(0), Some(-1), Some(i32::MAX)] {
                let mut globals = controller_globals();
                globals.selected_action_inhibition = inhibition;
                globals.selected_ledge_result = result;
                let saved = serde_json::to_vec(&globals.snapshot()).unwrap();
                let restored = GameplayGlobals::from_snapshot(
                    serde_json::from_slice(&saved).unwrap(),
                    &controller_settings(),
                    &[],
                    &(0..16).map(|channel| (channel, channel)).collect(),
                )
                .unwrap();
                assert_eq!(restored.selected_action_inhibition, inhibition);
                assert_eq!(restored.selected_ledge_result, result);
                globals.initialize_player_controller();
                assert_eq!(globals.selected_action_inhibition, Some(0));
                assert_eq!(globals.selected_ledge_result, result);
            }
        }
    }

    #[test]
    fn death_override_group_survives_replacement_and_explicit_initialization_clears_it() {
        assert_eq!(
            GameplayGlobals::default().player_death_override_group(),
            None
        );
        for group in [0, 13, 74, 121, 255] {
            let mut globals = controller_globals();
            globals.initialize_player_controller();
            globals.controller_prelude.as_mut().unwrap().override_group = group;
            let saved = serde_json::to_vec(&globals.snapshot()).unwrap();
            let mut restored = GameplayGlobals::from_snapshot(
                serde_json::from_slice(&saved).unwrap(),
                &controller_settings(),
                &[],
                &(0..16).map(|channel| (channel, channel)).collect(),
            )
            .unwrap();
            assert_eq!(restored.player_death_override_group(), Some(group));
            restored.initialize_player_controller();
            assert_eq!(restored.player_death_override_group(), Some(0));
        }
    }

    #[test]
    fn death_inhibition_and_animation_group_remain_independent_through_replacement() {
        assert_eq!(GameplayGlobals::default().player_hit_inhibited(), None);
        for group in [0, 13, 74, 121] {
            for inhibited in [false, true] {
                let mut globals = controller_globals();
                globals.initialize_player_controller();
                globals.controller_prelude.as_mut().unwrap().override_group = group;
                globals
                    .selected_health
                    .as_mut()
                    .unwrap()
                    .set_hit_inhibited(inhibited);
                let saved = serde_json::to_vec(&globals.snapshot()).unwrap();
                let mut restored = GameplayGlobals::from_snapshot(
                    serde_json::from_slice(&saved).unwrap(),
                    &controller_settings(),
                    &[],
                    &(0..16).map(|channel| (channel, channel)).collect(),
                )
                .unwrap();
                assert_eq!(restored.player_hit_inhibited(), Some(inhibited));
                assert_eq!(restored.player_death_override_group(), Some(group));
                assert_eq!(
                    restored.prepare_player_controller(0, 0, 0, 0).is_err(),
                    inhibited
                );
                restored.initialize_player_controller();
                assert_eq!(restored.player_hit_inhibited(), Some(false));
                assert_eq!(restored.player_death_override_group(), Some(0));
            }
        }
    }

    #[test]
    fn boolean_only_death_override_snapshot_cannot_invent_a_group() {
        let mut globals = controller_globals();
        globals.initialize_player_controller();
        let mut saved = serde_json::to_value(globals.snapshot()).unwrap();
        let controller = saved["controller_prelude"].as_object_mut().unwrap();
        controller.remove("override_group");
        controller.insert("override_active".into(), serde_json::json!(true));
        assert!(serde_json::from_value::<super::GameplayGlobalsSnapshot>(saved).is_err());
    }

    #[test]
    fn replacement_preserves_known_and_unknown_lineup_results() {
        for result in [None, Some(0), Some(-1), Some(17)] {
            let mut globals = controller_globals();
            globals.lineup_result = result;
            let saved = serde_json::to_vec(&globals.snapshot()).unwrap();
            let restored = GameplayGlobals::from_snapshot(
                serde_json::from_slice(&saved).unwrap(),
                &controller_settings(),
                &[],
                &(0..16).map(|channel| (channel, channel)).collect(),
            )
            .unwrap();
            assert_eq!(restored.lineup_result, result);
            assert_eq!(globals.lineup_result, result);
        }
    }

    #[test]
    fn controller_prelude_requires_initializer_and_preserves_timer_order() {
        let mut globals = controller_globals();
        assert!(globals.prepare_player_controller(1, 0, 0, 0).is_err());
        globals.initialize_player_controller();
        globals
            .prepare_player_controller(u32::MAX, 0, 0, 0)
            .unwrap();
        let state = globals.controller_prelude.as_mut().unwrap();
        state.conditional_ticks = 10;
        state.action_ticks = 7;
        state.secondary_ticks = u32::MAX;
        globals.prepare_player_controller(4, 1, 0, 0).unwrap();
        let state = globals.controller_prelude.as_ref().unwrap();
        assert_eq!(
            (
                state.conditional_ticks,
                state.action_ticks,
                state.secondary_ticks
            ),
            (10, 3, u32::MAX - 4)
        );
        globals.prepare_player_controller(4, 0, 1, 0).unwrap();
        assert_eq!(
            globals
                .controller_prelude
                .as_ref()
                .unwrap()
                .conditional_ticks,
            10
        );
        globals.prepare_player_controller(4, 0, 0, 0).unwrap();
        assert_eq!(
            globals
                .controller_prelude
                .as_ref()
                .unwrap()
                .conditional_ticks,
            6
        );
        globals
            .prepare_player_controller(u32::MAX, 0, 0, 0)
            .unwrap();
        let state = globals.controller_prelude.as_ref().unwrap();
        assert_eq!(
            (
                state.conditional_ticks,
                state.action_ticks,
                state.secondary_ticks
            ),
            (0, 0, 0)
        );
    }

    #[test]
    fn dialogue_mode_requires_fresh_session_initialization() {
        assert_eq!(GameplayGlobals::default().dialogue_state(), None);
        assert_eq!(GameplayGlobals::new_session().dialogue_state(), Some(0));
    }

    #[test]
    fn pending_action_precedes_unconditional_timers_and_death_admission_precedes_idle() {
        let mut globals = controller_globals();
        globals.initialize_player_controller();
        globals.advance_player_idle_clock(13);
        globals
            .selected_health
            .as_mut()
            .unwrap()
            .set_hit_inhibited(true);
        let state = globals.controller_prelude.as_mut().unwrap();
        state.pending_action = true;
        state.override_group = 74;
        state.conditional_ticks = 9;
        state.action_ticks = 9;
        state.secondary_ticks = 9;
        assert!(
            globals
                .prepare_player_controller(4, 0, 0, 0)
                .unwrap_err()
                .to_string()
                .contains("action 74")
        );
        let state = globals.controller_prelude.as_ref().unwrap();
        assert_eq!(
            (
                state.conditional_ticks,
                state.action_ticks,
                state.secondary_ticks
            ),
            (5, 9, 9)
        );
        assert!(
            globals
                .prepare_player_controller(4, 0, 0, 1)
                .unwrap_err()
                .to_string()
                .contains("death execution")
        );
        let state = globals.controller_prelude.as_ref().unwrap();
        assert_eq!(
            (
                state.conditional_ticks,
                state.action_ticks,
                state.secondary_ticks
            ),
            (1, 5, 5)
        );
        assert_eq!(globals.player_idle_counter(), Some(13));
        globals.initialize_player_controller();
        globals.prepare_player_controller(4, 0, 0, 0).unwrap();
        assert_eq!(globals.player_idle_counter(), Some(13));
    }

    #[test]
    fn sound_inhibition_is_sampled_before_pending_action_failure() {
        let mut globals = controller_globals();
        globals.initialize_player_controller();
        assert!(
            !globals
                .sound_maintenance
                .advance(1400, &mut |_| Ok(()))
                .unwrap()
        );
        globals.menu_state = 2;
        globals.controller_prelude.as_mut().unwrap().pending_action = true;
        assert!(globals.prepare_player_controller(1, 0, 0, 0).is_err());
        assert!(
            !globals
                .sound_maintenance
                .advance(1, &mut |_| {
                    panic!("inhibited timer reset after pending action failure")
                })
                .unwrap()
        );
        globals.menu_state = 0;
        globals.controller_prelude.as_mut().unwrap().pending_action = false;
        globals.prepare_player_controller(1, 0, 0, 0).unwrap();
        let mut stages = 0;
        assert!(
            globals
                .sound_maintenance
                .advance(1, &mut |_| {
                    stages += 1;
                    Ok(())
                })
                .unwrap()
        );
        assert_eq!(stages, 4);
    }

    #[test]
    fn idle_reset_precedes_lockout_and_requests_preserve_thresholds_and_rng() {
        let mut globals = GameplayGlobals::default();
        let mut random = EngineRandom::from_state(5);
        assert!(
            globals
                .prepare_player_idle(false, Some(0), 100, &mut random)
                .is_err()
        );
        globals.lockout_player(2);
        assert_eq!(
            globals
                .prepare_player_idle(false, Some(20), 100, &mut random)
                .unwrap(),
            None
        );
        assert_eq!(globals.player_idle_counter(), Some(0));
        globals.advance_player_idle_clock(1);
        assert_eq!(
            globals
                .prepare_player_idle(false, None, 100, &mut random)
                .unwrap(),
            None
        );
        assert_eq!(globals.player_idle_counter(), Some(0));
        globals.advance_player_idle_clock(13);
        assert_eq!(
            globals
                .prepare_player_idle(false, Some(0), 100, &mut random)
                .unwrap(),
            None
        );
        assert_eq!(globals.player_idle_counter(), Some(13));
        assert_eq!(random.state(), 5);
        globals.lockout_player(0);
        globals.begin_topic_menu();
        assert_eq!(
            globals
                .prepare_player_idle(false, Some(0), 100, &mut random)
                .unwrap(),
            Some(2)
        );
        assert_eq!(random.state(), 2701728395);
        assert_eq!(globals.player_idle_counter(), Some(-1));
        assert_eq!(globals.menu_state(), 1);
        globals.advance_player_idle_clock(5);
        assert_eq!(
            globals
                .prepare_player_idle(false, Some(0), 39, &mut random)
                .unwrap(),
            None
        );
        globals.advance_player_idle_clock(1);
        assert_eq!(
            globals
                .prepare_player_idle(false, Some(0), 39, &mut random)
                .unwrap(),
            Some(4)
        );
        assert_eq!(random.state(), 2701728395);
        assert_eq!(globals.player_idle_counter(), Some(-1));
        assert_eq!(
            globals
                .prepare_player_idle(true, Some(0), 39, &mut random)
                .unwrap(),
            None
        );
        assert_eq!(globals.player_idle_counter(), Some(0));
    }
}

/// Session-only state is captured once, independently of retained world records.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub(super) struct GameplayGlobalsSnapshot {
    #[serde(default)]
    native_source: NativeSource,
    native_command: super::NativeCommandState,
    native_command_return: u8,
    walk_route: super::WalkRouteState,
    locomotion_called: u8,
    ground_prevalidation_used: bool,
    ground_prevalidation: Option<super::OrdinaryGroundPrevalidation>,
    lineup_result: Option<i32>,
    combat_messages: super::combat::CombatMessagesSnapshot,
    dialogue: Option<super::DialogueSnapshot>,
    lockout: Option<u16>,
    player_idle_counter: Option<i32>,
    controller_prelude: Option<ControllerPrelude>,
    selected_combat_inhibition: Option<i32>,
    selected_health: Option<super::SelectedHealthState>,
    selected_action_inhibition: Option<u8>,
    selected_ledge_result: Option<i32>,
    #[serde(default)]
    world_loop_frame: u32,
    sound_maintenance: super::maintenance::SoundMaintenanceSnapshot,
    menu_state: u16,
    dialogue_state: Option<u16>,
    highlighted_choice: Option<String>,
    candle_show_requested: bool,
    #[serde(default)]
    potions: [i32; 4],
    #[serde(default)]
    player_swapped: bool,
    #[serde(default)]
    group_sync: super::GroupSync,
}
impl GameplayGlobals {
    /// Shared next-hop link flags and backtracking inhibition.
    pub fn walk_route(&self) -> &super::WalkRouteState {
        &self.walk_route
    }

    /// Current shared execution state; task-slot load/save may replace it.
    pub fn native_command(&self) -> &super::NativeCommandState {
        &self.native_command
    }

    pub(super) fn snapshot(&self) -> GameplayGlobalsSnapshot {
        GameplayGlobalsSnapshot {
            native_source: self.native_source,
            native_command: self.native_command,
            native_command_return: self.native_command_return,
            walk_route: self.walk_route,
            locomotion_called: self.locomotion_called,
            ground_prevalidation_used: self.ground_prevalidation_used,
            ground_prevalidation: self.ground_prevalidation,
            lineup_result: self.lineup_result,
            combat_messages: self.combat_messages.snapshot(),
            dialogue: self.dialogue.as_ref().map(super::DialogueState::snapshot),
            lockout: self.lockout,
            player_idle_counter: self.player_idle_counter,
            controller_prelude: self.controller_prelude.clone(),
            selected_combat_inhibition: self.selected_combat_inhibition,
            selected_health: self.selected_health.clone(),
            selected_action_inhibition: self.selected_action_inhibition,
            selected_ledge_result: self.selected_ledge_result,
            world_loop_frame: self.world_loop_frame,
            sound_maintenance: self.sound_maintenance.snapshot(),
            menu_state: self.menu_state,
            dialogue_state: self.dialogue_state,
            highlighted_choice: self.highlighted_choice.clone(),
            candle_show_requested: self.candle_show_requested,
            potions: self.potions,
            player_swapped: self.player_swapped,
            group_sync: self.group_sync.clone(),
        }
    }

    /// The aggregate supplies validated current/visited actor and restored sample owners.
    pub(super) fn from_snapshot(
        saved: GameplayGlobalsSnapshot,
        settings: &cyrus_assets::script::DialogueSettings,
        actors: &[(ScriptActorId, ScriptActorId)],
        samples: &BTreeMap<u8, u8>,
    ) -> Result<Self> {
        let mut bindings = BTreeMap::new();
        let mut destinations = std::collections::BTreeSet::new();
        for &(old, new) in actors {
            anyhow::ensure!(
                bindings.insert((old.generation, old.index), new).is_none(),
                "session snapshot: duplicate saved actor binding"
            );
            anyhow::ensure!(
                destinations.insert((new.generation, new.index)),
                "session snapshot: noninjective actor binding"
            );
        }
        let rebind = |generation, index| {
            bindings
                .get(&(generation, index))
                .copied()
                .with_context(|| {
                    format!("session snapshot actor {generation}/{index}: replacement absent")
                })
        };
        if let Some(health) = &saved.selected_health {
            health.validate()?;
        }
        let combat_messages = super::CombatMessages::from_snapshot(saved.combat_messages, &rebind)?;
        let dialogue = saved
            .dialogue
            .map(|dialogue| {
                super::DialogueState::from_snapshot(settings, dialogue, actors, samples)
            })
            .transpose()?;
        let native_source = match saved.native_source {
            NativeSource::Unknown => NativeSource::Unknown,
            NativeSource::Actor { generation, index } => {
                let actor = rebind(generation, index)?;
                NativeSource::Actor {
                    generation: actor.generation,
                    index: actor.index,
                }
            }
        };
        Ok(Self {
            native_source,
            native_command: saved.native_command,
            native_command_return: saved.native_command_return,
            walk_route: saved.walk_route,
            locomotion_called: saved.locomotion_called,
            ground_prevalidation_used: saved.ground_prevalidation_used,
            ground_prevalidation: saved.ground_prevalidation,
            combat_messages,
            dialogue,
            lockout: saved.lockout,
            lineup_result: saved.lineup_result,
            player_idle_counter: saved.player_idle_counter,
            controller_prelude: saved.controller_prelude,
            selected_combat_inhibition: saved.selected_combat_inhibition,
            selected_health: saved.selected_health,
            selected_action_inhibition: saved.selected_action_inhibition,
            selected_ledge_result: saved.selected_ledge_result,
            world_loop_frame: saved.world_loop_frame,
            sound_maintenance: super::SoundMaintenance::from_snapshot(saved.sound_maintenance),
            menu_state: saved.menu_state,
            dialogue_state: saved.dialogue_state,
            highlighted_choice: saved.highlighted_choice,
            candle_show_requested: saved.candle_show_requested,
            potions: saved.potions,
            player_swapped: saved.player_swapped,
            group_sync: saved.group_sync,
            host_requests: HostRequests::default(),
        })
    }
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub(super) struct ActivationSnapshot {
    owner: Option<(u64, usize)>,
    prompt: Option<String>,
    holdoff: u32,
    prompt_override: u32,
}
impl ActivationState {
    pub(super) fn snapshot(&self) -> ActivationSnapshot {
        ActivationSnapshot {
            owner: self.owner.map(|id| (id.generation, id.index)),
            prompt: self.prompt.clone(),
            holdoff: self.holdoff,
            prompt_override: self.prompt_override,
        }
    }

    pub(super) fn from_snapshot(
        saved: ActivationSnapshot,
        rebind: &impl Fn(u64, usize) -> Result<ScriptActorId>,
    ) -> Result<Self> {
        Ok(Self {
            owner: saved
                .owner
                .map(|(generation, index)| rebind(generation, index))
                .transpose()?,
            prompt: saved.prompt,
            holdoff: saved.holdoff,
            prompt_override: saved.prompt_override,
        })
    }
}
