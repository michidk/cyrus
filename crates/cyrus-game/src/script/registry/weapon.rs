//! Authoritative combat entry and weapon transitions follow preservation's engine/animation.md.
use super::{ScriptActorId, ScriptWorld};
use anyhow::{Context, Result};

/// Shared engine gates for a caller-admitted weapon operation.
/// Selected identity is validated against the current logical generation.
#[derive(Clone, Copy, Debug)]
pub struct WeaponInputs {
    pub selected_player: ScriptActorId,
    pub item_system_ready: bool,
    pub shell_execution: bool,
}

/// Timing and presentation gates supplied by the owning combat invocation.
#[derive(Clone, Copy, Debug)]
pub struct CombatEntryInputs {
    pub weapon: WeaponInputs,
    pub nominal_ticks: u32,
    pub taunt_animation_admitted: bool,
    /// Global IsDead override for the selected actor, independent of lifecycle state.
    pub selected_player_death_override: bool,
}

/// A synchronous borrow of one authoritative logical actor and its shared globals.
pub(super) struct CombatReceiver<'a> {
    actor: &'a mut super::ScriptActor,
    globals: &'a mut crate::script::GameplayGlobals,
    world: u32,
    id: ScriptActorId,
    position: Option<[i32; 3]>,
}
impl<'a> CombatReceiver<'a> {
    pub(super) fn new(
        actor: &'a mut super::ScriptActor,
        globals: &'a mut crate::script::GameplayGlobals,
        world: u32,
        id: ScriptActorId,
        position: Option<[i32; 3]>,
    ) -> Result<Self> {
        if let Some(error) = actor.failure() {
            anyhow::bail!("{error}");
        }
        Ok(Self {
            actor,
            globals,
            world,
            id,
            position,
        })
    }
    fn cleanup(
        &mut self,
        selected_player: ScriptActorId,
        nominal_ticks: u32,
        sounds: &mut dyn FnMut(ScriptActorId, crate::script::ScriptSound) -> Result<()>,
    ) -> Result<()> {
        let index = self.id.index;

        let actor = &mut *self.actor;
        let control = actor
            .control
            .as_mut()
            .context("weapon control unavailable")?;
        if control.weapon_drawn {
            anyhow::ensure!(
                control.combat.is_some(),
                "drawn weapon combat state unavailable"
            );
        }
        if self.id == selected_player
            && let Some(animation) = actor
                .animation
                .as_mut()
                .filter(|animation| matches!(animation.playback.current_group(), Some(91 | 129)))
        {
            let position = self.position;
            let surface = actor.surface;
            let default = animation.playback.default_group();
            let result = animation.playback.force_request_with_sounds(
                default,
                0,
                nominal_ticks,
                &mut |sound| {
                    sounds(
                        self.id,
                        crate::script::ScriptSound::Animation {
                            sound,
                            surface,
                            position,
                        },
                    )
                },
            );
            match result {
                Ok(frame) => animation.frame = frame.or(animation.frame),
                Err(error) => {
                    let message = format!(
                        "world {} instance {index} weapon cleanup animation: {error:#}",
                        self.world
                    );
                    actor.animation_failure = Some(message.clone());
                    anyhow::bail!(message);
                }
            }
        }
        if matches!(control.living_state, 20 | 21) {
            control.living_state = 0;
            control.motion_companion = 0;
        }
        if control.weapon_drawn {
            control.weapon_drawn = false;
            if self.id == selected_player {
                self.globals.request_candle_show();
            }
            control
                .combat
                .as_mut()
                .expect("validated drawn weapon combat")
                .flags &= !4;
        }
        Ok(())
    }
    fn draw(
        &mut self,
        inputs: WeaponInputs,
        nominal_ticks: u32,
        sounds: &mut dyn FnMut(ScriptActorId, crate::script::ScriptSound) -> Result<()>,
    ) -> Result<()> {
        let actor = &*self.actor;
        let control = actor
            .control
            .as_ref()
            .context("weapon draw control unavailable")?;
        let animation = actor.animation.as_ref();
        let (holding, drawing, sheathing) = crate::script::activation::weapon_predicates(
            control.living_state,
            animation.map(|animation| crate::script::ActivationAnimation {
                group: animation.playback.current_group(),
            }),
            control.combat.is_some(),
            control.weapon_drawn,
            actor.attributes[110],
            inputs.item_system_ready,
            inputs.shell_execution,
        );
        if (self.id == inputs.selected_player && actor.attributes[105] == 1)
            || !inputs.item_system_ready
            || animation.is_none()
            || inputs.shell_execution
            || control.rope_attached
            || (27..=32).contains(&control.living_state)
            || drawing
            || sheathing
            || holding
            || control.falling_state != 0
            || control.queued_motion != 0
            || control.living_state == 15
        {
            return Ok(());
        }
        if control.weapon_drawn || control.combat.is_none() {
            return self.cleanup(inputs.selected_player, nominal_ticks, sounds);
        }
        if (10..=19).contains(&control.living_state)
            || matches!(
                actor.surface.context("weapon draw surface unavailable")?,
                2 | 4
            )
        {
            return Ok(());
        }
        let control = self
            .actor
            .control
            .as_mut()
            .expect("admitted weapon control");
        control.living_state = 20;
        if self.id == inputs.selected_player {
            control
                .combat
                .as_mut()
                .expect("admitted assigned weapon")
                .flags |= 4;
        }
        Ok(())
    }
    pub(super) fn begin(
        &mut self,
        inputs: CombatEntryInputs,
        random: Option<&mut crate::random::EngineRandom>,
        source: Option<&dyn crate::script::CombatTauntSource>,
        sounds: &mut dyn FnMut(ScriptActorId, crate::script::ScriptSound) -> Result<()>,
    ) -> Result<()> {
        let control = self
            .actor
            .control
            .as_mut()
            .context("combat entry control unavailable")?;
        if self.id != inputs.weapon.selected_player {
            control.script_control |= 2;
        }
        if control.death_selector == 6 {
            return Ok(());
        }
        control.death_selector = 6;
        if self.actor.attributes[23] != 0 {
            if self
                .actor
                .animation
                .as_ref()
                .is_some_and(|animation| animation.playback.has_group(91))
            {
                if !self
                    .actor
                    .control
                    .as_ref()
                    .expect("combat control")
                    .weapon_drawn
                {
                    self.draw(inputs.weapon, inputs.nominal_ticks, sounds)?;
                    let position = self.position;
                    let actor = &mut *self.actor;
                    let surface = actor.surface;
                    let animation = actor.animation.as_mut().expect("group91 animation");
                    let frame = animation
                        .playback
                        .force_request_with_sounds(91, 0, inputs.nominal_ticks, &mut |sound| {
                            sounds(
                                self.id,
                                crate::script::ScriptSound::Animation {
                                    sound,
                                    surface,
                                    position,
                                },
                            )
                        })
                        .map_err(|error| {
                            let message = format!(
                                "world {} instance {} combat entry animation: {error:#}",
                                self.world, self.id.index
                            );
                            actor.animation_failure = Some(message.clone());
                            anyhow::anyhow!(message)
                        })?;
                    animation.frame = frame.or(animation.frame);
                }
            } else {
                self.cleanup(inputs.weapon.selected_player, inputs.nominal_ticks, sounds)?;
                let control = self.actor.control.as_mut().expect("combat control");
                control.weapon_drawn = true;
                control.living_state = 0;
            }
        } else {
            let control = self.actor.control.as_mut().expect("combat control");
            control.weapon_drawn = true;
            control.living_state = 0;
        }
        self.actor
            .control
            .as_mut()
            .expect("combat control")
            .combat
            .as_mut()
            .context("combat entry assigned state unavailable")?
            .flags |= 4;
        if !(self.id == inputs.weapon.selected_player && inputs.selected_player_death_override)
            && self
                .actor
                .combat_taunts
                .iter()
                .any(|taunt| taunt.trigger == 0 && taunt.label != [0; 4])
        {
            let random = random.context("combat entry shared RNG unavailable")?;
            if let Some(taunt) = super::select_combat_taunt(self.actor, 0, random).cloned() {
                source
                    .context("combat entry prepared taunt source unavailable")?
                    .with_playback(&mut |playback| {
                        super::dialogue::present_taunt(
                            self.actor,
                            self.globals,
                            self.world,
                            self.id,
                            self.position,
                            &taunt,
                            playback,
                            inputs.nominal_ticks,
                            inputs.taunt_animation_admitted,
                            sounds,
                        )
                        .map(|_| ())
                    })?;
            }
        }

        Ok(())
    }
}

impl ScriptWorld {
    fn combat_receiver(
        &mut self,
        id: ScriptActorId,
        player: ScriptActorId,
    ) -> Result<CombatReceiver<'_>> {
        let index = self.resolve(id)?;
        self.resolve(player)?;
        let position = super::animation_sound_position(self.spatial.as_ref(), index);
        CombatReceiver::new(
            &mut self.actors[index],
            &mut self.globals,
            self.world,
            id,
            position,
        )
    }

    /// Clear an admitted receiver's weapon and motion state. Selected draw/sheath
    /// playback restores its stored default, with immediate commands at nominal ticks.
    pub fn cleanup_actor_weapon(
        &mut self,
        id: ScriptActorId,
        selected_player: ScriptActorId,
        nominal_ticks: u32,
        sounds: &mut dyn FnMut(ScriptActorId, crate::script::ScriptSound) -> Result<()>,
    ) -> Result<()> {
        let world = self.world;
        self.combat_receiver(id, selected_player)?
            .cleanup(selected_player, nominal_ticks, sounds)
            .with_context(|| format!("world {world} instance {} weapon cleanup", id.index))
    }

    /// Apply draw admission without setting the drawn flag or requesting a group.
    /// The caller supplies established item-system and shell state.
    pub fn admit_actor_weapon_draw(
        &mut self,
        id: ScriptActorId,
        inputs: WeaponInputs,
        nominal_ticks: u32,
        sounds: &mut dyn FnMut(ScriptActorId, crate::script::ScriptSound) -> Result<()>,
    ) -> Result<()> {
        let world = self.world;
        self.combat_receiver(id, inputs.selected_player)?
            .draw(inputs, nominal_ticks, sounds)
            .with_context(|| format!("world {world} instance {} weapon draw admission", id.index))
    }

    /// Enter lifecycle six with synchronous weapon setup and type-zero taunt
    /// presentation. No portable numeric result is established for this effect.
    pub fn begin_actor_combat(
        &mut self,
        id: ScriptActorId,
        inputs: CombatEntryInputs,
        random: &mut crate::random::EngineRandom,
        playback: &mut dyn crate::script::CombatTauntPlayback,
        sounds: &mut dyn FnMut(ScriptActorId, crate::script::ScriptSound) -> Result<()>,
    ) -> Result<()> {
        self.resolve(id)?;
        self.resolve(inputs.weapon.selected_player)?;
        let source = PreparedTaunt(std::cell::RefCell::new(playback));
        let result = self
            .combat_receiver(id, inputs.weapon.selected_player)?
            .begin(inputs, Some(random), Some(&source), sounds);
        result.map_err(|error| {
            let message = format!(
                "world {} instance {} combat entry: {error:#}",
                self.world, id.index
            );
            self.actors[id.index].update_failure = Some(message.clone());
            anyhow::anyhow!(message)
        })
    }
}

struct PreparedTaunt<'a>(std::cell::RefCell<&'a mut dyn crate::script::CombatTauntPlayback>);
impl std::fmt::Debug for PreparedTaunt<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PreparedTaunt")
    }
}
impl crate::script::CombatTauntSource for PreparedTaunt<'_> {
    fn with_playback(
        &self,
        update: &mut dyn FnMut(&mut dyn crate::script::CombatTauntPlayback) -> Result<()>,
    ) -> Result<()> {
        let mut playback = self
            .0
            .try_borrow_mut()
            .context("combat taunt playback already borrowed")?;
        update(&mut **playback)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::script::registry::{
        ActorControlState, ScriptActor, ScriptActorSource, ScriptAnimation,
    };

    fn actor() -> ScriptActor {
        let data = cyrus_assets::animation::Animations {
            version: 2,
            world: 7,
            metadata: serde_json::json!({"actors":[{"script_name":"CYRUS",
                "animation_groups":[{"group_index":0,"anim_id":1,"anim_type":0,
                "commands":[{"opcode":0,"frame_reference":10},{"opcode":2,"value":0}]}]}]}),
            sections: Default::default(),
            models: Default::default(),
            unsupported: Default::default(),
        };
        ScriptActor {
            message_read: 0,
            source: ScriptActorSource::Player,
            runtime_identifier: None,
            namespace: None,
            native_hook: None,
            combat_taunts: vec![],
            runtime: None,
            unavailable: None,
            attributes: vec![0; 256],
            surface: Some(0),
            control: Some(ActorControlState {
                combat: Some(Default::default()),
                ..Default::default()
            }),
            model: Default::default(),
            collision: None,
            location_offsets: vec![],
            tasks: Default::default(),
            animation: Some(ScriptAnimation {
                playback: cyrus_world::animation::ActorAnimation::from_asset(&data, "CYRUS", 0)
                    .unwrap(),
                frame: None,
            }),
            animation_failure: None,
            update_failure: None,
        }
    }

    #[test]
    fn combat_weapon_draw_checks_falling_and_queued_state_and_cleanup_preserves_direction() {
        let id = ScriptActorId {
            generation: 1,
            index: 0,
        };
        for (falling, sliding, queued, expected) in
            [(0, 0, 0, 20), (1, 0, 0, 0), (0, 1, 0, 20), (0, 0, 1, 0)]
        {
            let mut actor = actor();
            let control = actor.control.as_mut().unwrap();
            control.falling_state = falling;
            control.sliding_state = sliding;
            control.queued_motion = queued;
            control.motion_direction = 3;
            control.motion_companion = 9;
            let mut globals = Default::default();
            let mut receiver = CombatReceiver::new(&mut actor, &mut globals, 7, id, None).unwrap();
            receiver
                .draw(
                    WeaponInputs {
                        selected_player: id,
                        item_system_ready: true,
                        shell_execution: false,
                    },
                    0,
                    &mut |_, _| Ok(()),
                )
                .unwrap();
            assert_eq!(
                receiver.actor.control.as_ref().unwrap().living_state,
                expected
            );
            receiver.cleanup(id, 0, &mut |_, _| Ok(())).unwrap();
            let control = receiver.actor.control.as_ref().unwrap();
            assert_eq!(control.motion_direction, 3);
            assert_eq!(control.motion_companion, if expected == 20 { 0 } else { 9 });
        }
    }
}
