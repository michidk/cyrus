//! Torches: `LightTorch` and `UnlightTorch`, the burn countdown and the held-torch progression.
//!
//! A torch burns while the actor's counter is nonzero. Lighting and dousing are lifecycle states
//! 33 and 34 that play animation groups 149 and 150; the counter changes once the group reaches
//! the actor's lighting or dousing frame threshold. Presentation of the held torch and its light
//! belongs to the attachment owner and is not modelled here.
use super::{GameHost, ScriptActorId, ScriptWorld};
use anyhow::{Context, Result};

const LIGHTING: u8 = 33;
const DOUSING: u8 = 34;
const LIGHT_GROUP: u16 = 149;
const DOUSE_GROUP: u16 = 150;
/// Attributes holding the progress threshold of the lighting and dousing groups.
const LIGHT_FRAME_ATTRIBUTE: usize = 103;
const DOUSE_FRAME_ATTRIBUTE: usize = 104;

impl ScriptWorld {
    fn torch_gates(&self, id: ScriptActorId, inputs: &TorchInputs, group: u16) -> Result<bool> {
        let actor = &self.actors[self.resolve(id)?];
        let Some(control) = actor.control.as_ref() else {
            return Ok(false);
        };
        let has_group = actor
            .animation
            .as_ref()
            .is_some_and(|animation| animation.playback.has_group(group));
        Ok(inputs.item_system_ready
            && !inputs.shell
            && has_group
            && !(27..=32).contains(&control.living_state))
    }

    /// `LightTorch`: begin lighting when nothing else holds the actor. An actor that cannot light
    /// stays unchanged.
    pub(super) fn light_torch(
        &mut self,
        id: ScriptActorId,
        inputs: TorchInputs,
        ticks: u32,
        sounds: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<()> {
        if !self.torch_gates(id, &inputs, LIGHT_GROUP)? {
            return Ok(());
        }
        let index = self.resolve(id)?;
        let actor = &self.actors[index];
        let control = actor.control.as_ref().expect("gated control");
        let animating = lighting_or_dousing(actor);
        let refused = matches!(actor.surface, Some(1..=4))
            || control.torch_burning != 0
            || animating
            || inputs.drawing
            || inputs.sheathing
            || inputs.holding
            || control.falling_state != 0
            || control.queued_motion != 0
            || control.living_state == 15
            || (10..=19).contains(&control.living_state);
        if refused {
            return Ok(());
        }
        self.request_ordinary_group(id, LIGHT_GROUP, false, ticks, sounds)?;
        let control = self.actors[index].control.as_mut().expect("gated control");
        control.living_state = LIGHTING;
        control.motion_companion = 0;
        Ok(())
    }

    /// `UnlightTorch`: begin dousing unless the actor is mid-transition or busy with a weapon.
    pub(super) fn unlight_torch(
        &mut self,
        id: ScriptActorId,
        inputs: TorchInputs,
        ticks: u32,
        sounds: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<()> {
        if !self.torch_gates(id, &inputs, DOUSE_GROUP)? {
            return Ok(());
        }
        let index = self.resolve(id)?;
        if lighting_or_dousing(&self.actors[index]) || inputs.drawing || inputs.sheathing {
            return Ok(());
        }
        self.request_ordinary_group(id, DOUSE_GROUP, false, ticks, sounds)?;
        let control = self.actors[index].control.as_mut().expect("gated control");
        control.living_state = DOUSING;
        control.motion_companion = 0;
        Ok(())
    }

    /// The burn countdown, once every eight world-loop iterations for an actor with a burning
    /// torch that is not being doused. In deep water the torch goes out at once; in shallow water
    /// it is doused; otherwise the counter loses the nominal ticks and dousing starts at zero.
    pub(super) fn update_torch_burn(
        &mut self,
        id: ScriptActorId,
        inputs: TorchInputs,
        ticks: u32,
        sounds: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<()> {
        let index = self.resolve(id)?;
        let actor = &self.actors[index];
        let Some(control) = actor.control.as_ref() else {
            return Ok(());
        };
        if control.torch_burning == 0
            || self.globals.world_loop_frame & 7 != 0
            || control.living_state == DOUSING
        {
            return Ok(());
        }
        let surface = actor.surface.unwrap_or(0);
        let control = self.actors[index]
            .control
            .as_mut()
            .expect("checked control");
        if matches!(surface, 2 | 4) {
            control.torch_burning = 0;
        } else if (1..=4).contains(&surface) {
            self.unlight_torch(id, inputs, ticks, sounds)?;
        } else {
            control.torch_burning = control.torch_burning.saturating_sub(ticks.min(255) as u8);
            if control.torch_burning == 0 {
                self.unlight_torch(id, inputs, ticks, sounds)?;
            }
        }
        Ok(())
    }

    /// The held-torch progression for an actor whose held-item byte is set: lighting finishes at
    /// the lighting frame threshold and starts the burn from the saved counter, or from the
    /// installed start value when none is saved; dousing finishes at its threshold, saving the
    /// counter, and spends the torch when it had burned out. Deep water or a held weapon drops
    /// the held item.
    pub(super) fn update_held_torch(
        &mut self,
        id: ScriptActorId,
        inputs: TorchInputs,
        ticks: u32,
        torch_start: &mut dyn FnMut() -> Result<u8>,
        spend: &mut dyn FnMut(u8) -> Result<()>,
        sounds: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<()> {
        let index = self.resolve(id)?;
        let actor = &self.actors[index];
        let Some(control) = actor.control.as_ref() else {
            return Ok(());
        };
        let Some(item) = control.held_item.filter(|&item| item != 0) else {
            return Ok(());
        };
        if !inputs.item_system_ready || actor.animation.is_none() || inputs.shell {
            return Ok(());
        }
        if matches!(actor.surface, Some(2 | 4)) || inputs.holding {
            self.actors[index]
                .control
                .as_mut()
                .expect("checked control")
                .held_item = None;
            return Ok(());
        }
        let state = control.living_state;
        let (group, threshold) = match state {
            LIGHTING => (LIGHT_GROUP, LIGHT_FRAME_ATTRIBUTE),
            DOUSING => (DOUSE_GROUP, DOUSE_FRAME_ATTRIBUTE),
            _ => return Ok(()),
        };
        self.request_ordinary_group(id, group, false, ticks, sounds)?;
        let actor = &mut self.actors[index];
        let (current, _, progress) = actor
            .animation
            .as_ref()
            .context("held torch animation unavailable")?
            .playback
            .frame_wait_state()?;
        if current != i32::from(group) || actor.attributes[threshold] > progress {
            return Ok(());
        }
        let control = actor.control.as_mut().expect("checked control");
        if state == LIGHTING {
            if control.torch_saved == 0 {
                control.torch_saved = torch_start()?;
            }
            control.torch_burning = control.torch_saved;
            control.living_state = 0;
        } else {
            let burned_out = control.torch_burning == 0;
            control.torch_saved = control.torch_burning;
            control.torch_burning = 0;
            control.living_state = 0;
            control.held_item = None;
            if burned_out {
                spend(item)?;
            }
        }
        Ok(())
    }
}

/// Weapon and item-system facts the torch rules read from the caller.
#[derive(Clone, Copy)]
pub(super) struct TorchInputs {
    pub item_system_ready: bool,
    pub shell: bool,
    pub drawing: bool,
    pub sheathing: bool,
    pub holding: bool,
}

/// Whether the actor is in, or playing the group of, a lighting or dousing transition.
fn lighting_or_dousing(actor: &super::ScriptActor) -> bool {
    let state = actor
        .control
        .as_ref()
        .map_or(0, |control| control.living_state);
    let group = actor.bound_animation_group().flatten();
    matches!(state, LIGHTING | DOUSING) || matches!(group, Some(LIGHT_GROUP | DOUSE_GROUP))
}

impl ScriptWorld {
    /// Torch rule inputs for an ordinary update of `index`.
    pub(super) fn ordinary_torch_inputs(
        &self,
        index: usize,
        inputs: super::ScriptInputs<'_>,
    ) -> Result<TorchInputs> {
        let item_system_ready = inputs
            .hud
            .map(super::super::ScriptHud::inventory_ready)
            .transpose()?
            .unwrap_or(false);
        let shell = inputs.shell_execution.unwrap_or(false);
        let actor = &self.actors[index];
        let (holding, drawing, sheathing) = match actor.control.as_ref() {
            Some(control) => super::super::activation::weapon_predicates(
                control.living_state,
                actor
                    .animation
                    .as_ref()
                    .map(|animation| super::super::ActivationAnimation {
                        group: animation.playback.current_group(),
                    }),
                control.combat.is_some(),
                control.weapon_drawn,
                actor.attributes[110],
                item_system_ready,
                shell,
            ),
            None => (false, false, false),
        };
        Ok(TorchInputs {
            item_system_ready,
            shell,
            drawing,
            sheathing,
            holding,
        })
    }

    /// The burn countdown for one actor, as the ordinary update runs it ahead of the actor's own
    /// update.
    pub fn advance_torch_burn(
        &mut self,
        id: ScriptActorId,
        inputs: super::ScriptInputs<'_>,
        ticks: u32,
        sounds: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<()> {
        let index = self.resolve(id)?;
        let torch = self.ordinary_torch_inputs(index, inputs)?;
        self.update_torch_burn(id, torch, ticks, sounds)
    }

    /// Run the held-torch progression for one actor, recording a failure on the actor.
    pub fn run_held_torch(
        &mut self,
        id: ScriptActorId,
        inputs: super::ScriptInputs<'_>,
        ticks: u32,
        sounds: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) {
        let Ok(index) = self.resolve(id) else {
            return;
        };
        let Some(item) = self.actors[index]
            .control
            .as_ref()
            .and_then(|control| control.held_item)
            .filter(|&item| item != 0)
        else {
            return;
        };
        let result = (|| -> Result<()> {
            let torch = self.ordinary_torch_inputs(index, inputs)?;
            let hud = inputs
                .hud
                .context("held torch inventory host unavailable")?;
            anyhow::ensure!(
                !torch.item_system_ready || hud.item_defined(u16::from(item)),
                "held item {item}: ITEM.INI definition absent"
            );
            self.update_held_torch(
                id,
                torch,
                ticks,
                &mut || hud.torch_start(),
                &mut |item| hud.drop_one(u16::from(item)),
                sounds,
            )
        })();
        if let Err(error) = result {
            let actor = &mut self.actors[index];
            actor.update_failure = Some(format!(
                "world {} instance {} actor {} held torch: {error:#}",
                self.world,
                index,
                actor.name()
            ));
        }
    }
}

impl GameHost<'_, '_, '_, '_> {
    pub(super) fn torch_inputs(&self, index: usize, function: &str) -> Result<TorchInputs> {
        let (holding, drawing, sheathing) = self.actor_weapon_predicates(index, function)?;
        Ok(TorchInputs {
            item_system_ready: self
                .inputs
                .hud
                .context("torch inventory host unavailable")?
                .inventory_ready()?,
            shell: self
                .inputs
                .shell_execution
                .context("torch shell context unavailable")?,
            drawing,
            sheathing,
            holding,
        })
    }
}
