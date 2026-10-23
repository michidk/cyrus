//! Falling continuation requests precede the shared vertical writer.
use super::{ScriptActorId, ScriptInputs, ScriptWorld};
use anyhow::{Context, Result};

/// What the selected player's landing needs beyond the nonplayer response: the health services
/// publish through the HUD and draw random numbers, and a steep support changes the fatal path.
pub(super) struct SelectedLanding<'a, 'b> {
    pub inputs: ScriptInputs<'a>,
    pub random: &'b mut crate::random::EngineRandom,
    pub steep: bool,
}

/// Animation request selected by an admitted unsupported-ground continuation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OrdinaryFallingRequest {
    pub group: u16,
    pub force: bool,
}

/// Resolve preservation's falling continuation animation rules. The distance
/// uses wrapping local Q8 subtraction, followed by signed absolute magnitude.
pub fn ordinary_falling_request(
    falling: u8,
    torch_burning: u8,
    holding_weapon: bool,
    current_group: Option<u16>,
    local_y: i32,
    origin_y: i32,
) -> Option<OrdinaryFallingRequest> {
    if falling == 0 {
        return None;
    }
    let torch = torch_burning != 0;
    let group = if falling & 127 == 3 {
        if torch { 64 } else { 63 }
    } else if falling & 128 != 0 && !torch {
        58
    } else if local_y.wrapping_sub(origin_y).wrapping_abs() < 32768 && falling == 8 {
        if torch { 60 } else { 59 }
    } else if local_y.wrapping_sub(origin_y).wrapping_abs() < 32768 && falling == 9 {
        if torch { 62 } else { 61 }
    } else if holding_weapon {
        return (current_group != Some(115)).then_some(OrdinaryFallingRequest {
            group: 113,
            force: true,
        });
    } else if torch {
        57
    } else {
        56
    };
    Some(OrdinaryFallingRequest {
        group,
        force: false,
    })
}

/// Selected directional state writes and optional local Q8 translation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OrdinaryDirectionalMotion {
    pub direction: u8,
    pub movement_bits: u8,
    pub heading: u16,
    pub distance: Option<i32>,
}

/// Resolve the original motion-byte direction and deep-water distance reduction.
/// `None` means an unsupported input code caused no directional state writes.
pub fn ordinary_directional_motion(
    motion: u8,
    heading: u16,
    distance: i32,
    surface: u8,
    model_controls_motion: bool,
) -> Option<OrdinaryDirectionalMotion> {
    let (direction, movement_bits, offset) = match motion & 127 {
        2 | 6 | 8 | 9 | 24 => (2, 0x30, 0),
        3 | 7 => (3, 0xc0, 1024),
        4 => (4, 3, 512),
        5 => (5, 12, 1536),
        _ => return None,
    };
    let distance = if matches!(surface, 2 | 4) {
        distance >> 1
    } else {
        distance
    };
    Some(OrdinaryDirectionalMotion {
        direction,
        movement_bits,
        heading: heading.wrapping_add(offset) & 2047,
        distance: (!model_controls_motion && distance > 0).then_some(distance),
    })
}

/// Entry decision after selector, recovery and horizontal-displacement services.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrdinaryFallEntry {
    RestoreStable { force_group: u16 },
    Start { force_group: Option<u16> },
}

/// Select the documented final animated entry branch. This is separate from
/// continuation priority and must run after the preceding admission services.
pub fn ordinary_fall_entry(
    current_group: u16,
    default_group: u16,
    exit_reached: bool,
    torch_burning: u8,
    holding_weapon: bool,
) -> OrdinaryFallEntry {
    if matches!(current_group, 18 | 30) {
        if exit_reached {
            return OrdinaryFallEntry::RestoreStable {
                force_group: default_group,
            };
        }
        return OrdinaryFallEntry::Start {
            force_group: Some(if torch_burning == 0 { 59 } else { 60 }),
        };
    }
    OrdinaryFallEntry::Start {
        force_group: if holding_weapon {
            (current_group != 115).then_some(113)
        } else {
            Some(if torch_burning == 0 { 56 } else { 57 })
        },
    }
}

/// Original bookkeeping for a support-miss entry, after its separate admission.
/// Local Y and the returned origin use Q8; the attribute is in map units.
pub fn ordinary_fall_origin(local_y: i32, attribute_32: u8) -> (i32, u8) {
    (local_y.wrapping_add(i32::from(attribute_32) * 256), 30)
}

/// Nonplayer health uses a wrapping signed addition followed by byte clamping.
pub fn ordinary_health_value(health: u8, change: i32) -> u8 {
    i32::from(health).wrapping_add(change).clamp(0, 255) as u8
}

impl ScriptWorld {
    /// Save the stable position at the admitted post-ground-response stage.
    /// Active queued/falling/sliding state retains the old record and validity.
    pub fn retain_ordinary_stable_position(&mut self, id: ScriptActorId) -> Result<bool> {
        let index = self.resolve(id)?;
        let control = self.actors[index]
            .control
            .as_ref()
            .context("ordinary stable control unavailable")?;
        if control.queued_motion != 0 || control.falling_state != 0 || control.sliding_state != 0 {
            return Ok(false);
        }
        let position = self.spatial(id)?.local.position;
        let control = self.actors[index]
            .control
            .as_mut()
            .expect("validated stable control");
        control.stable_local_position = position;
        control.stable_position_valid = true;
        Ok(true)
    }

    /// Apply the nonplayer health service, including death dispatch.
    /// An armed receiver dies through the contact death dispatch. The selected player forwards to its own death-owning
    /// health with no source. Combat mode bit two must be supplied when attribute 97 requires
    /// that predicate.
    pub fn change_ordinary_health(
        &mut self,
        id: ScriptActorId,
        change: i32,
        data: &cyrus_assets::animation::Animations,
        environment: super::transition::ObstacleEnvironment,
        combat_mode_bit_two: Option<bool>,
        sounds: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<bool> {
        self.change_health(
            id,
            change,
            data,
            environment,
            combat_mode_bit_two,
            None,
            sounds,
        )
    }

    /// [`Self::change_ordinary_health`] with the selected player's HUD and random inputs.
    #[allow(clippy::too_many_arguments)] // the public wrapper above is the narrow form
    pub(super) fn change_health(
        &mut self,
        id: ScriptActorId,
        change: i32,
        data: &cyrus_assets::animation::Animations,
        environment: super::transition::ObstacleEnvironment,
        combat_mode_bit_two: Option<bool>,
        selected: Option<(ScriptInputs<'_>, &mut crate::random::EngineRandom)>,
        sounds: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<bool> {
        self.require_model_identity(data)?;
        let index = self.resolve(id)?;
        let actor = &self.actors[index];
        if actor.attributes[120] != 0 {
            return Ok(false);
        }
        let control = actor
            .control
            .as_ref()
            .context("ordinary health control unavailable")?;
        let group = actor
            .animation
            .as_ref()
            .and_then(|animation| animation.playback.current_group());
        let (holding, _, _) = super::super::activation::weapon_predicates(
            control.living_state,
            actor
                .animation
                .as_ref()
                .map(|_| super::super::ActivationAnimation { group }),
            control.combat_death_default.is_some(),
            control.weapon_drawn,
            actor.attributes[110],
            environment.item_system_ready,
            environment.shell_execution,
        );
        if actor.attributes[97] != 0
            && !holding
            && !combat_mode_bit_two.context("ordinary health combat mode predicate unavailable")?
        {
            return Ok(false);
        }
        if environment.selected_player == Some(id) {
            let (inputs, random) = selected.context("selected-player health inputs unavailable")?;
            self.apply_selected_death_health(id, change, None, inputs, Some(random), sounds)?;
            return Ok(true);
        }
        let health = ordinary_health_value(actor.attributes[3], change);
        let death_group =
            if control.falling_state != 0 || matches!(group, Some(114 | 116 | 130 | 131)) {
                74
            } else {
                134
            };
        self.actors[index].attributes[3] = health;
        if health == 0 && holding {
            // An armed receiver dies through the contact death dispatch with selector zero.
            if self.updates.animation_system {
                self.dispatch_contact_death(id, 0, Some(environment.ticks), sounds)?;
            }
            self.actors[index]
                .control
                .as_mut()
                .expect("validated health control")
                .death_selector = 8;
        } else if health == 0 {
            // The force-request boundary rejects an actor without animation state.
            if self.updates.animation_system && self.actors[index].animation.is_some() {
                let position = super::animation_sound_position(self.spatial.as_ref(), index);
                let event_context = super::effects::ActorEventContext::capture(self, id);
                let event_camera = &mut self.camera;
                let event_effects = &mut self.effects;
                let actor = &mut self.actors[index];
                let surface = actor.surface;
                let animation = actor.animation.as_mut().expect("checked animation state");
                match animation.playback.force_request_with_sounds(
                    death_group,
                    0,
                    environment.ticks,
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
                            super::super::ScriptSound::Animation {
                                sound,
                                surface,
                                position,
                            },
                        )
                    },
                ) {
                    Ok(frame) => animation.frame = frame.or(animation.frame),
                    Err(error) => {
                        let message = format!(
                            "world {} instance {index} health death animation: {error:#}",
                            self.world
                        );
                        actor.animation_failure = Some(message.clone());
                        anyhow::bail!(message);
                    }
                }
            }
            self.actors[index]
                .control
                .as_mut()
                .expect("validated health control")
                .death_selector = 8;
        }
        Ok(true)
    }

    /// Resolve landing thresholds after the owning support service has assigned
    /// actor surface. Requests precede health mutation and falling clears last.
    /// Selected-player fatal/slope presentation remains a separate response.
    pub fn resolve_ordinary_landing_damage(
        &mut self,
        id: ScriptActorId,
        data: &cyrus_assets::animation::Animations,
        environment: super::transition::ObstacleEnvironment,
        settings: OrdinaryLandingSettings,
        combat_mode_bit_two: Option<bool>,
        sounds: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<OrdinaryLandingDecision> {
        self.land_with_damage(
            id,
            data,
            environment,
            settings,
            combat_mode_bit_two,
            None,
            sounds,
        )
    }

    /// [`Self::resolve_ordinary_landing_damage`] with the selected player's landing inputs.
    #[allow(clippy::too_many_arguments)] // the public wrapper above is the narrow form
    pub(super) fn land_with_damage(
        &mut self,
        id: ScriptActorId,
        data: &cyrus_assets::animation::Animations,
        environment: super::transition::ObstacleEnvironment,
        settings: OrdinaryLandingSettings,
        combat_mode_bit_two: Option<bool>,
        mut selected: Option<SelectedLanding<'_, '_>>,
        sounds: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<OrdinaryLandingDecision> {
        self.require_model_identity(data)?;
        let index = self.resolve(id)?;
        let actor = &self.actors[index];
        let control = actor
            .control
            .as_ref()
            .context("ordinary landing damage control unavailable")?;
        anyhow::ensure!(
            control.falling_state != 0,
            "world {} instance {index}: landing damage without falling state",
            self.world
        );
        let surface = actor
            .surface
            .context("ordinary landing assigned surface unavailable")?;
        let decision = ordinary_landing_decision(
            self.spatial(id)?.local.position[1],
            control.fall_origin_y,
            surface,
            settings,
        );
        self.actors[index]
            .control
            .as_mut()
            .expect("validated landing control")
            .jump_selector = 0;
        let effect = match decision {
            OrdinaryLandingDecision::Fatal if environment.selected_player == Some(id) => {
                let landing = selected
                    .as_ref()
                    .context("selected-player landing inputs unavailable")?;
                if landing.steep {
                    // A steep support raises the controller's pending action and nothing else.
                    self.globals.raise_player_pending_action()?;
                    return Ok(decision);
                }
                // The death presentation comes from the override group, not an animation request.
                self.globals.set_player_death_override_group(74)?;
                Some((None, -256))
            }
            OrdinaryLandingDecision::Fatal => Some((Some(74), -256)),
            OrdinaryLandingDecision::Hurt { health_change } => Some((Some(73), health_change)),
            OrdinaryLandingDecision::HurtExcluded => None,
            OrdinaryLandingDecision::Ordinary => return Ok(decision),
        };
        if let Some((group, change)) = effect {
            // The request boundary rejects an actor without animation state (a script-only or
            // static-model object): the health change below still applies.
            let animated = self.actors[index].animation.is_some();
            if let (Some(group), true, true) = (group, self.updates.animation_system, animated) {
                let position = super::animation_sound_position(self.spatial.as_ref(), index);
                let event_context = super::effects::ActorEventContext::capture(self, id);
                let event_camera = &mut self.camera;
                let event_effects = &mut self.effects;
                let actor = &mut self.actors[index];
                let surface = actor.surface;
                let animation = actor.animation.as_mut().expect("checked animation state");
                match animation.playback.request_with_sounds(
                    group,
                    0,
                    environment.ticks,
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
                            super::super::ScriptSound::Animation {
                                sound,
                                surface,
                                position,
                            },
                        )
                    },
                ) {
                    Ok(frame) => animation.frame = frame.or(animation.frame),
                    Err(error) => {
                        let message = format!(
                            "world {} instance {index} landing damage animation: {error:#}",
                            self.world
                        );
                        actor.animation_failure = Some(message.clone());
                        anyhow::bail!(message);
                    }
                }
            }
            self.change_health(
                id,
                change,
                data,
                environment,
                combat_mode_bit_two,
                selected
                    .as_mut()
                    .map(|landing| (landing.inputs, &mut *landing.random)),
                sounds,
            )?;
        }
        self.actors[index]
            .control
            .as_mut()
            .expect("validated landing control")
            .falling_state = 0;
        self.actors[index].sync_falling_motion();
        Ok(decision)
    }

    /// Dispatch an admitted ordinary landing animation and then clear falling.
    /// The caller completes surface and threshold response first and supplies
    /// the independently resolved selected-player weapon predicate when needed.
    pub fn complete_ordinary_landing(
        &mut self,
        id: ScriptActorId,
        data: &cyrus_assets::animation::Animations,
        environment: super::transition::ObstacleEnvironment,
        selected_player_holding_weapon: Option<bool>,
        sounds: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<()> {
        self.require_model_identity(data)?;
        let index = self.resolve(id)?;
        let actor = &self.actors[index];
        let control = actor
            .control
            .as_ref()
            .context("ordinary landing control unavailable")?;
        let animation = actor
            .animation
            .as_ref()
            .context("ordinary landing animation unavailable")?;
        let (holding, _, _) = super::super::activation::weapon_predicates(
            control.living_state,
            Some(super::super::ActivationAnimation {
                group: animation.playback.current_group(),
            }),
            control.combat_death_default.is_some(),
            control.weapon_drawn,
            actor.attributes[110],
            environment.item_system_ready,
            environment.shell_execution,
        );
        let request = ordinary_landing_request(
            control.falling_state,
            control.torch_burning,
            holding,
            selected_player_holding_weapon,
            animation.playback.current_group(),
        )?;
        if let Some(group) = request.filter(|_| self.updates.animation_system) {
            let position = super::animation_sound_position(self.spatial.as_ref(), index);
            let event_context = super::effects::ActorEventContext::capture(self, id);
            let event_camera = &mut self.camera;
            let event_effects = &mut self.effects;
            let actor = &mut self.actors[index];
            let surface = actor.surface;
            let animation = actor
                .animation
                .as_mut()
                .expect("validated landing animation");
            match animation.playback.request_with_sounds(
                group,
                0,
                environment.ticks,
                &mut |sound| {
                    if let cyrus_world::animation::AnimationSound::Event { flags } = sound {
                        return event_context.dispatch(flags, event_camera.as_mut(), event_effects);
                    }
                    sounds(
                        id,
                        super::super::ScriptSound::Animation {
                            sound,
                            surface,
                            position,
                        },
                    )
                },
            ) {
                Ok(frame) => animation.frame = frame.or(animation.frame),
                Err(error) => {
                    let message = format!(
                        "world {} instance {index} landing animation: {error:#}",
                        self.world
                    );
                    actor.animation_failure = Some(message.clone());
                    anyhow::bail!(message);
                }
            }
        }
        self.actors[index]
            .control
            .as_mut()
            .expect("validated landing control")
            .falling_state = 0;
        self.actors[index].sync_falling_motion();
        Ok(())
    }

    /// Apply directional state and local translation; return the direction whose
    /// update indicator must be retained by the owning update traversal.
    pub fn apply_ordinary_directional_motion(
        &mut self,
        id: ScriptActorId,
        motion: u8,
        distance: i32,
    ) -> Result<Option<u8>> {
        let index = self.resolve(id)?;
        if !matches!(motion & 127, 2..=9 | 24) {
            return Ok(None);
        }
        let actor = &self.actors[index];
        let animation = actor
            .animation
            .as_ref()
            .context("ordinary directional animation unavailable")?;
        let surface = actor
            .surface
            .context("ordinary directional stored surface unavailable")?;
        let pose = self.spatial(id)?.local;
        let Some(action) = ordinary_directional_motion(
            motion,
            pose.angles[1],
            distance,
            surface,
            animation.playback.translation_suppressed(),
        ) else {
            return Ok(None);
        };
        let translated = action
            .distance
            .map(|distance| {
                cyrus_world::transform::translate_forward(pose.position, action.heading, distance)
            })
            .transpose()?;
        let control = self.actors[index]
            .control
            .as_mut()
            .context("ordinary directional control unavailable")?;
        control.motion_direction = action.direction;
        control.movement_flags |= action.movement_bits;
        self.globals.locomotion_called |= match action.direction {
            2 => 4,
            3 => 8,
            4 => 1,
            5 => 2,
            _ => unreachable!("directional policy supplies directions two through five"),
        };
        if let Some(position) = translated {
            self.spatial
                .as_mut()
                .context("ordinary directional spatial state unavailable")?
                .actor_mut(index)?
                .local
                .position = position;
        }
        Ok(Some(action.direction))
    }

    /// Complete animated support-miss entry after its selector, recovery and
    /// horizontal-displacement services. The response owner establishes that
    /// those preceding branches returned normally; this stage does not bypass
    /// them. Returns false for a rollback, true for initialized falling state.
    pub fn complete_ordinary_animated_fall_entry(
        &mut self,
        id: ScriptActorId,
        data: &cyrus_assets::animation::Animations,
        environment: super::transition::ObstacleEnvironment,
        sounds: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<bool> {
        self.require_model_identity(data)?;
        let index = self.resolve(id)?;
        let actor = &self.actors[index];
        let control = actor
            .control
            .as_ref()
            .context("animated fall entry control unavailable")?;
        anyhow::ensure!(
            control.falling_state == 0,
            "world {} instance {index}: animated fall entry already falling",
            self.world
        );
        anyhow::ensure!(
            data.actor(actor.name())?["model_load_path"].as_str() == Some("animation_table"),
            "world {} instance {index}: animated fall entry model path unavailable",
            self.world
        );
        let animation = actor
            .animation
            .as_ref()
            .context("animated fall entry receiver unavailable")?;
        let group = animation
            .playback
            .current_group()
            .context("animated fall entry current group unavailable")?;
        let (holding, _, _) = super::super::activation::weapon_predicates(
            control.living_state,
            Some(super::super::ActivationAnimation { group: Some(group) }),
            control.combat_death_default.is_some(),
            control.weapon_drawn,
            actor.attributes[110],
            environment.item_system_ready,
            environment.shell_execution,
        );
        let entry = ordinary_fall_entry(
            group,
            animation.playback.default_group(),
            animation.playback.exit_reached(),
            control.torch_burning,
            holding,
        );
        let request = match entry {
            OrdinaryFallEntry::RestoreStable { force_group } => {
                let stable = self.actors[index]
                    .control
                    .as_ref()
                    .expect("validated fall entry control")
                    .stable_local_position;
                self.spatial
                    .as_mut()
                    .context("animated fall entry spatial state unavailable")?
                    .actor_mut(index)?
                    .local
                    .position = stable;
                Some(force_group)
            }
            OrdinaryFallEntry::Start { force_group } => {
                let (origin, timer) =
                    ordinary_fall_origin(self.spatial(id)?.local.position[1], actor.attributes[32]);
                let control = self.actors[index]
                    .control
                    .as_mut()
                    .expect("validated fall entry control");
                control.fall_origin_y = origin;
                control.fall_timer = timer;
                control.falling_state = 1;
                self.actors[index].sync_falling_motion();
                force_group
            }
        };
        if let Some(group) = request.filter(|_| self.updates.animation_system) {
            let position = super::animation_sound_position(self.spatial.as_ref(), index);
            let event_context = super::effects::ActorEventContext::capture(self, id);
            let event_camera = &mut self.camera;
            let event_effects = &mut self.effects;
            let actor = &mut self.actors[index];
            let surface = actor.surface;
            let animation = actor
                .animation
                .as_mut()
                .expect("validated animated fall receiver");
            let result = animation.playback.force_request_with_sounds(
                group,
                0,
                environment.ticks,
                &mut |sound| {
                    if let cyrus_world::animation::AnimationSound::Event { flags } = sound {
                        return event_context.dispatch(flags, event_camera.as_mut(), event_effects);
                    }
                    sounds(
                        id,
                        super::super::ScriptSound::Animation {
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
                        "world {} instance {index} fall entry animation: {error:#}",
                        self.world
                    );
                    actor.animation_failure = Some(message.clone());
                    anyhow::bail!(message);
                }
            }
        }
        Ok(matches!(entry, OrdinaryFallEntry::Start { .. }))
    }

    /// Start a static/model-less ordinary fall after an admitted support miss.
    /// Animated entry has separate recovery and displacement services and must
    /// use its own admission path. Existing falls retain their origin and timer.
    pub fn begin_static_ordinary_fall(
        &mut self,
        id: ScriptActorId,
        data: &cyrus_assets::animation::Animations,
    ) -> Result<bool> {
        self.require_model_identity(data)?;
        let index = self.resolve(id)?;
        let actor = &self.actors[index];
        let control = actor
            .control
            .as_ref()
            .context("ordinary fall entry control unavailable")?;
        if control.falling_state != 0 {
            return Ok(false);
        }
        anyhow::ensure!(
            control.queued_motion == 0 && control.sliding_state == 0,
            "world {} instance {index}: static fall entry motion admission unavailable",
            self.world
        );
        anyhow::ensure!(
            !matches!(control.living_state, 10..=19 | 27..=32),
            "world {} instance {index}: static fall entry special state unsupported",
            self.world
        );
        let path = data.actor(actor.name())?["model_load_path"]
            .as_str()
            .context("ordinary fall entry model path unavailable")?;
        anyhow::ensure!(
            matches!(path, "static" | "none"),
            "world {} instance {index}: animated fall entry services unsupported",
            self.world
        );
        anyhow::ensure!(
            actor.attributes[107] == 0,
            "world {} instance {index}: fall-disabled attribute excludes entry",
            self.world
        );
        let (origin, timer) =
            ordinary_fall_origin(self.spatial(id)?.local.position[1], actor.attributes[32]);
        let control = self.actors[index]
            .control
            .as_mut()
            .expect("validated fall entry control");
        control.fall_origin_y = origin;
        control.fall_timer = timer;
        control.falling_state = 1;
        self.actors[index].sync_falling_motion();
        Ok(true)
    }

    /// Continue an already admitted ordinary fall after a support miss. This
    /// does not initialize a fall or run landing response. The caller runs the selected
    /// player's edge service first.
    // The continuation consumes two independent tick samples and the probe height.
    #[allow(clippy::too_many_arguments)]
    pub fn continue_ordinary_falling(
        &mut self,
        id: ScriptActorId,
        data: &cyrus_assets::animation::Animations,
        environment: super::transition::ObstacleEnvironment,
        fine_ticks: u32,
        additional_ticks: u32,
        post_collide_height: u32,
        sounds: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<bool> {
        self.require_model_identity(data)?;
        let index = self.resolve(id)?;
        let actor = &self.actors[index];
        let control = actor
            .control
            .as_ref()
            .context("ordinary falling control unavailable")?;
        if control.falling_state == 0 {
            return Ok(false);
        }
        let path = data.actor(actor.name())?["model_load_path"]
            .as_str()
            .context("ordinary falling model path unavailable")?;
        anyhow::ensure!(
            matches!(path, "animation_table" | "static" | "none"),
            "world {} instance {index}: falling model path {path} unsupported",
            self.world
        );
        let animated = path == "animation_table";
        if animated {
            let animation = actor
                .animation
                .as_ref()
                .context("ordinary falling animation unavailable")?;
            let (holding, _, _) = super::super::activation::weapon_predicates(
                control.living_state,
                Some(super::super::ActivationAnimation {
                    group: animation.playback.current_group(),
                }),
                control.combat_death_default.is_some(),
                control.weapon_drawn,
                actor.attributes[110],
                environment.item_system_ready,
                environment.shell_execution,
            );
            let request = ordinary_falling_request(
                control.falling_state,
                control.torch_burning,
                holding,
                animation.playback.current_group(),
                self.spatial(id)?.local.position[1],
                control.fall_origin_y,
            );
            if let Some(request) = request.filter(|_| self.updates.animation_system) {
                let position = super::animation_sound_position(self.spatial.as_ref(), index);
                let event_context = super::effects::ActorEventContext::capture(self, id);
                let event_camera = &mut self.camera;
                let event_effects = &mut self.effects;
                let actor = &mut self.actors[index];
                let surface = actor.surface;
                let animation = actor
                    .animation
                    .as_mut()
                    .expect("validated falling animation");
                let mut emit = |sound| {
                    if let cyrus_world::animation::AnimationSound::Event { flags } = sound {
                        return event_context.dispatch(flags, event_camera.as_mut(), event_effects);
                    }
                    sounds(
                        id,
                        super::super::ScriptSound::Animation {
                            sound,
                            surface,
                            position,
                        },
                    )
                };
                let result = if request.force {
                    animation.playback.force_request_with_sounds(
                        request.group,
                        0,
                        environment.ticks,
                        &mut emit,
                    )
                } else {
                    animation.playback.request_with_sounds(
                        request.group,
                        0,
                        environment.ticks,
                        &mut emit,
                    )
                };
                match result {
                    Ok(frame) => animation.frame = frame.or(animation.frame),
                    Err(error) => {
                        let message = format!(
                            "world {} instance {index} falling animation: {error:#}",
                            self.world
                        );
                        actor.animation_failure = Some(message.clone());
                        anyhow::bail!(message);
                    }
                }
            }
        }
        self.run_ordinary_falling_writer(
            id,
            animated,
            fine_ticks,
            additional_ticks,
            post_collide_height,
        )?;
        Ok(true)
    }

    /// The vertical writer that follows every falling request, including the one
    /// a fresh entry makes.
    pub(super) fn run_ordinary_falling_writer(
        &mut self,
        id: ScriptActorId,
        animated: bool,
        fine_ticks: u32,
        additional_ticks: u32,
        post_collide_height: u32,
    ) -> Result<()> {
        let actor = &self.actors[self.resolve(id)?];
        let suppressed = animated
            && actor
                .animation
                .as_ref()
                .is_some_and(|animation| animation.playback.translation_suppressed());
        let timer = actor
            .control
            .as_ref()
            .context("ordinary falling control unavailable")?
            .fall_timer;
        let input = super::vertical::OrdinaryVerticalInputs::for_falling(
            timer,
            fine_ticks,
            additional_ticks,
            post_collide_height,
            suppressed,
        )?;
        self.apply_ordinary_falling_vertical(id, input)
    }
}

/// Installed landing thresholds in map units and the installed health decrement.
/// The response owner resolves them from SYSTEM.INI before invoking this rule.
#[derive(Clone, Copy, Debug)]
pub struct OrdinaryLandingSettings {
    pub death_height: i32,
    pub hurt_height: i32,
    pub hurt_zap: i32,
}

/// Threshold result precedes ordinary landing animation selection. `Ordinary`
/// still needs its motion/weapon-specific animation stage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrdinaryLandingDecision {
    Fatal,
    Hurt { health_change: i32 },
    HurtExcluded,
    Ordinary,
}

/// Resolve original wrapping Q8 distance, water suppression and strict thresholds.
/// Hurt exclusion reads the stored surface state. The selected-player fatal
/// presentation is not selected here.
pub fn ordinary_landing_decision(
    local_y: i32,
    origin_y: i32,
    surface: u8,
    settings: OrdinaryLandingSettings,
) -> OrdinaryLandingDecision {
    let distance = if (1..=4).contains(&surface) {
        0
    } else {
        local_y.wrapping_sub(origin_y).wrapping_abs()
    };
    if distance > settings.death_height.wrapping_mul(256) {
        OrdinaryLandingDecision::Fatal
    } else if distance > settings.hurt_height.wrapping_mul(256) {
        if matches!(surface, 1..=5 | 11) {
            OrdinaryLandingDecision::HurtExcluded
        } else {
            OrdinaryLandingDecision::Hurt {
                health_change: settings.hurt_zap.wrapping_neg(),
            }
        }
    } else {
        OrdinaryLandingDecision::Ordinary
    }
}

/// Resolve the ordinary landing request after threshold response. States eight
/// and nine read the selected player's weapon predicate, not the landing actor's.
/// A missing selected-player predicate is unsupported only in those branches.
pub fn ordinary_landing_request(
    falling: u8,
    torch_burning: u8,
    holding_weapon: bool,
    selected_player_holding_weapon: Option<bool>,
    current_group: Option<u16>,
) -> Result<Option<u16>> {
    if falling == 0 {
        return Ok(None);
    }
    let torch = torch_burning != 0;
    Ok(match falling & 127 {
        8 => Some(
            if selected_player_holding_weapon
                .context("landing state 8 selected-player weapon predicate unavailable")?
            {
                130
            } else if torch {
                68
            } else {
                67
            },
        ),
        9 => Some(
            if selected_player_holding_weapon
                .context("landing state 9 selected-player weapon predicate unavailable")?
            {
                131
            } else if torch {
                70
            } else {
                69
            },
        ),
        _ if !holding_weapon => Some(if torch { 66 } else { 65 }),
        _ => (!matches!(current_group, Some(115 | 116))).then_some(114),
    })
}
