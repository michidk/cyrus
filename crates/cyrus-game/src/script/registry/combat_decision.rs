//! Ordinary combat composes live services in the original decision order.
use super::{ScriptActorId, ScriptInputs, ScriptWorld};
use anyhow::{Context, Result};

impl ScriptWorld {
    /// Run one admitted nonplayer decision using the session random and sound owners.
    /// Cleanup may end the pass. Coordination's entry branch survives its state writes.
    /// Pacing uses the raw BIOS sample; waiting-taunt type uses world-loop parity.
    /// Earlier service writes survive later errors, following preservation's
    /// [decision rules](https://github.com/michidk/redguard-preservation/commit/fdbd0b6).
    pub fn run_ordinary_combat_decision(
        &mut self,
        id: ScriptActorId,
        selected_player: ScriptActorId,
        inputs: ScriptInputs<'_>,
        random: &mut crate::random::EngineRandom,
        sounds: &mut dyn FnMut(ScriptActorId, crate::script::ScriptSound) -> Result<()>,
    ) -> Result<()> {
        let result = (|| -> Result<()> {
            let clock = inputs
                .combat_clock_sample
                .context("combat BIOS-clock sample unavailable")? as i32;
            let query =
                inputs
                    .locomotion_geometry
                    .map(|geometry| super::obstacle::OrdinaryObstacleQuery {
                        scene: geometry.ground.scene,
                        animations: geometry.ground.animations,
                        prepared: geometry.ground.prepared,
                        vertical_offset: 0,
                        respond: false,
                    });
            let Some(sample) = self.prepare_ordinary_combat_decision(id, selected_player, query)?
            else {
                return Ok(());
            };
            if sample.engaged
                && (self.execute_combat_defence(id, -1, clock, inputs, random, sounds)?
                    || self.execute_combat_attack(id, -1, inputs, sounds)?)
            {
                return Ok(());
            }
            let branch = self.coordinate_ordinary_combat(
                id,
                super::OrdinaryCombatCoordinationInput {
                    engaged: sample.engaged,
                    selected_player,
                    frame_value: clock,
                },
                inputs,
                random,
                sounds,
            )?;
            if branch == super::OrdinaryCombatBranch::Disengaged {
                let (_, holding_weapon) = self.spacing_weapon(id, inputs)?;
                let combat_active = self.actors[self.resolve(id)?]
                    .control
                    .as_ref()
                    .and_then(|control| control.combat.as_ref())
                    .context("receiver combat unavailable")?
                    .flags
                    & 4
                    != 0;
                return self.move_disengaged_combat(
                    id,
                    sample.distance,
                    super::slope::OrdinarySlopeInputs {
                        selected_player: Some(selected_player),
                        holding_weapon,
                        combat_active,
                        frame_scale: inputs
                            .frame_scale
                            .context("combat frame scale unavailable")?,
                        ticks: inputs
                            .nominal_ticks
                            .context("combat nominal ticks unavailable")?,
                    },
                    inputs,
                    sounds,
                );
            }
            let spacing = super::spacing::OrdinarySpacingInput {
                distance: sample.distance,
                tick_counter: clock,
            };
            self.space_ordinary(id, spacing, inputs, random, sounds)?;
            if branch == super::OrdinaryCombatBranch::Active {
                self.counter_ordinary_combat(id, spacing, inputs, random, sounds)?;
                let weapon = super::WeaponInputs {
                    selected_player,
                    item_system_ready: inputs
                        .hud
                        .context("combat inventory owner unavailable")?
                        .inventory_ready()?,
                    shell_execution: inputs
                        .shell_execution
                        .context("combat shell context unavailable")?,
                };
                self.select_ordinary_combat_defence(
                    id,
                    spacing,
                    weapon,
                    clock,
                    inputs,
                    random,
                    &mut |_, _, _, _| anyhow::bail!("selected-player defence movement unavailable"),
                    sounds,
                )?;
            }
            if random.draw() * 100 / 32767 < 2 {
                let trigger = if branch == super::OrdinaryCombatBranch::Active {
                    5
                } else {
                    let allocation = self.actors[self.resolve(id)?]
                        .control
                        .as_ref()
                        .and_then(|control| control.combat.as_ref())
                        .context("receiver combat unavailable")?
                        .allocation_index;
                    if (allocation as u32).wrapping_add(self.globals.world_loop_frame) & 1 == 0 {
                        5
                    } else {
                        8
                    }
                };
                self.request_timed_combat_taunt(id, trigger, clock, inputs, random, sounds)?;
            }
            Ok(())
        })();
        result.with_context(|| {
            format!(
                "world {} instance {} ordinary combat decision",
                self.world, id.index
            )
        })
    }
}
