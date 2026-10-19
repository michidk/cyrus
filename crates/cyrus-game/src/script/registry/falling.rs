//! Falling continuation requests precede the shared vertical writer.
use super::{ScriptActorId, ScriptWorld};
use anyhow::{Context, Result};

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
    RestorePrevious { force_group: u16 },
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
            return OrdinaryFallEntry::RestorePrevious {
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

    /// Apply the nonplayer unarmed health service, including death dispatch.
    /// Armed death and selected-player health have separate services. Combat
    /// mode bit two must be supplied when attribute 97 requires that predicate.
    pub fn change_ordinary_health(
        &mut self,
        id: ScriptActorId,
        change: i32,
        data: &cyrus_assets::animation::Animations,
        environment: super::transition::ObstacleEnvironment,
        combat_mode_bit_two: Option<bool>,
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
        anyhow::ensure!(
            environment.selected_player != Some(id),
            "world {} instance {index}: selected-player health service unsupported",
            self.world
        );
        let health = ordinary_health_value(actor.attributes[3], change);
        anyhow::ensure!(
            health != 0 || !holding,
            "world {} instance {index}: armed health death services unsupported",
            self.world
        );
        let death_group =
            if control.falling_state != 0 || matches!(group, Some(114 | 116 | 130 | 131)) {
                74
            } else {
                134
            };
        self.actors[index].attributes[3] = health;
        if health == 0 {
            if self.updates.animation_system {
                let position = super::animation_sound_position(self.spatial.as_ref(), index);
                let actor = &mut self.actors[index];
                let surface = actor.surface;
                let animation = actor
                    .animation
                    .as_mut()
                    .context("ordinary health death animation unavailable")?;
                match animation.playback.force_request_with_sounds(
                    death_group,
                    0,
                    environment.ticks,
                    &mut |sound| {
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
            OrdinaryLandingDecision::Fatal => {
                anyhow::ensure!(
                    environment.selected_player != Some(id),
                    "world {} instance {index}: selected-player fatal landing service unsupported",
                    self.world
                );
                Some((74, -256))
            }
            OrdinaryLandingDecision::Hurt { health_change } => Some((73, health_change)),
            OrdinaryLandingDecision::HurtExcluded => None,
            OrdinaryLandingDecision::Ordinary => return Ok(decision),
        };
        if let Some((group, change)) = effect {
            if self.updates.animation_system {
                let position = super::animation_sound_position(self.spatial.as_ref(), index);
                let actor = &mut self.actors[index];
                let surface = actor.surface;
                let animation = actor
                    .animation
                    .as_mut()
                    .context("ordinary landing damage animation unavailable")?;
                match animation.playback.request_with_sounds(
                    group,
                    0,
                    environment.ticks,
                    &mut |sound| {
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
            self.change_ordinary_health(
                id,
                change,
                data,
                environment,
                combat_mode_bit_two,
                sounds,
            )?;
        }
        self.actors[index]
            .control
            .as_mut()
            .expect("validated landing control")
            .falling_state = 0;
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
            OrdinaryFallEntry::RestorePrevious { force_group } => {
                let previous = self.spatial(id)?.previous_local_position;
                self.spatial
                    .as_mut()
                    .context("animated fall entry spatial state unavailable")?
                    .actor_mut(index)?
                    .local
                    .position = previous;
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
                force_group
            }
        };
        if let Some(group) = request.filter(|_| self.updates.animation_system) {
            let position = super::animation_sound_position(self.spatial.as_ref(), index);
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
        Ok(true)
    }

    /// Continue an already admitted ordinary fall after a support miss. This
    /// does not initialize a fall or run landing response. Animated selected
    /// player edge services must be supplied by their owner and remain unsupported.
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
            anyhow::ensure!(
                environment.selected_player != Some(id),
                "world {} instance {index}: falling selected-player edge service unsupported",
                self.world
            );
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
                let actor = &mut self.actors[index];
                let surface = actor.surface;
                let animation = actor
                    .animation
                    .as_mut()
                    .expect("validated falling animation");
                let mut emit = |sound| {
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
        let actor = &self.actors[index];
        let suppressed = animated
            && actor
                .animation
                .as_ref()
                .is_some_and(|animation| animation.playback.translation_suppressed());
        let timer = actor
            .control
            .as_ref()
            .expect("validated falling control")
            .fall_timer;
        let input = super::vertical::OrdinaryVerticalInputs::for_falling(
            timer,
            fine_ticks,
            additional_ticks,
            post_collide_height,
            suppressed,
        )?;
        self.apply_ordinary_falling_vertical(id, input)?;
        Ok(true)
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
