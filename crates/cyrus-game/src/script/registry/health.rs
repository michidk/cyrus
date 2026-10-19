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
    pub(super) fn initialized(health: u8) -> Self {
        Self {
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

    pub(in crate::script) fn initialize_death(&mut self) {
        let death = self.death.get_or_insert(SelectedDeathState {
            progress: 0,
            mode: 0,
            local_position: None,
        });
        death.progress = 0;
        death.mode = 0;
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
    /// Ledge reset retains its own support and inhibition state. Attached-rope
    /// destruction remains unavailable. Death is
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
            let control = self.actors[index]
                .control
                .as_ref()
                .context("selected reaction control unavailable")?;
            ensure!(
                !control.rope_attached,
                "selected reaction attached-rope destruction unavailable"
            );
        }
        Ok(())
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
                ensure!(
                    !self.actors[index]
                        .control
                        .as_ref()
                        .context("selected death control unavailable")?
                        .rope_attached,
                    "selected death attached-rope detach unavailable"
                );
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
                let actor = &mut self.actors[index];
                let surface = actor.surface;
                let animation = actor.animation.as_mut().expect("validated animation");
                let frame = animation.playback.force_request_with_sounds(
                    group,
                    0,
                    ticks,
                    &mut |sound| {
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
