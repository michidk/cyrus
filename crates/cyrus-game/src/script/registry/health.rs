//! Selected-player additive health; contact callers retain death dispatch ownership.
use super::{ScriptActorId, ScriptInputs, ScriptWorld};
use crate::script::{GameplayGlobals, ScriptSound};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

/// Session health initialized separately from controller/combat inhibition.
/// Follows preservation's SOUP selected-player additive-health rules.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SelectedHealthState {
    maximum: i32,
    displayed: i32,
    hit_inhibited: bool,
}

impl SelectedHealthState {
    pub(super) fn initialized(health: u8) -> Self {
        Self {
            maximum: 100,
            displayed: i32::from(health),
            hit_inhibited: false,
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

    /// Session maximum used by additive health clamping.
    pub fn maximum(&self) -> i32 {
        self.maximum
    }
    /// Health last published after successful feedback callbacks.
    pub fn displayed(&self) -> i32 {
        self.displayed
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
        let result = (|| {
            let index = self.resolve(id)?;
            let selected = if let Some(activation) = inputs.activation {
                Some(activation.player)
            } else {
                self.actors
                    .iter()
                    .position(|actor| matches!(actor.source, super::ScriptActorSource::Player))
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
                self.request_hurt_combat_taunt(id, inputs, random, sounds)?;
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
}

#[cfg(test)]
mod tests {
    use super::SelectedHealthState;

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
