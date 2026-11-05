//! Ordinary slope response follows preservation's player engine rules.
use super::{ScriptActorId, ScriptWorld, ground::OrdinaryGroundQuery};
use crate::script::locomotion::StepDirection;
use anyhow::{Context, Result, ensure};
use cyrus_collision::ground::SelectedSupport;
use cyrus_script::{AnimationGroup, Attribute};

/// A class-two slide decision in Q8 distance and 1/2048-turn headings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OrdinarySlide {
    pub movement_heading: u16,
    pub target_heading: u16,
    pub group: AnimationGroup,
    /// Direction whose `move_counter` field the slide saturates and whose called flag it sets.
    pub step: StepDirection,
    pub distance: i32,
}

fn difference(first: u16, second: u16) -> i32 {
    let delta = i32::from(first & 2047) - i32::from(second & 2047);
    if delta > 1024 {
        delta - 2048
    } else if delta < -1024 {
        delta + 2048
    } else {
        delta
    }
}

/// Resolve class-two motion from the support normal and effective heading.
/// Settings loading clamps slide speed before the wrapping distance product.
pub fn ordinary_slide(
    normal: [i32; 3],
    effective_heading: u16,
    torch: bool,
    slide_speed: i32,
    frame_scale: u32,
) -> OrdinarySlide {
    let movement_heading =
        crate::player::motion::displacement_heading(i64::from(normal[0]), i64::from(normal[2]));
    let forward = difference(movement_heading, effective_heading).abs() < 512;
    OrdinarySlide {
        movement_heading,
        target_heading: (movement_heading + if forward { 0 } else { 1024 }) & 2047,
        group: match (forward, torch) {
            (true, false) => AnimationGroup::SLIDE,
            (true, true) => AnimationGroup::SLIDE_FORWARD_TORCH,
            (false, false) => AnimationGroup::SLIDE_BACKWARD,
            (false, true) => AnimationGroup::SLIDE_BACKWARD_TORCH,
        },
        step: if forward {
            StepDirection::Forward
        } else {
            StepDirection::Backward
        },
        distance: slide_speed
            .max(8)
            .wrapping_mul(128)
            .wrapping_mul(frame_scale as i32)
            >> 8,
    }
}

/// One local-heading update and its optional ordinary turn animation request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OrdinarySlopeTurn {
    pub heading: u16,
    pub group: Option<AnimationGroup>,
}

/// Turn rate is clamped after frame scaling. Weapon state chooses animation;
/// combat mode independently chooses which installed attribute supplies rate.
pub fn ordinary_slope_turn(
    local_heading: u16,
    target: u16,
    rate_attribute: u8,
    frame_scale: u32,
    holding_weapon: bool,
    torch: bool,
) -> OrdinarySlopeTurn {
    let delta = -difference(local_heading, target);
    let rate = (i32::from(rate_attribute).wrapping_mul(frame_scale as i32) >> 8).max(8);
    if delta.abs() <= rate {
        return OrdinarySlopeTurn {
            heading: target & 2047,
            group: None,
        };
    }
    let positive = delta > 0;
    let group = match (holding_weapon, positive, torch) {
        (true, true, _) => AnimationGroup::FIGHT_TURN_LEFT,
        (true, false, _) => AnimationGroup::FIGHT_TURN_RIGHT,
        (false, true, false) => AnimationGroup::TURN_LEFT,
        (false, true, true) => AnimationGroup::TURN_LEFT_TORCH,
        (false, false, false) => AnimationGroup::TURN_RIGHT,
        (false, false, true) => AnimationGroup::TURN_RIGHT_TORCH,
    };
    OrdinarySlopeTurn {
        heading: (i32::from(local_heading).wrapping_add(if positive { rate } else { -rate }) & 2047)
            as u16,
        group: Some(group),
    }
}

/// Caller-established predicates and timing for nonplayer slope and facing operations.
/// Combat mode is separate from the existence of a combat record. The caller
/// supplies the weapon predicate after item/shell services and actor overrides.
pub struct OrdinarySlopeInputs {
    pub selected_player: Option<ScriptActorId>,
    pub holding_weapon: bool,
    pub combat_active: bool,
    pub frame_scale: u32,
    pub ticks: u32,
}

impl ScriptWorld {
    /// Face a target from local Q8 X/Z, following preservation's combat-maintenance
    /// facing rule. Arrival snaps X/Z and registers a deferred map update without
    /// changing Y or heading. Otherwise return the signed pre-turn difference,
    /// or zero when the ordinary turn snaps. Selected-player camera feedback
    /// requires its own owner and is rejected before writes.
    pub fn face_ordinary_target(
        &mut self,
        id: ScriptActorId,
        target: ScriptActorId,
        inputs: OrdinarySlopeInputs,
        sounds: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<i32> {
        let result = (|| -> Result<_> {
            self.resolve(id)?;
            self.resolve(target)?;
            ensure!(
                inputs.selected_player != Some(id),
                "selected-player facing camera unavailable"
            );
            let destination = self.spatial(target)?.local.position;
            self.face_ordinary_point(id, destination, inputs, sounds)
        })();
        result.with_context(|| {
            format!(
                "world {} instance {} target {} facing",
                self.world, id.index, target.index
            )
        })
    }

    /// Face a retained local Q8 point, snapping only X/Z on horizontal arrival.
    /// Y is ignored. Uses the same speed, turn and registration owners as target facing.
    /// Selected-player camera feedback remains unavailable.
    pub fn face_ordinary_point(
        &mut self,
        id: ScriptActorId,
        destination: [i32; 3],
        inputs: OrdinarySlopeInputs,
        sounds: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<i32> {
        let result = (|| -> Result<i32> {
            let index = self.resolve(id)?;
            ensure!(
                inputs.selected_player != Some(id),
                "selected-player facing camera unavailable"
            );
            let speed = self.ordinary_group_speed(index);
            let origin = self.spatial(id)?.local;
            let x = destination[0].wrapping_sub(origin.position[0]);
            let z = destination[2].wrapping_sub(origin.position[2]);
            let threshold =
                ((i32::from(speed) + 1) << 8).wrapping_mul(inputs.frame_scale as i32) >> 8;
            if self.snap_ordinary_horizontal(index, destination, threshold)? {
                return Ok(0);
            }
            self.turn_ordinary_toward(
                id,
                super::super::spatial::query_bearing(x, z),
                inputs,
                sounds,
            )
        })();
        result.with_context(|| {
            format!(
                "world {} instance {} point {destination:?} facing",
                self.world, id.index
            )
        })
    }

    /// One ordinary turn step toward a heading, without an arrival snap. Returns the signed
    /// pre-turn difference, or zero when the turn snaps onto the heading. Selected-player camera
    /// feedback remains unavailable.
    pub fn face_ordinary_angle(
        &mut self,
        id: ScriptActorId,
        heading: u16,
        inputs: OrdinarySlopeInputs,
        sounds: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<i32> {
        self.resolve(id)?;
        ensure!(
            inputs.selected_player != Some(id),
            "selected-player facing camera unavailable"
        );
        self.turn_ordinary_toward(id, heading, inputs, sounds)
            .with_context(|| {
                format!(
                    "world {} instance {} heading {heading} facing",
                    self.world, id.index
                )
            })
    }

    pub(super) fn turn_ordinary_toward(
        &mut self,
        id: ScriptActorId,
        desired: u16,
        inputs: OrdinarySlopeInputs,
        sounds: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<i32> {
        let index = self.resolve(id)?;
        let origin = self.spatial(id)?.local;
        let actor = &self.actors[index];
        let torch = actor
            .control
            .as_ref()
            .context("facing control unavailable")?
            .torch_burning
            != 0;
        let turn = ordinary_slope_turn(
            origin.angles[1],
            desired,
            actor.attributes[if inputs.holding_weapon || inputs.combat_active {
                Attribute::FightTurnSpeed
            } else {
                Attribute::TurnSpeed
            }],
            inputs.frame_scale,
            inputs.holding_weapon,
            torch,
        );
        if actor.animation.is_some()
            && let Some(group) = turn.group
        {
            self.request_ordinary_group(id, group, false, inputs.ticks, sounds)?;
        }
        self.spatial
            .as_mut()
            .context("spatial owner unavailable")?
            .actor_mut(index)?
            .local
            .angles[1] = turn.heading;
        if inputs.selected_player == Some(id) {
            // The selected player's turn rebuilds its matrix and copies the angles to history.
            self.spatial
                .as_mut()
                .context("spatial owner unavailable")?
                .actor_mut(index)?
                .reset_rotation([false; 3])?;
        }
        Ok(if turn.group.is_none() {
            0
        } else {
            super::super::decision::heading_difference(
                i32::from(desired),
                i32::from(origin.angles[1]),
            )
        })
    }

    pub(super) fn snap_ordinary_horizontal(
        &mut self,
        index: usize,
        destination: [i32; 3],
        threshold: i32,
    ) -> Result<bool> {
        let spatial = self
            .spatial
            .as_mut()
            .context("spatial owner unavailable")?
            .actor_mut(index)?;
        let position = &mut spatial.local.position;
        let distance = cyrus_collision::distance::rounded_distance([
            destination[0].wrapping_sub(position[0]),
            0,
            destination[2].wrapping_sub(position[2]),
        ])?;
        if distance > threshold {
            return Ok(false);
        }
        position[0] = destination[0];
        position[2] = destination[2];
        if !self.map_updates.contains(&index) {
            self.map_updates.push(index);
        }
        Ok(true)
    }

    pub(super) fn ordinary_group_speed(&self, index: usize) -> u8 {
        let actor = &self.actors[index];
        let group = actor
            .animation
            .as_ref()
            .and_then(|animation| animation.playback.current_group());
        let attribute = match group {
            Some(
                AnimationGroup::RUN | AnimationGroup::RUN_TORCH | AnimationGroup::SWIM_FORWARD,
            ) => Attribute::ForwardSpeed,
            Some(AnimationGroup::RUN_BACKWARD | AnimationGroup::SWIM_BACKWARD) => {
                Attribute::BackwardSpeed
            }
            Some(AnimationGroup::WALK | AnimationGroup::WALK_FORWARD_TORCH) => {
                Attribute::ForwardWalk
            }
            Some(AnimationGroup::WALK_BACKWARD | AnimationGroup::WALK_BACKWARD_TORCH) => {
                Attribute::BackwardWalk
            }
            Some(
                AnimationGroup::STEP_LEFT
                | AnimationGroup::STEP_LEFT_TORCH
                | AnimationGroup::STEP_RIGHT
                | AnimationGroup::STEP_RIGHT_TORCH,
            ) => Attribute::SideSpeed,
            Some(AnimationGroup::FIGHT_RUN) => Attribute::FightForwardWalk,
            Some(AnimationGroup::FIGHT_FORW | AnimationGroup::FIGHT_BACK) => {
                Attribute::FightForwardSpeed
            }
            _ => return 0,
        };
        actor.attributes[attribute]
    }

    /// Test arrival at a local Q8 point using integer-map-unit distance and the
    /// receiver's positive Y bounds extent. The strict speed threshold is not
    /// frame-scaled. Admission quantizes XYZ to whole units and registers movement;
    /// rejection preserves position. This is an arrival snap, not pathfinding.
    pub fn arrive_ordinary_point(
        &mut self,
        id: ScriptActorId,
        destination: [i32; 3],
    ) -> Result<bool> {
        let result = (|| -> Result<_> {
            let index = self.resolve(id)?;
            let origin = self.spatial(id)?.local.position;
            let height = self.ordinary_collision_shape(id)?.positive_extent[1];
            let differences = [
                (destination[0] >> 8).wrapping_sub(origin[0] >> 8),
                (destination[1] >> 8).wrapping_sub(height.wrapping_add(origin[1] >> 8)),
                (destination[2] >> 8).wrapping_sub(origin[2] >> 8),
            ];
            let distance = cyrus_collision::distance::rounded_distance(differences)?;
            let threshold = i32::from(self.ordinary_group_speed(index)).wrapping_mul(384) >> 8;
            if distance >= threshold {
                return Ok(false);
            }
            self.spatial
                .as_mut()
                .context("spatial owner unavailable")?
                .actor_mut(index)?
                .local
                .position = destination.map(|value| (value >> 8) << 8);
            if !self.map_updates.contains(&index) {
                self.map_updates.push(index);
            }
            Ok(true)
        })();
        result.with_context(|| {
            format!(
                "world {} instance {} point {destination:?} arrival",
                self.world, id.index
            )
        })
    }

    pub(super) fn request_ordinary_group(
        &mut self,
        id: ScriptActorId,
        group: AnimationGroup,
        force: bool,
        ticks: u32,
        sounds: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<()> {
        let index = self.resolve(id)?;
        let position = super::animation_sound_position(self.spatial.as_ref(), index);
        let event_context = super::effects::ActorEventContext::capture(self, id);
        let event_camera = &mut self.camera;
        let event_effects = &mut self.effects;
        let actor = &mut self.actors[index];
        let surface = actor.surface;
        let animation = actor
            .animation
            .as_mut()
            .context("ordinary animation unavailable")?;
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
        let result = if force {
            animation
                .playback
                .force_request_with_sounds(group, 0, ticks, &mut emit)
        } else {
            animation
                .playback
                .request_with_sounds(group, 0, ticks, &mut emit)
        };
        match result {
            Ok(frame) => animation.frame = frame.or(animation.frame),
            Err(error) => {
                let message = format!(
                    "world {} instance {index} ordinary animation: {error:#}",
                    self.world
                );
                actor.animation_failure = Some(message.clone());
                anyhow::bail!(message);
            }
        }
        Ok(())
    }

    /// Execute the admitted slope helper, including ordered turn/slide requests,
    /// translation and a second primary support query. The selected player's turn also
    /// rebuilds its rotation matrix and angle history.
    pub fn resolve_ordinary_slope(
        &mut self,
        id: ScriptActorId,
        support: Option<&SelectedSupport>,
        query: OrdinaryGroundQuery<'_>,
        inputs: OrdinarySlopeInputs,
        sounds: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<()> {
        self.require_collision_query(id, query.prepared, query.scene, query.animations)?;
        let index = self.resolve(id)?;
        ensure!(
            !matches!(support, Some(SelectedSupport::Fallback { .. })),
            "world {} instance {index}: ordinary slope requires primary support",
            self.world
        );
        let actor = &self.actors[index];
        let animation = actor
            .animation
            .as_ref()
            .context("ordinary slope animation unavailable")?;
        let control = actor
            .control
            .as_ref()
            .context("ordinary slope control unavailable")?;
        let torch = control.torch_burning != 0;
        let Some(support) = support.and_then(|support| match support {
            SelectedSupport::Primary { support, .. } if support.classification() == 2 => {
                Some(support)
            }
            _ => None,
        }) else {
            if !animation.playback.current_group().is_some_and(|group| {
                (AnimationGroup::SLIDE..=AnimationGroup::SLIDE_BACKWARD_TORCH).contains(&group)
            }) {
                self.request_ordinary_group(
                    id,
                    if torch {
                        AnimationGroup::SLIDE_FORWARD_TORCH
                    } else {
                        AnimationGroup::SLIDE
                    },
                    false,
                    inputs.ticks,
                    sounds,
                )?;
            }
            return Ok(());
        };
        let slide_speed = query
            .scene
            .settings
            .slide_speed
            .context("ordinary slope slide_speed absent; re-export world")?;
        let first = control.sliding_state == 0;
        let spatial = self.spatial(id)?;
        let slide = ordinary_slide(
            support.normal,
            spatial.query_pose()?.angles[1],
            torch,
            slide_speed,
            inputs.frame_scale,
        );
        let ticks = inputs.ticks;
        self.turn_ordinary_toward(id, slide.target_heading, inputs, sounds)?;
        self.request_ordinary_group(id, slide.group, first, ticks, sounds)?;
        self.actors[index]
            .control
            .as_mut()
            .expect("validated slope control")
            .move_counter |= slide.step.counter_mask();
        self.globals.locomotion_called |= slide.step.called_flag();
        if slide.distance > 0 {
            let position = cyrus_world::transform::translate_forward(
                self.spatial(id)?.local.position,
                slide.movement_heading,
                slide.distance,
            )?;
            self.spatial
                .as_mut()
                .context("ordinary slope spatial state unavailable")?
                .actor_mut(index)?
                .local
                .position = position;
        }
        let extent = self.ordinary_collision_shape(id)?.positive_extent[1];
        let standing_height = query.scene.settings.standing_height;
        if let Some(support) = self.query_ordinary_ground(id, query)? {
            let height = i32::try_from(support.actor_height(extent, standing_height))
                .context("ordinary slope origin outside Q8 range")?;
            self.spatial
                .as_mut()
                .context("ordinary slope spatial state unavailable")?
                .actor_mut(index)?
                .local
                .position[1] = height;
        }
        let control = self.actors[index]
            .control
            .as_mut()
            .expect("validated slope control");
        if first {
            control.sliding_distance = 0;
        }
        control.sliding_distance = control.sliding_distance.wrapping_add(slide.distance);
        control.sliding_state = 1;
        Ok(())
    }
}
