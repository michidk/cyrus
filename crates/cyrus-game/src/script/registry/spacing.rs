//! Fight spacing reads and mutates the receiver's shared movement state.
use super::{ScriptActorId, ScriptWorld, obstacle::OrdinaryObstacleQuery};
use anyhow::{Context, Result, ensure};

/// Existing movement executors available to ordinary combat spacing.
#[derive(Clone, Copy, Debug)]
pub enum OrdinaryMovement {
    Forward,
    WalkForward,
    Backward,
    Left,
    Right,
}
impl OrdinaryMovement {
    fn native_name(self) -> &'static str {
        match self {
            Self::Forward => "MoveForward",
            Self::WalkForward => "WalkForward",
            Self::Backward => "MoveBackward",
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
