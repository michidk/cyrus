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
#[derive(Debug, Serialize)]
struct ControllerPrelude {
    override_active: bool,
    pending_action: bool,
    conditional_ticks: u32,
    action_ticks: u32,
    secondary_ticks: u32,
}

/// Gameplay globals retain engine initialization and native-call assignments.
/// Unknown startup values remain None.
/// The host retains these globals while replacing the logical world registry.
#[derive(Default, Debug, Serialize)]
pub struct GameplayGlobals {
    /// Engine message queues persist independently of a logical world registry.
    pub(super) combat_messages: super::CombatMessages,
    pub(super) dialogue: Option<super::DialogueState>,
    lockout: Option<u16>,
    pub(super) lineup_result: Option<i32>,
    player_idle_counter: Option<i32>,
    controller_prelude: Option<ControllerPrelude>,
    pub(super) selected_combat_inhibition: Option<i32>,
    pub(super) sound_maintenance: super::SoundMaintenance,
    menu_state: u16,
    dialogue_state: Option<u16>,
    highlighted_choice: Option<String>,
    candle_show_requested: bool,
}
impl GameplayGlobals {
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

    pub(super) fn player_death_override(&self) -> Option<bool> {
        self.controller_prelude
            .as_ref()
            .map(|state| state.override_active)
    }

    /// Run only at the explicit standalone-player initializer, never merely on a
    /// world swap or save restore. The sound-maintenance owner resets separately.
    pub(super) fn initialize_player_controller(&mut self) {
        self.lineup_result = Some(0);
        self.selected_combat_inhibition = Some(0);
        self.sound_maintenance.initialize_player();
        self.controller_prelude = Some(ControllerPrelude {
            override_active: false,
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
            !state.override_active,
            "player controller: override execution unavailable"
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
    use crate::random::EngineRandom;

    fn controller_globals() -> GameplayGlobals {
        let mut globals = GameplayGlobals::new_session();
        globals
            .prepare_dialogue(&cyrus_assets::script::DialogueSettings {
                menu_start: [0, 0],
                max_menu_items: 300,
                max_dialog_lines: 16,
                max_text_width: 320,
                menu_traverse_delay: 1,
                print_text: true,
                use_speech: false,
                max_distance: 1024,
                rtx_filename: "TEST.RTX".into(),
            })
            .unwrap();
        globals
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
    fn pending_action_precedes_unconditional_timers_and_override_precedes_idle() {
        let mut globals = controller_globals();
        globals.initialize_player_controller();
        globals.advance_player_idle_clock(13);
        let state = globals.controller_prelude.as_mut().unwrap();
        state.pending_action = true;
        state.override_active = true;
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
                .contains("override")
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
