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
    fn on_edge(self) -> bool {
        (10..=19).contains(&self.state)
    }
    fn weapon_predicates(
        self,
        holding_attribute: u8,
        inputs: &ActivationInputs,
    ) -> (bool, bool, bool) {
        let ready = inputs.item_system_ready && !inputs.shell_execution && self.animation.is_some();
        let group = self.animation.and_then(|animation| animation.group);
        let drawing = ready && (self.state == 20 || group == Some(91));
        let sheathing = ready && (self.state == 21 || group == Some(129));
        let holding = holding_attribute != 0
            || (ready && self.weapon_assigned && self.weapon_drawn && !sheathing);
        (holding, drawing, sheathing)
    }
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

    pub(super) fn call(
        &mut self,
        receiver: ScriptActorId,
        name: &str,
        arguments: &[Argument],
        inputs: Option<&ActivationInputs>,
        receiver_attributes: &[u8],
        selected_player: Option<&super::ScriptActor>,
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
        let selected_player = selected_player.context("activation selected player absent")?;
        let player_attributes = selected_player.attributes();
        let actor = inputs.actor(receiver.index)?;
        let player = inputs.player_state(selected_player)?;
        let (holding, drawing, sheathing) =
            player.weapon_predicates(player_attributes[110], inputs);
        if inputs.lockout == 1
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
    ) -> Result<ActivationOverlay> {
        let player = inputs.player_state(selected_player)?;
        if self.prompt.is_none() {
            self.prompt_override = 0;
        }
        if inputs.lockout == 1 {
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
                    cycle_allowed: !player
                        .weapon_predicates(selected_player.attributes()[110], inputs)
                        .0,
                }
            },
        )
    }
}
