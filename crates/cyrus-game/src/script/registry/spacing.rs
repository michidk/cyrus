//! Fight spacing reads and mutates the receiver's shared movement state.
use super::{ScriptActorId, ScriptWorld, obstacle::OrdinaryObstacleQuery};
use anyhow::{Context, Result, ensure};

/// Ordered neighbors retained by combat spacing, not a sorted nearest pair.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OrdinarySpacingNeighbors {
    pub primary: Option<ScriptActorId>,
    pub secondary: Option<ScriptActorId>,
}

/// Shared combat-pass samples consumed by fight spacing. Distance is in Q8
/// runtime units; tick_counter is the raw cached BIOS sample interpreted as signed.
#[derive(Clone, Copy, Debug)]
pub struct OrdinarySpacingInput {
    pub distance: i32,
    pub tick_counter: i32,
}

/// Existing movement executors available to ordinary combat spacing.
#[derive(Clone, Copy, Debug)]
pub enum OrdinaryMovement {
    Forward,
    WalkForward,
    Backward,
    WalkBackward,
    Left,
    Right,
}
impl OrdinaryMovement {
    fn native_name(self) -> &'static str {
        match self {
            Self::Forward => "MoveForward",
            Self::WalkForward => "WalkForward",
            Self::Backward => "MoveBackward",
            Self::WalkBackward => "WalkBackward",
            Self::Left => "MoveLeft",
            Self::Right => "MoveRight",
        }
    }
}

/// Live host predicates and timing for preservation's forward movement amount query.
/// Weapon state comes from the item/shell owner; combat, surface, animation and
/// packed startup state are read from the receiver at invocation time.
#[derive(Clone, Copy)]
pub struct OrdinaryForwardAmountInputs {
    pub selected_player: Option<ScriptActorId>,
    pub holding_weapon: bool,
    pub frame_scale: u32,
    pub ticks: u32,
    pub duration: i32,
    pub elapsed: i32,
}

impl ScriptWorld {
    /// Run the disengaged nonplayer branch from one shared Q8 distance sample.
    /// Facing and arrival use the retained point, independently of the live target.
    /// The caller establishes the facing predicates and shared timing; movement
    /// uses the ordinary native services synchronously without a persistent task.
    /// Earlier facing/arrival writes survive a later movement-service error.
    pub fn move_disengaged_combat(
        &mut self,
        id: ScriptActorId,
        distance: i32,
        facing: super::slope::OrdinarySlopeInputs,
        inputs: super::ScriptInputs<'_>,
        sounds: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<()> {
        let result = (|| -> Result<_> {
            let index = self.resolve(id)?;
            ensure!(
                !matches!(
                    self.actors[index].source,
                    super::ScriptActorSource::Player | super::ScriptActorSource::AlternatePlayer
                ) && facing.selected_player != Some(id),
                "selected-player disengaged combat movement unavailable"
            );
            let point = self.actors[index]
                .control
                .as_ref()
                .and_then(|control| control.combat.as_ref())
                .context("disengaged combat record unavailable")?
                .retained_target_position;
            if self.face_ordinary_point(id, point, facing, sounds)? == 0
                && !self.arrive_ordinary_point(id, point)?
            {
                let movement = if distance < 500001 {
                    OrdinaryMovement::Forward
                } else {
                    OrdinaryMovement::WalkForward
                };
                self.move_ordinary(id, movement, 1, 0, inputs, sounds)?;
            }
            Ok(())
        })();
        result.with_context(|| {
            format!(
                "world {} instance {} disengaged combat movement",
                self.world, id.index
            )
        })
    }

    fn spacing_distance(&self, id: ScriptActorId, other: ScriptActorId) -> Result<i32> {
        let origin = self.query_position(id)?;
        let other = self.query_position(other)?;
        cyrus_collision::distance::rounded_distance([
            other[0].wrapping_sub(origin[0]),
            0,
            other[2].wrapping_sub(origin[2]),
        ])
    }

    pub(super) fn spacing_weapon(
        &mut self,
        id: ScriptActorId,
        inputs: super::ScriptInputs<'_>,
    ) -> Result<(Option<ScriptActorId>, bool)> {
        let index = self.resolve(id)?;
        if let Some(activation) = inputs.activation {
            self.validate_activation(activation)?;
        }
        let host = super::GameHost {
            native_source_program: None,
            world: self.world,
            animation_system: self.updates.animation_system,
            task_system: self.updates.task_system,
            index,
            generation: self.generation,
            ordinary_actors: None,
            walk_map_count: None,
            registry: self,
            sounds: None,
            random: None,
            inputs,
        };
        Ok((
            host.selected_player(),
            host.receiver_weapon_predicates("IsHoldingWeapon")?.0,
        ))
    }

    fn spacing_step(
        &mut self,
        id: ScriptActorId,
        movement: OrdinaryMovement,
        inputs: super::ScriptInputs<'_>,
        sounds: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<bool> {
        let geometry = inputs
            .locomotion_geometry
            .context("spacing directional geometry unavailable")?;
        let direction = match movement {
            OrdinaryMovement::Backward => super::hazard::OrdinaryStepDirection::Backward,
            OrdinaryMovement::Left => super::hazard::OrdinaryStepDirection::Left,
            OrdinaryMovement::Right => super::hazard::OrdinaryStepDirection::Right,
            _ => anyhow::bail!("spacing admission requires backward or lateral movement"),
        };
        if !self.ordinary_step_clear(
            id,
            direction,
            geometry.ground,
            geometry.environment,
            sounds,
        )? {
            return Ok(false);
        }
        self.move_ordinary(id, movement, 1, 0, inputs, sounds)?;
        Ok(true)
    }

    /// Run one admitted ordinary fight-spacing pass from live combat state and
    /// scheduler-owned distance/tick samples. All requests execute synchronously.
    /// Only action -1 and an absolute facing result below 128 admit the pass.
    /// An admitted successful pass registers deferred map movement even when no
    /// movement was requested. Automatic combat scheduler integration is separate.
    pub fn space_ordinary(
        &mut self,
        id: ScriptActorId,
        sample: OrdinarySpacingInput,
        inputs: super::ScriptInputs<'_>,
        random: &mut crate::random::EngineRandom,
        sounds: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<()> {
        let result = (|| -> Result<()> {
            let index = self.resolve(id)?;
            let combat = self.actors[index]
                .control
                .as_ref()
                .and_then(|control| control.combat.as_ref())
                .context("fight spacing combat record unavailable")?;
            if combat.action != -1 {
                return Ok(());
            }
            let target = combat.target.context("fight spacing target unavailable")?;
            let [mut near, mut far, mut third] = combat.spacing_ranges;
            let mut period = combat.spacing_period;
            let allocation = combat.allocation_index;
            let stance = combat.attacker_selection_state != 0;
            let active = combat.flags & 4 != 0;
            let frame_scale = inputs
                .frame_scale
                .context("fight spacing frame_scale unavailable")?;
            let ticks = inputs
                .nominal_ticks
                .context("fight spacing nominal_ticks unavailable")?;
            let (selected_player, holding_weapon) = self.spacing_weapon(id, inputs)?;
            ensure!(
                selected_player != Some(id)
                    && !matches!(
                        self.actors[index].source,
                        super::ScriptActorSource::Player
                            | super::ScriptActorSource::AlternatePlayer
                    ),
                "selected-player fight spacing unavailable"
            );
            if !stance {
                near = 30000;
                far = far.wrapping_mul(3);
                third = third
                    .wrapping_mul(3)
                    .wrapping_add(allocation.wrapping_mul(random.draw().wrapping_mul(100) / 32767));
                period = period
                    .wrapping_mul(2)
                    .wrapping_add(allocation.wrapping_mul(random.draw().wrapping_mul(10) / 32767));
            }
            let angle = self.face_ordinary_target(
                id,
                target,
                super::slope::OrdinarySlopeInputs {
                    selected_player,
                    holding_weapon,
                    combat_active: active,
                    frame_scale,
                    ticks,
                },
                sounds,
            )?;
            if angle.wrapping_abs() >= 128 {
                return Ok(());
            }
            if sample.distance < near {
                for movement in [
                    OrdinaryMovement::Backward,
                    OrdinaryMovement::Right,
                    OrdinaryMovement::Left,
                ] {
                    if self.spacing_step(id, movement, inputs, sounds)? {
                        break;
                    }
                }
            } else if sample.distance > far.wrapping_mul(3) {
                if self.actors[index]
                    .animation
                    .as_ref()
                    .is_some_and(|a| a.playback.has_group(31))
                {
                    self.move_ordinary(id, OrdinaryMovement::WalkForward, 1, 0, inputs, sounds)?;
                }
            } else if sample.distance > third.wrapping_add(far.wrapping_sub(third) / 2) {
                if self.actors[index]
                    .animation
                    .as_ref()
                    .is_some_and(|a| a.playback.has_group(92))
                {
                    let query = inputs.locomotion_geometry.map(|geometry| {
                        super::obstacle::OrdinaryObstacleQuery {
                            scene: geometry.ground.scene,
                            animations: geometry.ground.animations,
                            prepared: geometry.ground.prepared,
                            vertical_offset: 0,
                            respond: false,
                        }
                    });
                    let amount = self.ordinary_forward_amount(
                        id,
                        OrdinaryForwardAmountInputs {
                            selected_player,
                            holding_weapon,
                            frame_scale,
                            ticks,
                            duration: 1,
                            elapsed: 0,
                        },
                        query,
                        sounds,
                    )?;
                    if sample.distance.wrapping_sub(amount) >= third {
                        self.move_ordinary(id, OrdinaryMovement::Forward, 1, 0, inputs, sounds)?;
                        self.spacing_distance(id, target)?;
                    }
                }
            } else if !self.space_ordinary_neighbors(id, target, inputs, sounds)? {
                let dividend = sample.tick_counter.wrapping_add(allocation);
                let remainder = dividend.checked_rem(period).with_context(|| {
                    format!("fight spacing division fault: {dividend} % {period}")
                })?;
                if remainder == 1 {
                    let choice = random.draw().wrapping_mul(20) / 32767;
                    let movement = if choice < 16 {
                        OrdinaryMovement::Backward
                    } else if choice < 18 {
                        OrdinaryMovement::Right
                    } else {
                        OrdinaryMovement::Left
                    };
                    self.spacing_step(id, movement, inputs, sounds)?;
                }
            }
            if !self.map_updates.contains(&index) {
                self.map_updates.push(index);
            }
            Ok(())
        })();
        result.with_context(|| format!("world {} instance {} fight spacing", self.world, id.index))
    }

    /// Select from retained ordinary traversal using live targeting, selection-state
    /// one and IsDead. None means no eligible attacker; a first receiver still
    /// establishes a pass with absent neighbors. The first entry is unmeasured.
    pub fn ordinary_spacing_neighbors(
        &self,
        id: ScriptActorId,
        target: ScriptActorId,
        selected_player: Option<ScriptActorId>,
    ) -> Result<Option<OrdinarySpacingNeighbors>> {
        let result = (|| -> Result<_> {
            self.resolve(id)?;
            self.resolve(target)?;
            if let Some(player) = selected_player {
                self.resolve(player)?;
            }
            let traversal = self
                .actor_lists
                .as_ref()
                .and_then(|lists| lists.retained_selected())
                .context("neighbor spacing ordinary traversal unavailable")?;
            let mut pair = None;
            let mut bound = 65535;
            for &candidate in traversal {
                let index = self.resolve(candidate)?;
                if candidate == target {
                    continue;
                }
                let Some(combat) = self.actors[index]
                    .control
                    .as_ref()
                    .and_then(|control| control.combat.as_ref())
                else {
                    continue;
                };
                if combat.target != Some(target)
                    || combat.attacker_selection_state != 1
                    || self.query_is_dead(candidate, selected_player)?
                {
                    continue;
                }
                let Some(pair) = pair.as_mut() else {
                    pair = Some(OrdinarySpacingNeighbors {
                        primary: (candidate != id).then_some(candidate),
                        secondary: None,
                    });
                    continue;
                };
                if candidate == id {
                    continue;
                }
                let distance = self.spacing_distance(id, candidate)?;
                if distance < bound {
                    bound = distance;
                    pair.secondary = pair.primary;
                    pair.primary = Some(candidate);
                }
            }
            Ok(pair)
        })();
        result.with_context(|| {
            format!(
                "world {} instance {} target {} spacing neighbors",
                self.world, id.index, target.index
            )
        })
    }

    /// Request the original neighbor sidestep synchronously. A dispatched request
    /// returns true regardless of native movement completion and creates no VM task.
    /// The enclosing spacing root owns deferred map registration.
    pub fn space_ordinary_neighbors(
        &mut self,
        id: ScriptActorId,
        target: ScriptActorId,
        inputs: super::ScriptInputs<'_>,
        sounds: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<bool> {
        let result = (|| -> Result<bool> {
            let selected = inputs
                .activation
                .map(|activation| activation.player)
                .or_else(|| {
                    self.selected_player_index().map(|index| ScriptActorId {
                        generation: self.generation,
                        index,
                    })
                });
            let Some(pair) = self.ordinary_spacing_neighbors(id, target, selected)? else {
                return Ok(false);
            };
            let a = self.ordinary_neighbor_separation(id, target, pair.primary)?;
            let b = self.ordinary_neighbor_separation(id, target, pair.secondary)?;
            let threshold = self
                .non_engaged_spacing
                .context("installed non_engaged_spacing unavailable")?;
            let separation = if a.wrapping_abs() < threshold {
                (b.wrapping_abs() > threshold / 3).then_some(a)
            } else {
                (b.wrapping_abs() < threshold && a.wrapping_abs() > threshold / 2).then_some(b)
            };
            let Some(separation) = separation else {
                return Ok(false);
            };
            let movement = if separation < 0 {
                OrdinaryMovement::Right
            } else {
                OrdinaryMovement::Left
            };
            self.move_ordinary(id, movement, 1, 0, inputs, sounds)?;
            Ok(true)
        })();
        result.with_context(|| {
            format!(
                "world {} instance {} target {} neighbor sidestep",
                self.world, id.index, target.index
            )
        })
    }

    /// Signed neighbor separation in 2048 units per turn, using ordinary local
    /// Q8 positions and preservation's normalized vector-to-angle conversion.
    /// Subtractions wrap before arithmetic shift eight; exact half-turn signs
    /// remain distinct. An absent neighbor returns 1024 without spatial queries.
    pub fn ordinary_neighbor_separation(
        &self,
        id: ScriptActorId,
        target: ScriptActorId,
        neighbor: Option<ScriptActorId>,
    ) -> Result<i32> {
        let Some(neighbor) = neighbor else {
            return Ok(1024);
        };
        let result = (|| -> Result<i32> {
            let target = self.spatial(target)?.local.position;
            let angle = |actor| -> Result<i32> {
                let position = self.spatial(actor)?.local.position;
                let displacement =
                    std::array::from_fn(|axis| position[axis].wrapping_sub(target[axis]) >> 8);
                let [x, _, z] = super::super::spatial::normalized_direction(displacement);
                Ok((-i32::from(super::super::spatial::query_bearing(x, z)) - 1024) & 2047)
            };
            let difference = angle(id)? - angle(neighbor)?;
            Ok(if difference > 1024 {
                difference - 2048
            } else if difference < -1024 {
                difference + 2048
            } else {
                difference
            })
        })();
        result.with_context(|| {
            format!(
                "world {} instance {} neighbor {} target {} spacing separation",
                self.world, id.index, neighbor.index, target.index
            )
        })
    }

    /// Invoke one ordinary movement update synchronously with shared native
    /// animation, item/shell, geometry and spatial services. Duration and elapsed
    /// use nominal ticks; returns zero on completion or one to continue.
    /// This updates the real receiver without creating a persistent VM task.
    /// The enclosing spacing pass owns map registration. Selected-player movement
    /// remains unavailable here.
    pub fn move_ordinary(
        &mut self,
        id: ScriptActorId,
        movement: OrdinaryMovement,
        duration: i32,
        elapsed: i32,
        inputs: super::ScriptInputs<'_>,
        sounds: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<i32> {
        let result = (|| -> Result<i32> {
            let index = self.resolve(id)?;
            ensure!(
                inputs.frame_scale.is_some() && inputs.nominal_ticks.is_some(),
                "ordinary movement requires frame_scale and nominal_ticks"
            );
            if let Some(activation) = inputs.activation {
                self.validate_activation(activation)?;
            }
            ensure!(
                !matches!(
                    self.actors[index].source,
                    super::ScriptActorSource::Player | super::ScriptActorSource::AlternatePlayer
                ) && inputs
                    .activation
                    .is_none_or(|activation| activation.player != id),
                "selected-player synchronous movement unavailable"
            );
            if let Some(error) = self.actors[index].failure() {
                anyhow::bail!("{error}");
            }
            let ordinary_actors = self
                .actor_lists
                .as_ref()
                .and_then(|lists| lists.retained_selected())
                .map(<[_]>::to_vec);
            let mut host = super::GameHost {
                native_source_program: None,
                world: self.world,
                animation_system: self.updates.animation_system,
                task_system: self.updates.task_system,
                index,
                generation: self.generation,
                ordinary_actors: ordinary_actors.as_deref(),
                walk_map_count: self.walk_nodes.as_ref().map(|nodes| nodes.map_count()),
                registry: self,
                sounds: Some(sounds),
                random: None,
                inputs,
            };
            let function = movement.native_name();
            let result = host.native_task_call(
                (
                    function,
                    super::super::tasks::NativeTaskMode::SynchronousMovement,
                ),
                &[cyrus_script::Argument::Integer(duration)],
                cyrus_script::Invocation {
                    elapsed: Some(elapsed),
                    unpaused_tasks: 0,
                    unpaused_blocking_tasks: 0,
                },
            )?;
            match result {
                Some(cyrus_script::CallResult::Complete(value)) => Ok(value),
                _ => anyhow::bail!(
                    "synchronous {function} did not return an immediate movement result"
                ),
            }
        })();
        result.with_context(|| {
            format!(
                "world {} instance {} {movement:?} synchronous movement",
                self.world, id.index
            )
        })
    }

    /// Query the forward Q8 amount without translating. This requests animation,
    /// marks the shared forward-called flag and advances the actual startup
    /// counter. Translation suppression returns the adjusted unscaled amount.
    /// A following movement invocation must read the resulting live state.
    pub fn ordinary_forward_amount(
        &mut self,
        id: ScriptActorId,
        inputs: OrdinaryForwardAmountInputs,
        query: Option<OrdinaryObstacleQuery<'_>>,
        sounds: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<i32> {
        let result = (|| -> Result<i32> {
            let index = self.resolve(id)?;
            let actor = &self.actors[index];
            let control = actor
                .control
                .as_ref()
                .context("forward amount control unavailable")?;
            if control.rope_attached
                || control.sliding_state != 0
                || control.falling_state != 0
                || control.queued_motion != 0
                || control.living_state != 0
            {
                return Ok(0);
            }
            if (control.movement_flags >> 4) & 3 < 3
                && !self
                    .forward_step_clear(
                        id,
                        inputs.selected_player,
                        inputs.frame_scale as i32,
                        query,
                    )?
                    .context("forward amount step geometry unavailable")?
            {
                return Ok(0);
            }
            let deep = matches!(actor.surface, Some(2 | 4));
            let combat = control.combat.as_ref();
            let (group, attribute) =
                if inputs.holding_weapon || combat.is_some_and(|combat| combat.flags & 4 != 0) {
                    if combat.is_some_and(|combat| combat.action == 4) {
                        (106, 16)
                    } else {
                        (92, 17)
                    }
                } else {
                    (
                        if deep {
                            142
                        } else if control.torch_burning != 0 {
                            30
                        } else {
                            18
                        },
                        8,
                    )
                };
            let step = actor.attributes[attribute];
            let adjustment = actor.attributes[11];
            ensure!(
                actor
                    .animation
                    .as_ref()
                    .context("forward amount animation unavailable")?
                    .playback
                    .has_group(group),
                "forward amount animation group {group} unavailable"
            );
            self.actors[index]
                .control
                .as_mut()
                .context("forward amount control unavailable")?
                .motion_direction = 2;
            self.request_ordinary_group(id, group, false, inputs.ticks, sounds)?;
            let animation = &self.actors[index]
                .animation
                .as_ref()
                .context("forward amount animation unavailable")?
                .playback;
            if self.updates.animation_system && animation.current_group() != Some(group) {
                return Ok(i32::from(super::super::locomotion::continues(
                    inputs.duration,
                    inputs.elapsed,
                )));
            }
            let suppressed = animation.translation_suppressed();
            self.globals.locomotion_called |= 4;
            let amount = i32::from(step) << 8;
            let amount = if deep { amount >> 1 } else { amount };
            let amount = super::super::locomotion::initial_step_distance(
                &mut self.actors[index]
                    .control
                    .as_mut()
                    .context("forward amount control unavailable")?
                    .movement_flags,
                4,
                adjustment,
                amount,
            );
            Ok(if suppressed {
                amount
            } else {
                amount.wrapping_mul(inputs.frame_scale as i32) >> 8
            })
        })();
        result.with_context(|| format!("world {} instance {} forward amount", self.world, id.index))
    }
}

/// The movement a nonplayer actor's current animation group keeps going when the update made no
/// request: forward in 18, 30, 67 through 70, 75, 77, 92, 106, 130, 131 and 142; backward in 19,
/// 76, 78 and 93; the combat circle left in 94 and right in 95; the walk forward in 20, 21 and 31;
/// the walk backward in 22 and 23; the sidestep left in 24, 25 and 28 and right in 26, 27 and 29.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Continuation {
    Forward,
    Backward,
    Circle(super::super::locomotion::Strafe),
    Walk,
    WalkBackward,
    Sidestep(super::super::locomotion::Strafe),
}

impl Continuation {
    fn of(group: u16) -> Option<Self> {
        use super::super::locomotion::Strafe;
        match group {
            18 | 30 | 67..=70 | 75 | 77 | 92 | 106 | 130 | 131 | 142 => Some(Self::Forward),
            19 | 76 | 78 | 93 => Some(Self::Backward),
            94 => Some(Self::Circle(Strafe::Left)),
            95 => Some(Self::Circle(Strafe::Right)),
            20 | 21 | 31 => Some(Self::Walk),
            22 | 23 => Some(Self::WalkBackward),
            24 | 25 | 28 => Some(Self::Sidestep(Strafe::Left)),
            26 | 27 | 29 => Some(Self::Sidestep(Strafe::Right)),
            _ => None,
        }
    }

    /// The shared movement-called flag bit this step sets.
    fn called_flag(self) -> u8 {
        match self {
            Self::Forward | Self::Walk => 4,
            Self::Backward | Self::WalkBackward => 8,
            Self::Circle(strafe) | Self::Sidestep(strafe) => strafe.called_flag(),
        }
    }

    /// The motion direction the original stores for this step.
    fn direction(self) -> u8 {
        use super::super::locomotion::Strafe;
        match self {
            Self::Forward => 2,
            Self::Backward => 3,
            Self::Walk => 6,
            Self::WalkBackward => 7,
            Self::Sidestep(strafe) => strafe.motion_direction(),
            // The original stores 25 for the left circle and the right strafe's 5 for the right.
            Self::Circle(Strafe::Left) => 25,
            Self::Circle(Strafe::Right) => 5,
        }
    }
}

impl ScriptWorld {
    /// Continue an ordinary actor's movement after its script and decision stages. The step runs
    /// only when its movement was not called this update and the current animation group is one
    /// of the continuing groups. It requests no group, so an idle request made earlier in the
    /// update stays pending until the playing group finishes, and the actor keeps moving until
    /// then (preservation SOUP.md, movement continuation). The walk, walk-backward and sidestep
    /// steps need the ordinary collision geometry for their step check and make no step without
    /// it. Swimming actors and the sliding and ledge variants are not continued (#52).
    pub fn continue_ordinary_motion(
        &mut self,
        id: ScriptActorId,
        inputs: super::ScriptInputs<'_>,
        query: Option<OrdinaryObstacleQuery<'_>>,
        sounds: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<()> {
        use super::super::locomotion::{self, Strafe};
        use super::hazard::OrdinaryStepDirection;
        let result = (|| -> Result<()> {
            let index = self.resolve(id)?;
            // The held-weapon predicate needs the item owner; update passes without one
            // (timer and inspection passes) make no continuation step.
            if inputs.hud.is_none() {
                return Ok(());
            }
            let frame_scale = inputs
                .frame_scale
                .context("movement continuation frame_scale unavailable")?
                as i32;
            let actor = &self.actors[index];
            let (Some(control), Some(animation)) =
                (actor.control.as_ref(), actor.animation.as_ref())
            else {
                return Ok(());
            };
            let group = animation.playback.current_group();
            let Some(step) = group.and_then(Continuation::of) else {
                return Ok(());
            };
            if self.globals.locomotion_called & step.called_flag() != 0
                || control.rope_attached
                || control.sliding_state != 0
                || control.falling_state != 0
                || control.queued_motion != 0
                || control.living_state != 0
                || matches!(actor.surface, Some(2 | 4))
            {
                return Ok(());
            }
            let suppressed = animation.playback.translation_suppressed();
            let combat_stance = control
                .combat
                .as_ref()
                .is_some_and(|combat| combat.flags & 4 != 0);
            let adjustment = actor.attributes[11];
            let counters = control.movement_flags;
            let (selected_player, holding_weapon) = self.spacing_weapon(id, inputs)?;
            let stance = holding_weapon || combat_stance;
            // The walk, walk-backward and sidestep steps store their direction before the check.
            if matches!(
                step,
                Continuation::Walk | Continuation::WalkBackward | Continuation::Sidestep(_)
            ) {
                self.actors[index]
                    .control
                    .as_mut()
                    .context("movement continuation control unavailable")?
                    .motion_direction = step.direction();
            }
            match step {
                Continuation::Forward => {
                    if (counters >> 4) & 3 < 3
                        && !self
                            .forward_step_clear(id, selected_player, frame_scale, query)?
                            .context("movement continuation step geometry unavailable")?
                    {
                        return Ok(());
                    }
                }
                Continuation::Backward => {
                    if stance
                        && let Some(query) = query
                        && self.blocked_ahead(id, 30, query)?
                    {
                        return Ok(());
                    }
                }
                // Combat circling runs no lateral step check, as its native does not.
                Continuation::Circle(_) => {}
                Continuation::Walk => {
                    if !self
                        .forward_step_clear(id, selected_player, frame_scale, query)?
                        .context("movement continuation step geometry unavailable")?
                    {
                        return Ok(());
                    }
                }
                Continuation::WalkBackward | Continuation::Sidestep(_) => {
                    let Some(geometry) = inputs.locomotion_geometry else {
                        return Ok(());
                    };
                    let direction = match step {
                        Continuation::Sidestep(Strafe::Left) => OrdinaryStepDirection::Left,
                        Continuation::Sidestep(Strafe::Right) => OrdinaryStepDirection::Right,
                        _ => OrdinaryStepDirection::Backward,
                    };
                    if !self.ordinary_step_clear(
                        id,
                        direction,
                        geometry.ground,
                        geometry.environment,
                        sounds,
                    )? {
                        return Ok(());
                    }
                }
            }
            let attributes = &self.actors[index].attributes;
            // A zero walk step length falls back to half of the plain step byte, unshifted.
            let half = |plain: u8| i32::from(plain) >> 1;
            let (step_length, shift) = match step {
                Continuation::Forward if stance => (i32::from(attributes[17]) << 8, 4),
                Continuation::Forward if matches!(group, Some(75 | 77)) => {
                    (i32::from(attributes[8]) << 7, 4)
                }
                Continuation::Forward => (i32::from(attributes[8]) << 8, 4),
                Continuation::Backward if stance => (i32::from(attributes[18]) << 8, 6),
                Continuation::Backward if matches!(group, Some(76 | 78)) => {
                    (i32::from(attributes[8]) << 7, 6)
                }
                Continuation::Backward => (i32::from(attributes[9]) << 8, 6),
                Continuation::Circle(strafe) => {
                    (i32::from(attributes[83]) << 8, strafe.counter_shift())
                }
                Continuation::Walk => {
                    let walk = if stance {
                        attributes[19]
                    } else {
                        attributes[12]
                    };
                    let length = if walk == 0 {
                        half(attributes[8])
                    } else {
                        i32::from(walk) << 8
                    };
                    (length, 4)
                }
                Continuation::WalkBackward => {
                    let walk = if stance {
                        attributes[20]
                    } else {
                        attributes[13]
                    };
                    let length = if walk == 0 {
                        half(attributes[9])
                    } else {
                        i32::from(walk) << 8
                    };
                    (length, 6)
                }
                Continuation::Sidestep(strafe) => {
                    (i32::from(attributes[10]) << 8, strafe.counter_shift())
                }
            };
            let turn = attributes[84];
            let control = self.actors[index]
                .control
                .as_mut()
                .context("movement continuation control unavailable")?;
            control.motion_direction = step.direction();
            self.globals.locomotion_called |= step.called_flag();
            let distance = locomotion::initial_step_distance(
                &mut control.movement_flags,
                shift,
                adjustment,
                step_length,
            );
            let spatial = self
                .spatial
                .as_mut()
                .context("movement continuation spatial state unavailable")?
                .actor_mut(index)?;
            let movement = match step {
                Continuation::Forward | Continuation::Walk => locomotion::forward_translation(
                    spatial.local.angles[1],
                    distance,
                    frame_scale,
                    suppressed,
                ),
                Continuation::Backward | Continuation::WalkBackward => {
                    locomotion::backward_translation(
                        spatial.local.angles[1],
                        distance,
                        frame_scale,
                        suppressed,
                    )
                }
                Continuation::Circle(strafe) => {
                    let (angle, movement) = locomotion::strafe_motion(
                        strafe,
                        spatial.local.angles[1],
                        turn,
                        distance,
                        frame_scale,
                        suppressed,
                    );
                    spatial.local.angles[1] = angle;
                    movement
                }
                Continuation::Sidestep(strafe) => locomotion::strafe_translation(
                    strafe,
                    spatial.local.angles[1],
                    distance,
                    frame_scale,
                    suppressed,
                ),
            };
            if let Some((heading, distance)) = movement {
                spatial.local.position = cyrus_world::transform::translate_forward(
                    spatial.local.position,
                    heading,
                    distance,
                )?;
            }
            Ok(())
        })();
        result.with_context(|| {
            format!(
                "world {} instance {} movement continuation",
                self.world, id.index
            )
        })
    }
}
