//! Authoritative combat entry and weapon transitions follow preservation's engine/animation.md.
use super::{ScriptActorId, ScriptWorld};
use crate::script::AiType;
use crate::script::CombatFlags;
use crate::script::JumpMotion;
use crate::script::LivingState;
use anyhow::{Context, Result};
use cyrus_script::{AnimationGroup, Attribute};

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

impl super::ScriptActor {
    pub(super) fn hand_item(
        &mut self,
        item: i32,
        hud: &crate::script::ScriptHud,
        shell_execution: Option<bool>,
    ) -> Result<()> {
        let surface = self
            .surface
            .context("HandItem receiver surface unavailable")?;
        let control = self
            .control
            .as_mut()
            .context("HandItem receiver control unavailable")?;
        if matches!(surface, 2 | 4)
            || control.living_state.is_ledge()
            || control.living_state.is_actor_bounce()
            || control.falling_state != JumpMotion::NONE
            || control.sliding_state != 0
            || control.queued_motion != JumpMotion::NONE
            || self.attributes[Attribute::HoldingWeapon] != 0
        {
            return Ok(());
        }
        let shell = shell_execution.context("HandItem shell admission unavailable")?;
        if shell
            || super::super::activation::weapon_predicates(
                control.living_state,
                self.animation
                    .as_ref()
                    .map(|animation| crate::script::ActivationAnimation {
                        group: animation.playback.current_group(),
                    }),
                control.combat_death_default.is_some(),
                control.weapon_drawn,
                self.attributes[Attribute::HoldingWeapon],
                true,
                shell,
            )
            .0
        {
            return Ok(());
        }
        let item = u16::try_from(item).context("HandItem item ID outside u16 range")?;
        hud.validate_item(item)?;
        control.held_item = Some(item as u8);
        Ok(())
    }
}

/// A synchronous borrow of one authoritative logical actor and its shared globals.
pub(super) struct CombatReceiver<'a> {
    actor: &'a mut super::ScriptActor,
    globals: &'a mut crate::script::GameplayGlobals,
    world: u32,
    id: ScriptActorId,
    position: Option<[i32; 3]>,
    events: Option<super::effects::ActorEventDispatch<'a>>,
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
            events: None,
        })
    }
    pub(super) fn with_events(mut self, events: super::effects::ActorEventDispatch<'a>) -> Self {
        self.events = Some(events);
        self
    }
    /// An actor that always holds its weapon (`attr_holding_weapon`) keeps it drawn while `attr_weapon`
    /// names a weapon item, and drawing or sheathing never runs; without such an item the
    /// holding attributes are cleared. Without combat state the weapon is not drawn.
    fn hold_intrinsic_weapon(&mut self, hud: &crate::script::ScriptHud) {
        let actor = &mut *self.actor;
        let weapon = actor.attributes[Attribute::Weapon];
        let valid = weapon != 0 && hud.validate_weapon(weapon).is_ok();
        if !valid {
            actor.attributes[Attribute::HoldingWeapon] = 0;
            actor.attributes[Attribute::Weapon] = 0;
        }
        let control = actor
            .control
            .as_mut()
            .expect("validated weapon transition control");
        control.weapon_drawn = valid && control.combat.is_some();
        if valid
            && matches!(
                control.living_state,
                LivingState::DRAWING_SWORD | LivingState::SHEATHING_SWORD
            )
        {
            control.living_state = LivingState::ORDINARY;
        }
    }

    fn update_weapon_transition(
        &mut self,
        inputs: WeaponInputs,
        hud: &crate::script::ScriptHud,
        nominal_ticks: u32,
        sounds: &mut dyn FnMut(ScriptActorId, crate::script::ScriptSound) -> Result<()>,
    ) -> Result<()> {
        let actor = &*self.actor;
        if !inputs.item_system_ready || inputs.shell_execution || actor.animation.is_none() {
            return Ok(());
        }
        let control = actor
            .control
            .as_ref()
            .context("weapon transition control unavailable")?;
        if control.combat.is_none() && actor.attributes[Attribute::HoldingWeapon] == 0 {
            return Ok(());
        }
        if actor.attributes[Attribute::HoldingWeapon] != 0 {
            self.hold_intrinsic_weapon(hud);
        }
        let actor = &*self.actor;
        let control = actor.control.as_ref().expect("validated weapon control");
        let weapon = actor.attributes[Attribute::Weapon];
        hud.validate_weapon(weapon)?;
        let state = control.living_state;
        let requested = match state {
            LivingState::DRAWING_SWORD => Some(AnimationGroup::DRAW_SWORD),
            LivingState::SHEATHING_SWORD
                if actor.animation.as_ref().unwrap().playback.current_group()
                    != Some(AnimationGroup::SHEATH_SWORD) =>
            {
                Some(AnimationGroup::SHEATH_SWORD)
            }
            _ => None,
        };
        let position = self.position;
        let actor = &mut *self.actor;
        let surface = actor.surface;
        let animation = actor
            .animation
            .as_mut()
            .expect("validated weapon animation");
        if let Some(group) = requested {
            let frame =
                animation
                    .playback
                    .request_with_sounds(group, 0, nominal_ticks, &mut |sound| {
                        if let cyrus_world::animation::AnimationSound::Event { flags } = sound {
                            return self
                                .events
                                .as_mut()
                                .context("combat actor-event services unavailable")?
                                .dispatch(flags);
                        }
                        sounds(
                            self.id,
                            crate::script::ScriptSound::Animation {
                                sound,
                                surface,
                                position,
                            },
                        )
                    })?;
            animation.frame = frame.or(animation.frame);
            // A newly requested sheath group is tested on the next admitted pass.
            if state == LivingState::SHEATHING_SWORD {
                return Ok(());
            }
        }
        let (group, pending, progress) = animation.playback.frame_wait_state()?;
        let control = actor.control.as_mut().expect("validated weapon control");
        if state == LivingState::DRAWING_SWORD
            && group == i32::from(AnimationGroup::DRAW_SWORD)
            && progress >= actor.attributes[Attribute::DrawFrame]
        {
            control.weapon_drawn = true;
            control.living_state = LivingState::ORDINARY;
        } else if state == LivingState::SHEATHING_SWORD
            && group == i32::from(AnimationGroup::SHEATH_SWORD)
            && progress >= actor.attributes[Attribute::SheathFrame]
        {
            control.weapon_drawn = false;
            control.living_state = LivingState::ORDINARY;
            if pending == 129 {
                animation
                    .playback
                    .cancel_pending_group(AnimationGroup::SHEATH_SWORD)?;
            }
        }
        Ok(())
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
            && let Some(animation) = actor.animation.as_mut().filter(|animation| {
                matches!(
                    animation.playback.current_group(),
                    Some(AnimationGroup::DRAW_SWORD | AnimationGroup::SHEATH_SWORD)
                )
            })
        {
            let position = self.position;
            let surface = actor.surface;
            let default = animation.playback.default_group();
            let result = animation.playback.force_request_with_sounds(
                default,
                0,
                nominal_ticks,
                &mut |sound| {
                    if let cyrus_world::animation::AnimationSound::Event { flags } = sound {
                        return self
                            .events
                            .as_mut()
                            .context("combat actor-event services unavailable")?
                            .dispatch(flags);
                    }
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
        if matches!(
            control.living_state,
            LivingState::DRAWING_SWORD | LivingState::SHEATHING_SWORD
        ) {
            control.living_state = LivingState::ORDINARY;
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
                .flags
                .remove(CombatFlags::STANCE);
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
            actor.attributes[Attribute::HoldingWeapon],
            inputs.item_system_ready,
            inputs.shell_execution,
        );
        if (self.id == inputs.selected_player && actor.attributes[Attribute::PlayerType] == 1)
            || !inputs.item_system_ready
            || animation.is_none()
            || inputs.shell_execution
            || control.rope_attached
            || control.living_state.is_bounce()
            || drawing
            || sheathing
            || holding
            || control.falling_state != JumpMotion::NONE
            || control.queued_motion != JumpMotion::NONE
            || control.living_state == LivingState::HANGING
        {
            return Ok(());
        }
        if control.held_item.is_some_and(|item| item != 0) || control.combat.is_none() {
            return self.cleanup(inputs.selected_player, nominal_ticks, sounds);
        }
        if control.living_state.is_ledge()
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
        control.living_state = LivingState::DRAWING_SWORD;
        if self.id == inputs.selected_player {
            control
                .combat
                .as_mut()
                .expect("admitted assigned weapon")
                .flags
                .insert(CombatFlags::STANCE);
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
        if control.ai_type == AiType::COMBAT {
            return Ok(());
        }
        control.ai_type = AiType::COMBAT;
        if self.actor.attributes[Attribute::Weapon] != 0 {
            if self
                .actor
                .animation
                .as_ref()
                .is_some_and(|animation| animation.playback.has_group(AnimationGroup::DRAW_SWORD))
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
                        .force_request_with_sounds(
                            AnimationGroup::DRAW_SWORD,
                            0,
                            inputs.nominal_ticks,
                            &mut |sound| {
                                if let cyrus_world::animation::AnimationSound::Event { flags } =
                                    sound
                                {
                                    return self
                                        .events
                                        .as_mut()
                                        .context("combat actor-event services unavailable")?
                                        .dispatch(flags);
                                }
                                sounds(
                                    self.id,
                                    crate::script::ScriptSound::Animation {
                                        sound,
                                        surface,
                                        position,
                                    },
                                )
                            },
                        )
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
                control.living_state = LivingState::ORDINARY;
            }
        } else {
            let control = self.actor.control.as_mut().expect("combat control");
            control.weapon_drawn = true;
            control.living_state = LivingState::ORDINARY;
        }
        self.actor
            .control
            .as_mut()
            .expect("combat control")
            .combat
            .as_mut()
            .context("combat entry assigned state unavailable")?
            .flags
            .insert(CombatFlags::STANCE);
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
                        let events = &mut self.events;
                        let mut emit = |actor, sound| {
                            if let crate::script::ScriptSound::Animation {
                                sound: cyrus_world::animation::AnimationSound::Event { flags },
                                ..
                            } = sound
                            {
                                return events
                                    .as_mut()
                                    .context("combat taunt actor-event services unavailable")?
                                    .dispatch(flags);
                            }
                            sounds(actor, sound)
                        };
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
                            &mut emit,
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
        let events = super::effects::ActorEventDispatch {
            context: super::effects::ActorEventContext::capture(self, id),
            camera: self.camera.as_mut(),
            effects: &mut self.effects,
        };
        CombatReceiver::new(
            &mut self.actors[index],
            &mut self.globals,
            self.world,
            id,
            position,
        )
        .map(|receiver| receiver.with_events(events))
    }

    /// Validate the selected player's assigned item against current shared ownership.
    /// Called before ordinary traversal by an owner with established player state.
    /// Closed inventory leaves the actor untouched; a failed validity check clears
    /// the weapon attribute and applies the shared cleanup, without item acquisition.
    pub fn check_player_weapon(
        &mut self,
        selected_player: ScriptActorId,
        hud: &crate::script::ScriptHud,
        shell_execution: bool,
        nominal_ticks: u32,
        sounds: &mut dyn FnMut(ScriptActorId, crate::script::ScriptSound) -> Result<()>,
    ) -> Result<()> {
        if !hud.inventory_ready()? {
            return Ok(());
        }
        let index = self.resolve(selected_player)?;
        let actor = &self.actors[index];
        let weapon = actor.attributes[Attribute::Weapon];
        if actor.animation.is_none()
            || shell_execution
            || weapon == 0
            || !hud.owned_weapon(weapon)?
        {
            self.actors[index].attributes[Attribute::Weapon] = 0;
            self.actors[index].model.weapon = None;
            self.cleanup_actor_weapon(selected_player, selected_player, nominal_ticks, sounds)?;
        }
        Ok(())
    }

    /// Settle draw/sheath state in the admitted pre-traversal model pass.
    /// Thresholds are installed `attr_draw_frame`/`attr_sheath_frame` and compare the wrapping animation
    /// progress byte, independently of model frames and command cursors. This logical
    /// stage does not provide weapon attachment geometry or hit positions.
    /// Follows preservation's [equipped-weapon transition](https://github.com/michidk/redguard-preservation/commit/4221864).
    pub fn update_actor_weapon_transition(
        &mut self,
        id: ScriptActorId,
        inputs: WeaponInputs,
        hud: &crate::script::ScriptHud,
        nominal_ticks: u32,
        sounds: &mut dyn FnMut(ScriptActorId, crate::script::ScriptSound) -> Result<()>,
    ) -> Result<()> {
        let world = self.world;
        self.combat_receiver(id, inputs.selected_player)?
            .update_weapon_transition(inputs, hud, nominal_ticks, sounds)
            .with_context(|| {
                format!(
                    "world {world} instance {} equipped-weapon transition",
                    id.index
                )
            })
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
            "animation_groups":([0, 91, 129].map(|group| serde_json::json!({
                "group_index":group,"anim_id":1,"anim_type":0,
                "commands":[{"opcode":0,"frame_reference":10},{"opcode":0,"frame_reference":11},{"opcode":2,"value":0}]
            })))}]}),
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
            created_retired: false,
            unavailable: None,
            attributes: cyrus_script::Attributes::default(),
            surface: Some(0),
            control: Some(ActorControlState {
                combat: Some(Default::default()),
                ..Default::default()
            }),
            model: Default::default(),
            collision: None,
            player_movement: None,
            light: None,
            flat: None,
            location_offsets: vec![],
            tasks: Default::default(),
            native_commands: Default::default(),
            animation: Some(ScriptAnimation {
                playback: cyrus_world::animation::ActorAnimation::from_asset(
                    &data,
                    "CYRUS",
                    AnimationGroup::PANIC,
                )
                .unwrap(),
                frame: None,
            }),
            animation_failure: None,
            update_failure: None,
        }
    }

    fn weapon_hud() -> crate::script::ScriptHud {
        let data = cyrus_assets::hud::HudAsset {
            labels: Default::default(),
            images: vec![],
            candle_mode: 0,
            candle: [0; 2],
            candle_size: [0; 2],
            pickup: [0; 2],
            prompt: [0; 2],
            override_ticks: 0,
            compass: [0; 2],
            compass_size: [0; 2],
            world_compass: Default::default(),
            powerup_slots: [[0; 2]; 2],
            powerup_sizes: vec![],
            logbook: [0; 2],
            logbook_size: [0; 2],
            walk_mode: false,
            auto_defend: false,
            notices: Default::default(),
            inventory: [(
                1,
                cyrus_assets::hud::InventoryItem {
                    weapon: true,
                    ..Default::default()
                },
            )]
            .into(),
            start_items: vec![],
            items: vec![],
            selected: None,
            bindings: [0; 15],
            font: Default::default(),
            text_alpha: [0; 2],
            player_text_alpha: [0; 2],
            used_text_alpha: [0; 2],
            torch_time: None,
            inventory_screen: Default::default(),
            island_map: Default::default(),
        };
        let hud = crate::script::ScriptHud::default();
        hud.configure(&data, &crate::preferences::Preferences::default());
        hud
    }

    #[test]
    fn equipped_weapon_transitions_use_progress_threshold_and_preserve_companion() {
        let hud = weapon_hud();
        let id = ScriptActorId {
            generation: 1,
            index: 0,
        };
        for state in [LivingState::DRAWING_SWORD, LivingState::SHEATHING_SWORD] {
            for threshold in [0, 1, 2, 255] {
                let mut actor = actor();
                actor.attributes[Attribute::Weapon] = 1;
                actor.attributes[Attribute::SheathFrame] = threshold;
                actor.attributes[Attribute::DrawFrame] = threshold;
                let group = if state == LivingState::DRAWING_SWORD {
                    91
                } else {
                    129
                };
                let animation = actor.animation.as_mut().unwrap();
                animation
                    .playback
                    .force_request(AnimationGroup::from_index(group), 0, 0)
                    .unwrap();
                for _ in 0..256 {
                    if animation.playback.frame_wait_state().unwrap().2 == 1 {
                        break;
                    }
                    animation.playback.advance(1).unwrap();
                }
                assert_eq!(animation.playback.frame_wait_state().unwrap().2, 1);
                animation
                    .playback
                    .request(AnimationGroup::from_index(group), 0, 0)
                    .unwrap();
                let control = actor.control.as_mut().unwrap();
                control.living_state = state;
                control.motion_companion = 7;
                control.weapon_drawn = state == LivingState::SHEATHING_SWORD;
                let mut globals = Default::default();
                CombatReceiver::new(&mut actor, &mut globals, 7, id, None)
                    .unwrap()
                    .update_weapon_transition(
                        WeaponInputs {
                            selected_player: id,
                            item_system_ready: true,
                            shell_execution: false,
                        },
                        &hud,
                        0,
                        &mut |_, _| anyhow::bail!("unexpected transition sound"),
                    )
                    .unwrap();
                let control = actor.control.as_ref().unwrap();
                assert_eq!(
                    control.living_state,
                    if threshold <= 1 {
                        LivingState::ORDINARY
                    } else {
                        state
                    }
                );
                assert_eq!(
                    control.weapon_drawn,
                    if threshold <= 1 {
                        state == LivingState::DRAWING_SWORD
                    } else {
                        state == LivingState::SHEATHING_SWORD
                    }
                );
                assert_eq!(control.motion_companion, 7);
                let playback = &actor.animation.as_ref().unwrap().playback;
                assert_eq!(
                    playback.current_group(),
                    Some(AnimationGroup::from_index(group))
                );
                assert_eq!(playback.frame_wait_state().unwrap().2, 1);
                assert_eq!(
                    playback.pending_group(),
                    if state == LivingState::SHEATHING_SWORD && threshold <= 1 {
                        None
                    } else {
                        Some(AnimationGroup::from_index(group))
                    }
                );
            }
        }
    }

    #[test]
    fn newly_requested_sheath_waits_for_the_next_model_pass() {
        let hud = weapon_hud();
        let id = ScriptActorId {
            generation: 1,
            index: 0,
        };
        let mut actor = actor();
        actor.attributes[Attribute::Weapon] = 1;
        actor.attributes[Attribute::SheathFrame] = 0;
        let control = actor.control.as_mut().unwrap();
        control.living_state = LivingState::SHEATHING_SWORD;
        control.weapon_drawn = true;
        control.motion_companion = 7;
        let mut globals = Default::default();
        let mut receiver = CombatReceiver::new(&mut actor, &mut globals, 7, id, None).unwrap();
        let inputs = WeaponInputs {
            selected_player: id,
            item_system_ready: true,
            shell_execution: false,
        };
        receiver
            .update_weapon_transition(inputs, &hud, 0, &mut |_, _| Ok(()))
            .unwrap();
        let control = receiver.actor.control.as_ref().unwrap();
        assert_eq!(
            (
                control.living_state,
                control.weapon_drawn,
                control.motion_companion
            ),
            (LivingState::SHEATHING_SWORD, true, 7)
        );
        assert_eq!(
            receiver
                .actor
                .animation
                .as_ref()
                .unwrap()
                .playback
                .current_group(),
            Some(AnimationGroup::SHEATH_SWORD)
        );
        receiver
            .update_weapon_transition(inputs, &hud, 0, &mut |_, _| Ok(()))
            .unwrap();
        let control = receiver.actor.control.as_ref().unwrap();
        assert_eq!(
            (
                control.living_state,
                control.weapon_drawn,
                control.motion_companion
            ),
            (LivingState::ORDINARY, false, 7)
        );
    }

    #[test]
    fn an_actor_that_always_holds_its_weapon_keeps_it_drawn() {
        let hud = weapon_hud();
        let id = ScriptActorId {
            generation: 1,
            index: 0,
        };
        let inputs = WeaponInputs {
            selected_player: id,
            item_system_ready: true,
            shell_execution: false,
        };
        // (`attr_weapon`, combat state, living state) -> (drawn, living state, `attr_holding_weapon`, `attr_weapon`)
        for (weapon, combat, living, expected) in [
            (
                1,
                true,
                LivingState::ORDINARY,
                (true, LivingState::ORDINARY, 1, 1),
            ),
            (
                1,
                true,
                LivingState::DRAWING_SWORD,
                (true, LivingState::ORDINARY, 1, 1),
            ),
            (
                1,
                true,
                LivingState::SHEATHING_SWORD,
                (true, LivingState::ORDINARY, 1, 1),
            ),
            (
                1,
                false,
                LivingState::ORDINARY,
                (false, LivingState::ORDINARY, 1, 1),
            ),
        ] {
            let mut actor = actor();
            actor.attributes[Attribute::HoldingWeapon] = 1;
            actor.attributes[Attribute::Weapon] = weapon;
            let control = actor.control.as_mut().unwrap();
            control.living_state = living;
            if !combat {
                control.combat = None;
            }
            let mut globals = Default::default();
            let mut receiver = CombatReceiver::new(&mut actor, &mut globals, 7, id, None).unwrap();
            receiver
                .update_weapon_transition(inputs, &hud, 0, &mut |_, _| Ok(()))
                .unwrap();
            let actor = &*receiver.actor;
            let control = actor.control.as_ref().unwrap();
            assert_eq!(
                (
                    control.weapon_drawn,
                    control.living_state,
                    actor.attributes[Attribute::HoldingWeapon],
                    actor.attributes[Attribute::Weapon]
                ),
                expected
            );
        }
        // A missing weapon item clears the holding attributes and the transition then fails.
        let mut actor = actor();
        actor.attributes[Attribute::HoldingWeapon] = 1;
        actor.attributes[Attribute::Weapon] = 0;
        let mut globals = Default::default();
        let mut receiver = CombatReceiver::new(&mut actor, &mut globals, 7, id, None).unwrap();
        assert!(
            receiver
                .update_weapon_transition(inputs, &hud, 0, &mut |_, _| Ok(()))
                .is_err()
        );
        assert_eq!(
            (
                receiver.actor.attributes[Attribute::HoldingWeapon],
                receiver.actor.attributes[Attribute::Weapon]
            ),
            (0, 0)
        );
    }

    #[test]
    fn weapon_draw_uses_held_item_independently_of_drawn_flag() {
        let id = ScriptActorId {
            generation: 1,
            index: 0,
        };
        for held_item in [None, Some(0), Some(7)] {
            let mut actor = actor();
            actor.attributes[Attribute::Weapon] = 1;
            actor.control.as_mut().unwrap().held_item = held_item;
            let mut globals = Default::default();
            CombatReceiver::new(&mut actor, &mut globals, 7, id, None)
                .unwrap()
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
                actor.control.as_ref().unwrap().living_state,
                if held_item == Some(7) {
                    LivingState::ORDINARY
                } else {
                    LivingState::DRAWING_SWORD
                }
            );
            assert!(!actor.control.as_ref().unwrap().weapon_drawn);
        }
    }
    #[test]
    fn hand_item_rejects_edges_air_and_drawn_weapons_without_resolving_item() {
        for gate in 0..8 {
            let mut actor = actor();
            let control = actor.control.as_mut().unwrap();
            control.held_item = Some(7);
            match gate {
                0 => control.living_state = LivingState::TURNING_TO_HIGH_LEDGE,
                1 => control.living_state = LivingState::ENTERING_LEDGE,
                2 => control.living_state = LivingState::LANDED_ON_ACTOR,
                3 => control.living_state = LivingState::FALLING_FROM_ACTOR,
                4 => control.falling_state = JumpMotion::STANDING,
                5 => control.sliding_state = 1,
                6 => control.queued_motion = JumpMotion::STANDING,
                7 => {
                    control.weapon_drawn = true;
                    control.combat_death_default = Some(1);
                }
                _ => unreachable!(),
            }
            actor
                .hand_item(-1, &crate::script::ScriptHud::default(), Some(false))
                .unwrap();
            assert_eq!(
                actor.control.as_ref().unwrap().held_item,
                Some(7),
                "gate {gate}"
            );
        }
    }

    #[test]
    fn combat_weapon_draw_checks_falling_and_queued_state_and_cleanup_preserves_direction() {
        let id = ScriptActorId {
            generation: 1,
            index: 0,
        };
        for (falling, sliding, queued, expected) in [
            (
                JumpMotion::NONE,
                0,
                JumpMotion::NONE,
                LivingState::DRAWING_SWORD,
            ),
            (
                JumpMotion::STANDING,
                0,
                JumpMotion::NONE,
                LivingState::ORDINARY,
            ),
            (
                JumpMotion::NONE,
                1,
                JumpMotion::NONE,
                LivingState::DRAWING_SWORD,
            ),
            (
                JumpMotion::NONE,
                0,
                JumpMotion::STANDING,
                LivingState::ORDINARY,
            ),
        ] {
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
            assert_eq!(
                control.motion_companion,
                if expected == LivingState::DRAWING_SWORD {
                    0
                } else {
                    9
                }
            );
        }
    }
}
