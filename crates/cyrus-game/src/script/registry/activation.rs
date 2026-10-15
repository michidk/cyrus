//! Ordinary activation reads current logical actors at each invocation.
use super::{ScriptActor, ScriptActorId, ScriptActorSource, ScriptWorld};
use crate::script::{
    ActivationActor, ActivationAnimation, ActivationInputs, ActivationOverlay, GameplayGlobals,
    ScriptHud,
};
use anyhow::{Context, Result};
use cyrus_script::Argument;
use std::collections::BTreeMap;

fn actor_state(actor: &ScriptActor) -> Result<ActivationActor> {
    let control = actor
        .control
        .as_ref()
        .context("activation control state unavailable")?;
    Ok(ActivationActor {
        state: control.living_state,
        on_rope: control.rope_attached,
        water_state: actor
            .surface
            .context("activation surface state unavailable")?,
        torch_burning: control.torch_burning,
        weapon_assigned: control.combat_death_default.is_some(),
        weapon_drawn: control.weapon_drawn,
        animation: actor.animation.as_ref().map(|_| ActivationAnimation {
            group: actor.bound_animation_group().flatten(),
        }),
    })
}

pub(super) fn ordinary_inputs(
    actors: &[ScriptActor],
    generation: u64,
    globals: &GameplayGlobals,
    hud: &ScriptHud,
    use_held: bool,
    receiver: Option<(usize, &[Argument])>,
) -> Result<ActivationInputs> {
    let index = actors
        .iter()
        .position(|actor| matches!(actor.source, ScriptActorSource::Player))
        .context("activation selected player unavailable")?;
    let mut states = BTreeMap::from([(index, actor_state(&actors[index])?)]);
    let mut labels = BTreeMap::new();
    if let Some((receiver, arguments)) = receiver {
        let [label] = arguments else {
            anyhow::bail!("activation requires one RTX label")
        };
        let bytes = label.integer()?.to_le_bytes();
        anyhow::ensure!(
            bytes.is_ascii(),
            "activation requires four-byte ASCII RTX label"
        );
        let label = std::str::from_utf8(&bytes)?.to_ascii_lowercase();
        if let Some(text) = hud.activation_label(&label)? {
            labels.insert(label, text);
        }
        states.insert(receiver, actor_state(&actors[receiver])?);
    }
    Ok(ActivationInputs {
        player: ScriptActorId { generation, index },
        actors: states,
        labels,
        lockout: globals
            .lockout()
            .context("activation lockout unavailable")?,
        dialogue_open: globals.menu_state() != 0,
        lineup_result: globals
            .lineup_result
            .context("activation player-lineup result unavailable")?,
        use_held,
        item_system_ready: hud.inventory_ready()?,
        shell_execution: false,
        render_disabled: false,
    })
}

impl ScriptWorld {
    /// Draw ordinary prompts from current actor and session state, using nominal ticks.
    /// Inspection callers retain their explicit activation inputs. Alignment execution
    /// remains unsupported; its unknown result prevents subsequent prompt admission.
    pub fn ordinary_activation_overlay(
        &mut self,
        hud: &ScriptHud,
        ticks: u32,
        cycle_held: bool,
        override_ticks: u32,
    ) -> Result<ActivationOverlay> {
        let inputs = ordinary_inputs(
            &self.actors,
            self.generation,
            &self.globals,
            hud,
            false,
            None,
        )?;
        self.activation_overlay(&inputs, ticks, cycle_held, override_ticks)
    }
}
