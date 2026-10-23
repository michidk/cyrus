//! Selected-player health services retain distinct publication and death ordering.
use super::{ScriptActorId, ScriptInputs, ScriptWorld};
use crate::script::{GameplayGlobals, ScriptSound};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

/// Session health initialized separately from controller/combat inhibition.
/// Follows preservation's distinct additive and death-owning health rules.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SelectedHealthState {
    maximum: i32,
    displayed: i32,
    hit_inhibited: bool,
    #[serde(default)]
    death: Option<SelectedDeathState>,
    #[serde(default)]
    falling_elapsed: Option<u32>,
}

/// Death state initialized by the selected-player initializer, independently of action timers.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SelectedDeathState {
    progress: u32,
    mode: i32,
    local_position: Option<[i32; 3]>,
}
impl SelectedDeathState {
    /// Elapsed nominal ticks, wrapping at 32 bits.
    pub fn progress(&self) -> u32 {
        self.progress
    }
    /// Retained death-effect selector, initialized to zero.
    pub fn mode(&self) -> i32 {
        self.mode
    }
    /// Local Q8 position captured after fatal health publication and before notification.
    pub fn local_position(&self) -> Option<[i32; 3]> {
        self.local_position
    }
}

impl SelectedHealthState {
    pub(in crate::script) fn initialized(health: u8) -> Self {
        Self {
            falling_elapsed: Some(0),
            maximum: 100,
            displayed: i32::from(health),
            hit_inhibited: false,
            death: Some(SelectedDeathState {
                progress: 0,
                mode: 0,
                local_position: None,
            }),
        }
    }

    pub(in crate::script) fn validate(&self) -> Result<()> {
        ensure!(
            (1..=255).contains(&self.maximum),
            "selected health maximum outside 1..=255"
        );
        ensure!(
            (0..=255).contains(&self.displayed),
            "selected displayed health outside byte range"
        );
        Ok(())
    }

    /// Numeric death state; absent in legacy snapshots that did not retain it.
    pub fn death(&self) -> Option<&SelectedDeathState> {
        self.death.as_ref()
    }
    /// Session maximum used by health clamping.
    pub fn maximum(&self) -> i32 {
        self.maximum
    }
    /// Last published health: after additive feedback, before death-owning feedback.
    pub fn displayed(&self) -> i32 {
        self.displayed
    }
    pub(super) fn retain_death_position(&mut self, position: Option<[i32; 3]>) {
        self.death
            .as_mut()
            .expect("initialized death")
            .local_position = position;
    }

    /// Independent world-loop falling timeout; absent in legacy snapshots.
    pub fn falling_elapsed(&self) -> Option<u32> {
        self.falling_elapsed
    }

    pub(in crate::script) fn initialize_death(&mut self) {
        self.falling_elapsed = Some(0);
        let death = self.death.get_or_insert(SelectedDeathState {
            progress: 0,
            mode: 0,
            local_position: None,
        });
        death.progress = 0;
        death.mode = 0;
    }

    pub(super) fn advance_falling_elapsed(&mut self, falling: bool, ticks: u32) -> Result<u32> {
        let elapsed = self
            .falling_elapsed
            .as_mut()
            .context("selected falling elapsed owner unavailable")?;
        *elapsed = if falling {
            elapsed.wrapping_add(ticks)
        } else {
            0
        };
        Ok(*elapsed)
    }

    pub(in crate::script) fn set_hit_inhibited(&mut self, inhibited: bool) {
        self.hit_inhibited = inhibited;
    }

    /// Separate hit-rejection flag; this is not combat-decision inhibition.
    pub fn hit_inhibited(&self) -> bool {
        self.hit_inhibited
    }
}

impl GameplayGlobals {
    /// Unknown before selected-player initialization or when absent from a restored snapshot.
    pub fn selected_health(&self) -> Option<&SelectedHealthState> {
        self.selected_health.as_ref()
    }
}

impl ScriptWorld {
    pub(super) fn set_model_death_mode_one(&mut self) -> Result<()> {
        self.globals
            .selected_health
            .as_mut()
            .and_then(|health| health.death.as_mut())
            .context("model selected death mode unavailable")?
            .mode = 1;
        Ok(())
    }

    /// Reset the selected receiver's ledge action after its damage animation starts.
    /// Support clearing precedes transition admission; blocked transitions retain
    /// state 17 and clear directional state. Follows preservation's SOUP reset rule.
    fn reset_selected_reaction_action(
        &mut self,
        id: ScriptActorId,
        inputs: ScriptInputs<'_>,
        sounds: &mut dyn FnMut(ScriptActorId, ScriptSound) -> Result<()>,
    ) -> Result<()> {
        let index = self.resolve(id)?;
        let living = self.actors[index]
            .control
            .as_ref()
            .context("selected action control unavailable")?
            .living_state;
        if living != 15 {
            return Ok(());
        }
        let inhibited = self
            .globals
            .selected_action_inhibition
            .context("selected action inhibition unavailable")?;
        if inhibited != 0 {
            return Ok(());
        }
        self.actors[index]
            .control
            .as_mut()
            .expect("validated control")
            .living_state = 17;
        self.actors[index].model.ledge_support = None;
        if self.actors[index].attributes[105] != 1 {
            self.globals.selected_ledge_result = Some(0);
            let torch_burning = self.actors[index]
                .control
                .as_ref()
                .expect("validated control")
                .torch_burning
                != 0;
            let shell = if !torch_burning && self.updates.animation_system {
                inputs
                    .shell_execution
                    .context("selected action shell context unavailable")?
            } else {
                true
            };
            let actor = &self.actors[index];
            let blocked = self.updates.animation_system
                && !shell
                && actor
                    .animation
                    .as_ref()
                    .and_then(|animation| animation.playback.current_group())
                    .is_some_and(|group| matches!(group, 149 | 150));
            if !torch_burning && !blocked {
                let ticks = inputs
                    .nominal_ticks
                    .context("selected action nominal ticks unavailable")?;
                let position = super::animation_sound_position(self.spatial.as_ref(), index);
                let event_context = super::effects::ActorEventContext::capture(self, id);
                let event_camera = &mut self.camera;
                let event_effects = &mut self.effects;
                let actor = &mut self.actors[index];
                let surface = actor.surface;
                let animation = actor
                    .animation
                    .as_mut()
                    .context("selected action animation unavailable")?;
                let frame =
                    animation
                        .playback
                        .force_request_with_sounds(56, 0, ticks, &mut |sound| {
                            if let cyrus_world::animation::AnimationSound::Event { flags } = sound {
                                return event_context.dispatch(
                                    flags,
                                    event_camera.as_mut(),
                                    event_effects,
                                );
                            }
                            sounds(
                                id,
                                ScriptSound::Animation {
                                    sound,
                                    surface,
                                    position,
                                },
                            )
                        })?;
                animation.frame = frame.or(animation.frame);
                let control = actor.control.as_mut().expect("validated control");
                control.action_transition_counter = 6;
                control.living_state = 0;
            }
        }
        let control = self.actors[index]
            .control
            .as_mut()
            .expect("validated control");
        if control.living_state != 0 {
            control.motion_direction = 0;
            control.directional_state = [0; 2];
        }
        Ok(())
    }

    /// Apply selected-player additive health in service order. Inhibition precedes
    /// HUD access. Timer and actor health writes survive later feedback errors;
    /// displayed health publishes only after taunt and reaction return.
    /// Ledge reset retains its own support and inhibition state. An attached rope
    /// is let go through the rope detach. Death is
    /// dispatched by the contact caller, never by this additive service.
    pub fn apply_selected_contact_health(
        &mut self,
        id: ScriptActorId,
        change: i32,
        inputs: ScriptInputs<'_>,
        random: &mut crate::random::EngineRandom,
        sounds: &mut dyn FnMut(ScriptActorId, ScriptSound) -> Result<()>,
    ) -> Result<()> {
        self.apply_selected_additive_health(id, change, inputs, Some(random), sounds)
    }

    /// Native inspection may omit providers. Damage requires randomness only
    /// after publishing actor health; healing and inhibition never query it.
    pub(super) fn apply_selected_additive_health(
        &mut self,
        id: ScriptActorId,
        change: i32,
        inputs: ScriptInputs<'_>,
        random: Option<&mut crate::random::EngineRandom>,
        sounds: &mut dyn FnMut(ScriptActorId, ScriptSound) -> Result<()>,
    ) -> Result<()> {
        let result = (|| {
            let index = self.resolve(id)?;
            let selected = if let Some(activation) = inputs.activation {
                Some(activation.player)
            } else {
                self.selected_player_index()
                    .map(|index| self.id(index))
                    .transpose()?
            };
            ensure!(
                selected == Some(id),
                "selected health receiver differs from selected player"
            );
            let state = self
                .globals
                .selected_health
                .as_ref()
                .context("selected health initialization unavailable")?;
            if state.hit_inhibited {
                return Ok(());
            }
            let maximum = state.maximum;
            inputs
                .hud
                .context("selected health HUD owner unavailable")?
                .request_health_feedback();
            let health = i32::from(self.actors[index].attributes[3])
                .wrapping_add(change)
                .clamp(0, maximum);
            self.actors[index].attributes[3] = health as u8;
            if change < 0 {
                let random = random.context("selected health random stream unavailable")?;
                self.request_hurt_combat_taunt(id, inputs, random, sounds)?;
                self.selected_health_reaction(id, inputs, random, sounds)?;
            }
            self.globals
                .selected_health
                .as_mut()
                .expect("validated selected health")
                .displayed = health;
            Ok(())
        })();
        result.with_context(|| {
            format!(
                "world {} instance {} selected additive health",
                self.world, id.index
            )
        })
    }
    fn selected_health_reaction(
        &mut self,
        id: ScriptActorId,
        inputs: ScriptInputs<'_>,
        random: &mut crate::random::EngineRandom,
        sounds: &mut dyn FnMut(ScriptActorId, ScriptSound) -> Result<()>,
    ) -> Result<()> {
        let index = self.resolve(id)?;
        let animation = self.actors[index]
            .animation
            .as_ref()
            .context("selected health reaction animation unavailable")?;
        let suppressed = [
            animation.playback.current_group(),
            animation.playback.pending_group(),
        ]
        .into_iter()
        .flatten()
        .any(|group| matches!(group, 73 | 74 | 117 | 118 | 132 | 133));
        if !suppressed {
            let odd = random.draw() & 1 != 0;
            let (_, holding) = self.spacing_weapon(id, inputs)?;
            let group = match (holding, odd) {
                (false, false) => 133,
                (false, true) => 132,
                (true, false) => 118,
                (true, true) => 117,
            };
            let ticks = inputs
                .nominal_ticks
                .context("selected reaction nominal ticks unavailable")?;
            let position = super::animation_sound_position(self.spatial.as_ref(), index);
            let event_context = super::effects::ActorEventContext::capture(self, id);
            let event_camera = &mut self.camera;
            let event_effects = &mut self.effects;
            let actor = &mut self.actors[index];
            let surface = actor.surface;
            let animation = actor
                .animation
                .as_mut()
                .expect("validated reaction animation");
            let frame =
                animation
                    .playback
                    .force_request_with_sounds(group, 0, ticks, &mut |sound| {
                        if let cyrus_world::animation::AnimationSound::Event { flags } = sound {
                            return event_context.dispatch(
                                flags,
                                event_camera.as_mut(),
                                event_effects,
                            );
                        }
                        sounds(
                            id,
                            ScriptSound::Animation {
                                sound,
                                surface,
                                position,
                            },
                        )
                    })?;
            animation.frame = frame.or(animation.frame);
            self.reset_selected_reaction_action(id, inputs, sounds)?;
            let attached = self.actors[index]
                .control
                .as_ref()
                .context("selected reaction control unavailable")?
                .rope_attached;
            if attached {
                self.detach_selected_from_rope(id, inputs)?;
            }
        }
        Ok(())
    }

    /// Handle the selected inhibited player's nonzero death override before ordinary
    /// default selection or inventory queries. Returns false when that branch is absent.
    pub fn select_selected_death_animation_default(
        &mut self,
        id: ScriptActorId,
        ticks: u32,
        sounds: &mut dyn FnMut(ScriptActorId, ScriptSound) -> Result<()>,
    ) -> Result<bool> {
        let index = self.resolve(id)?;
        if self.selected_player_index() != Some(index) {
            return Ok(false);
        }
        let result = (|| {
            let group = self
                .globals
                .player_death_override_group()
                .context("selected death override owner unavailable")?;
            if group == 0
                || !self
                    .globals
                    .player_hit_inhibited()
                    .context("selected death inhibition unavailable")?
            {
                return Ok(false);
            }
            if let Some(error) = self.actors[index].failure() {
                anyhow::bail!("{error}");
            }
            let group = u16::try_from(group)
                .context("selected death override outside animation group range")?;
            let position = super::animation_sound_position(self.spatial.as_ref(), index);
            let event_context = super::effects::ActorEventContext::capture(self, id);
            let event_camera = &mut self.camera;
            let event_effects = &mut self.effects;
            let actor = &mut self.actors[index];
            let surface = actor.surface;
            let animation = actor
                .animation
                .as_mut()
                .context("selected death animation unavailable")?;
            match animation
                .playback
                .select_death_override_with_sounds(group, ticks, &mut |sound| {
                    if let cyrus_world::animation::AnimationSound::Event { flags } = sound {
                        return event_context.dispatch(flags, event_camera.as_mut(), event_effects);
                    }
                    sounds(
                        id,
                        ScriptSound::Animation {
                            sound,
                            surface,
                            position,
                        },
                    )
                }) {
                Ok(frame) => {
                    animation.frame = frame.or(animation.frame);
                    Ok(true)
                }
                Err(error) => {
                    let message = format!(
                        "world {} instance {index} selected death default: {error:#}",
                        self.world
                    );
                    actor.animation_failure = Some(message.clone());
                    anyhow::bail!(message)
                }
            }
        })();
        result.with_context(|| {
            format!(
                "world {} instance {index} selected death default",
                self.world
            )
        })
    }

    /// Death admission bypasses idle and movement. The synchronous starter precedes
    /// progress accumulation; environmental spawning requires its own host.
    pub(super) fn advance_selected_death_controller(
        &mut self,
        id: ScriptActorId,
        inputs: ScriptInputs<'_>,
        ticks: u32,
        random: &mut crate::random::EngineRandom,
        sounds: &mut dyn FnMut(ScriptActorId, ScriptSound) -> Result<()>,
    ) -> Result<()> {
        let result = (|| {
            let index = self.resolve(id)?;
            ensure!(
                self.selected_player_index() == Some(index),
                "death controller receiver is not selected"
            );
            ensure!(
                self.globals
                    .player_hit_inhibited()
                    .context("selected death inhibition unavailable")?,
                "selected death controller requires hit inhibition"
            );
            let mut group = self
                .globals
                .player_death_override_group()
                .context("selected death override owner unavailable")?;
            if group == 0 {
                let (_, holding) = self.spacing_weapon(id, inputs)?;
                group = if holding { 122 } else { 134 };
                self.globals.set_player_death_override_group(group)?;
            }
            let position = super::animation_sound_position(self.spatial.as_ref(), index);
            let event_context = super::effects::ActorEventContext::capture(self, id);
            let event_camera = &mut self.camera;
            let event_effects = &mut self.effects;
            let actor = &mut self.actors[index];
            let surface = actor.surface;
            let animation = actor
                .animation
                .as_mut()
                .context("selected death animation unavailable")?;
            let frame = animation.playback.force_request_with_sounds(
                u16::try_from(group)
                    .context("selected death override outside animation group range")?,
                0,
                ticks,
                &mut |sound| {
                    if let cyrus_world::animation::AnimationSound::Event { flags } = sound {
                        return event_context.dispatch(flags, event_camera.as_mut(), event_effects);
                    }
                    sounds(
                        id,
                        ScriptSound::Animation {
                            sound,
                            surface,
                            position,
                        },
                    )
                },
            )?;
            animation.frame = frame.or(animation.frame);
            let death = self
                .globals
                .selected_health
                .as_mut()
                .context("selected health initialization unavailable")?
                .death
                .as_mut()
                .context("selected death progress owner unavailable")?;
            death.progress = death.progress.wrapping_add(ticks);
            let (mode, progress) = (death.mode, death.progress);
            match mode {
                1 => {
                    self.emit_selected_death_effect(id, 0xb081, 32, 4096, random)?;
                    let bound = (progress as i32 / 6).wrapping_add(1);
                    if random.rnd(bound)? == 0 {
                        self.emit_selected_death_effect(id, 0x22480, 8, 1024, random)?;
                    }
                    if random.rnd(bound)? == 0 {
                        self.emit_selected_death_effect(id, 0x2580, 8, 1024, random)?;
                    }
                    if progress as i32 > 12 {
                        self.emit_selected_death_effect(id, 0x32480, 8, 1024, random)?;
                    }
                }
                2 if (random.draw() as u32) & progress.wrapping_add(1) == 0 => {
                    let early = (progress as i32) < 12;
                    self.emit_selected_death_effect(
                        id,
                        if early { 0x2500 } else { 0x12500 },
                        8,
                        1024,
                        random,
                    )?;
                    let variation = random.rnd(64)? - 32;
                    sounds(
                        id,
                        ScriptSound::Positional {
                            effect: if early { 71 } else { 70 },
                            volume: 0,
                            rate_offset_hz: variation,
                            position: {
                                let spatial = self.spatial(id)?;
                                if spatial.parent.is_some() {
                                    spatial.query_pose()?.position
                                } else {
                                    spatial.linked_position.unwrap_or(spatial.local.position)
                                }
                            },
                        },
                    )?;
                }
                _ => {}
            }
            Ok(())
        })();
        result.with_context(|| {
            format!(
                "world {} instance {} selected death controller",
                self.world, id.index
            )
        })
    }

    fn emit_selected_death_effect(
        &mut self,
        id: ScriptActorId,
        descriptor: u32,
        bound: i32,
        offset: i32,
        random: &mut crate::random::EngineRandom,
    ) -> Result<()> {
        ensure!(
            self.effects.is_some(),
            "selected death environmental effect spawning unavailable"
        );
        let point = self
            .globals
            .selected_health
            .as_ref()
            .and_then(|health| health.death.as_ref())
            .and_then(|death| death.local_position)
            .context("selected death captured effect point unavailable")?;
        let camera = self
            .camera
            .as_ref()
            .context("selected death published camera unavailable")?;
        self.spatial(camera.camera_actor)
            .context("selected death camera pose not initialized")?;
        let camera = camera.current.position;
        let mut point = super::effects::death_effect_point(point, camera);
        point[0] = point[0]
            .wrapping_add(random.rnd(bound)?.wrapping_mul(256))
            .wrapping_sub(offset);
        point[2] = point[2]
            .wrapping_add(random.rnd(bound)?.wrapping_mul(256))
            .wrapping_sub(offset);
        let height = self.ordinary_collision_shape(id)?.positive_extent[1];
        point[1] = point[1]
            .wrapping_add(height.wrapping_mul(256))
            .wrapping_add(1536);
        self.spawn_environment_effect(descriptor, point)
    }

    /// Selected `AdjustHealth` routing, shared by native commands and model hazards.
    /// Early replenish feedback precedes inhibition; nonzero modes retain the native source.
    pub(super) fn apply_selected_health_command(
        &mut self,
        id: ScriptActorId,
        change: i32,
        mode: i32,
        inputs: ScriptInputs<'_>,
        random: Option<&mut crate::random::EngineRandom>,
        sounds: &mut dyn FnMut(ScriptActorId, ScriptSound) -> Result<()>,
    ) -> Result<()> {
        inputs
            .hud
            .context("AdjustHealth: selected-player HUD owner unavailable")?
            .request_replenish_health_feedback();
        if mode == 0 {
            self.apply_selected_additive_health(id, change, inputs, random, sounds)
        } else {
            self.apply_selected_death_health(
                id,
                change,
                Some(self.globals.native_source),
                inputs,
                random,
                sounds,
            )
        }
    }

    /// Death-owning health publishes displayed health before notifications. A missing
    /// source context is distinct from generic forwarding's known absent source.
    pub(super) fn apply_selected_death_health(
        &mut self,
        id: ScriptActorId,
        change: i32,
        source: Option<super::super::activation::NativeSource>,
        inputs: ScriptInputs<'_>,
        random: Option<&mut crate::random::EngineRandom>,
        sounds: &mut dyn FnMut(ScriptActorId, ScriptSound) -> Result<()>,
    ) -> Result<()> {
        let result = (|| {
            let index = self.resolve(id)?;
            ensure!(
                self.selected_player_index() == Some(index),
                "death health receiver is not selected"
            );
            let state = self
                .globals
                .selected_health
                .as_ref()
                .context("selected health initialization unavailable")?;
            if state.hit_inhibited {
                return Ok(());
            }
            let maximum = state.maximum;
            inputs
                .hud
                .context("selected death health HUD owner unavailable")?
                .request_health_feedback();
            let health = i32::from(self.actors[index].attributes[3])
                .wrapping_add(change)
                .clamp(0, maximum);
            if change < 0 {
                self.reset_selected_reaction_action(id, inputs, sounds)?;
                if self.actors[index]
                    .control
                    .as_ref()
                    .context("selected death control unavailable")?
                    .rope_attached
                {
                    self.detach_selected_from_rope(id, inputs)?;
                }
            }
            self.actors[index].attributes[3] = health as u8;
            self.globals
                .selected_health
                .as_mut()
                .expect("validated health")
                .displayed = health;
            if health == 0
                && !inputs
                    .death_suppressed
                    .context("selected death suppression input unavailable")?
            {
                self.request_combat_taunt(
                    id,
                    6,
                    0,
                    inputs,
                    random.context("selected death random stream unavailable")?,
                    sounds,
                )?;
                let state = self
                    .globals
                    .selected_health
                    .as_mut()
                    .expect("validated health");
                state.hit_inhibited = true;
                state
                    .death
                    .as_mut()
                    .context("selected death progress owner unavailable")?
                    .progress = 0;
                let position = self
                    .spatial
                    .as_ref()
                    .context("selected death spatial owner unavailable")?
                    .actor(index)?
                    .local
                    .position;
                self.globals
                    .selected_health
                    .as_mut()
                    .expect("validated health")
                    .death
                    .as_mut()
                    .expect("validated death")
                    .local_position = Some(position);
                self.actors[index]
                    .control
                    .as_mut()
                    .context("selected death control unavailable")?
                    .death_selector = 8;
                if self
                    .globals
                    .player_death_override_group()
                    .context("selected death override owner unavailable")?
                    != 0
                {
                    return Ok(());
                }
                let source = match source {
                    None => None,
                    Some(super::super::activation::NativeSource::Unknown) => {
                        anyhow::bail!("selected death native source unavailable")
                    }
                    Some(super::super::activation::NativeSource::Actor { generation, index }) => {
                        Some(ScriptActorId { generation, index })
                    }
                };
                if let Some(source) = source.filter(|source| *source != id) {
                    let source_index = self.resolve(source)?;
                    let group = self.actors[source_index].attributes[109];
                    if group != 0 {
                        self.globals
                            .set_player_death_override_group(i32::from(group))?;
                        return Ok(());
                    }
                }
                if matches!(
                    self.actors[index]
                        .surface
                        .context("selected death surface unavailable")?,
                    2 | 4
                ) {
                    self.globals
                        .selected_health
                        .as_mut()
                        .expect("validated health")
                        .death
                        .as_mut()
                        .expect("validated death")
                        .mode = 2;
                }
                if self.actors[index].attributes[105] == 1 {
                    return Ok(());
                }
                let actor = &self.actors[index];
                let control = actor
                    .control
                    .as_ref()
                    .context("selected death control unavailable")?;
                let animation = actor
                    .animation
                    .as_ref()
                    .context("selected death animation unavailable")?;
                if control.falling_state != 0
                    || matches!(
                        animation.playback.current_group(),
                        Some(114 | 116 | 130 | 131)
                    )
                {
                    self.globals.set_player_death_override_group(74)?;
                    return Ok(());
                }
                let (_, holding) = self.spacing_weapon(id, inputs)?;
                let group = if holding {
                    if self.actors[index]
                        .animation
                        .as_ref()
                        .expect("validated animation")
                        .playback
                        .has_group(122)
                    {
                        122
                    } else {
                        121
                    }
                } else {
                    134
                };
                let ticks = inputs
                    .nominal_ticks
                    .context("selected death nominal ticks unavailable")?;
                let position = super::animation_sound_position(self.spatial.as_ref(), index);
                let event_context = super::effects::ActorEventContext::capture(self, id);
                let event_camera = &mut self.camera;
                let event_effects = &mut self.effects;
                let actor = &mut self.actors[index];
                let surface = actor.surface;
                let animation = actor.animation.as_mut().expect("validated animation");
                let frame = animation.playback.force_request_with_sounds(
                    group,
                    0,
                    ticks,
                    &mut |sound| {
                        if let cyrus_world::animation::AnimationSound::Event { flags } = sound {
                            return event_context.dispatch(
                                flags,
                                event_camera.as_mut(),
                                event_effects,
                            );
                        }
                        sounds(
                            id,
                            ScriptSound::Animation {
                                sound,
                                surface,
                                position,
                            },
                        )
                    },
                )?;
                animation.frame = frame.or(animation.frame);
                if holding {
                    self.actors[index]
                        .control
                        .as_mut()
                        .expect("validated control")
                        .combat_death_default = Some(if group == 122 { 124 } else { 123 });
                    self.release_held_walk_nodes(id)?;
                }
            } else {
                self.globals.set_player_death_override_group(0)?;
                let state = self
                    .globals
                    .selected_health
                    .as_mut()
                    .expect("validated health");
                state.hit_inhibited = false;
                state
                    .death
                    .as_mut()
                    .context("selected death progress owner unavailable")?
                    .progress = 0;
                if change < 0 {
                    let random = random.context("selected death random stream unavailable")?;
                    self.request_hurt_combat_taunt(id, inputs, random, sounds)?;
                    self.selected_health_reaction(id, inputs, random, sounds)?;
                }
            }
            Ok(())
        })();
        result.with_context(|| {
            format!(
                "world {} instance {} selected death-owning health",
                self.world, id.index
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::SelectedHealthState;

    #[test]
    fn death_book_return_clears_only_inhibition_and_terminal_outcome() {
        let mut actors = crate::script::ScriptActors::default();
        actors.globals.selected_health = Some(SelectedHealthState::initialized(0));
        actors.globals.initialize_player_controller();
        actors.globals.set_player_death_override_group(74).unwrap();
        let health = actors.globals.selected_health.as_mut().unwrap();
        health.hit_inhibited = true;
        health.falling_elapsed = Some(145);
        health.death.as_mut().unwrap().progress = 99;
        actors.world_loop_outcome = Some(crate::script::WorldLoopOutcome::Death);
        actors.finish_death_book().unwrap();
        let health = actors.globals.selected_health().unwrap();
        assert!(!health.hit_inhibited());
        assert_eq!(health.falling_elapsed(), Some(145));
        assert_eq!(health.death().unwrap().progress(), 99);
        assert_eq!(actors.globals.player_death_override_group(), Some(74));
        assert!(actors.world_loop_outcome.is_none());
        actors.globals.selected_health = None;
        actors.world_loop_outcome = Some(crate::script::WorldLoopOutcome::Death);
        assert!(actors.finish_death_book().is_err());
        assert_eq!(
            actors.world_loop_outcome,
            Some(crate::script::WorldLoopOutcome::Death)
        );
    }

    #[test]
    fn player_initialization_resets_death_counters_but_preserves_captured_position() {
        let mut globals = crate::script::GameplayGlobals::default();
        globals.selected_health = Some(
            serde_json::from_value(serde_json::json!({
                "maximum":100,"displayed":0,"hit_inhibited":true,
                "death":{"progress":99,"mode":2,"local_position":[12,34,56]}
            }))
            .unwrap(),
        );
        globals.initialize_player_controller();
        let health = globals.selected_health().unwrap();
        assert!(!health.hit_inhibited());
        let death = health.death().unwrap();
        assert_eq!(death.progress(), 0);
        assert_eq!(death.mode(), 0);
        assert_eq!(death.local_position(), Some([12, 34, 56]));
        let mut saved = serde_json::to_value(health).unwrap();
        saved.as_object_mut().unwrap().remove("death");
        globals.selected_health = Some(serde_json::from_value(saved).unwrap());
        assert!(globals.selected_health().unwrap().death().is_none());
        globals.initialize_player_controller();
        assert_eq!(
            globals
                .selected_health()
                .unwrap()
                .death()
                .unwrap()
                .local_position(),
            None
        );
    }

    #[test]
    fn selected_health_snapshot_validates_bounds_and_retains_separate_inhibition() {
        let initial = SelectedHealthState::initialized(255);
        assert_eq!(initial.maximum(), 100);
        assert_eq!(initial.displayed(), 255);
        assert!(!initial.hit_inhibited());
        initial.validate().unwrap();
        let saved = serde_json::json!({"maximum":100,"displayed":43,"hit_inhibited":true});
        let restored: SelectedHealthState = serde_json::from_value(saved.clone()).unwrap();
        restored.validate().unwrap();
        assert!(restored.hit_inhibited());
        assert_eq!(restored.displayed(), 43);
        for (field, invalid) in [
            ("maximum", 0),
            ("maximum", 256),
            ("displayed", -1),
            ("displayed", 256),
        ] {
            let mut candidate = saved.clone();
            candidate[field] = invalid.into();
            let candidate: SelectedHealthState = serde_json::from_value(candidate).unwrap();
            assert!(candidate.validate().is_err(), "accepted {field}={invalid}");
        }
    }
}

#[cfg(test)]
mod falling_timeout_tests {
    use super::SelectedHealthState;
    #[test]
    fn legacy_falling_elapsed_remains_unknown_until_player_initialization() {
        let mut saved = serde_json::to_value(SelectedHealthState::initialized(40)).unwrap();
        saved.as_object_mut().unwrap().remove("falling_elapsed");
        let mut health: SelectedHealthState = serde_json::from_value(saved).unwrap();
        assert_eq!(health.falling_elapsed(), None);
        assert!(
            health
                .advance_falling_elapsed(false, 1)
                .unwrap_err()
                .to_string()
                .contains("owner unavailable")
        );
        health.initialize_death();
        assert_eq!(health.falling_elapsed(), Some(0));
    }
}
