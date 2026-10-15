//! Obstacle motion transitions follow preservation's ordinary forward wrapper.
use super::{ScriptActorId, ScriptWorld, obstacle::OrdinaryObstacleQuery};
use anyhow::{Context, Result, ensure};

/// Current motion bytes and weapon predicate for a single original transition.
/// Logical Y is Q8; attribute 32 is its unsigned installed map-unit offset.
#[derive(Clone, Copy, Debug)]
pub struct ObstacleTransitionInput {
    pub living_state: u8,
    pub falling_state: u8,
    pub queued_motion: u8,
    pub logical_y: i32,
    pub fall_attribute: u8,
    pub holding_weapon: bool,
    pub current_group: Option<u16>,
}

/// Newly started fall bookkeeping. The group is a forced request, not a default.
#[derive(Debug, PartialEq, Eq)]
pub struct ObstacleFallStart {
    pub origin_y: i32,
    pub timer: u8,
    pub force_group: Option<u16>,
}

/// Original byte writes and newly added result bits. Unmentioned bookkeeping
/// retains its previous value; the host ORs these bits into existing flags.
#[derive(Debug, PartialEq, Eq)]
pub struct ObstacleTransition {
    pub living_state: u8,
    pub falling_state: u8,
    pub queued_motion: u8,
    pub body_response_flags: u8,
    pub clear_motion_vector: bool,
    pub fall_start: Option<ObstacleFallStart>,
}

fn interrupted(living: u8, queued: u8) -> bool {
    (27..=32).contains(&living) || queued == 24
}

/// Compute the original state transition. State fifteen does nothing. Ordinary
/// grounded state zero with neither queued nor falling motion also stays put.
pub fn ordinary_obstacle_transition(input: ObstacleTransitionInput) -> Option<ObstacleTransition> {
    if input.living_state == 15 {
        return None;
    }
    let clear = interrupted(input.living_state, input.queued_motion);
    let living = if clear { 0 } else { input.living_state };
    let queued = if clear { 1 } else { input.queued_motion };
    let mut result = ObstacleTransition {
        living_state: living,
        falling_state: input.falling_state,
        queued_motion: queued,
        body_response_flags: 0,
        clear_motion_vector: clear,
        fall_start: None,
    };
    if queued == 0 {
        if input.falling_state != 0 {
            result.body_response_flags = 0x20;
            result.falling_state = (input.falling_state & 0x80) | 1;
        }
    } else {
        result.body_response_flags = 0x10;
        result.falling_state = (queued & 0x80) | 1;
        result.queued_motion = 0;
        result.fall_start = Some(ObstacleFallStart {
            origin_y: input
                .logical_y
                .wrapping_add(i32::from(input.fall_attribute) * 256),
            timer: 30,
            force_group: if result.falling_state & 0x80 != 0 {
                Some(58)
            } else if !input.holding_weapon {
                Some(56)
            } else if input.current_group != Some(113) {
                Some(113)
            } else {
                None
            },
        });
    }
    Some(result)
}

/// Caller-established wrapper globals. Auto-grab is true only for installed
/// setting value one. None explicitly establishes that no selected player is
/// in this registry. Ticks reuse the current shared animation increment.
#[derive(Clone, Copy, Debug)]
pub struct ObstacleEnvironment {
    pub selected_player: Option<ScriptActorId>,
    pub auto_grab: bool,
    pub item_system_ready: bool,
    pub shell_execution: bool,
    pub ticks: u32,
}

impl ScriptWorld {
    /// Commit one motion transition and any forced animation through the actor's
    /// synchronous sound path. Static models and disabled animation skip requests.
    /// Existing fall bookkeeping is untouched unless queued motion starts a fall.
    pub fn apply_ordinary_obstacle_transition(
        &mut self,
        id: ScriptActorId,
        data: &cyrus_assets::animation::Animations,
        environment: ObstacleEnvironment,
        sounds: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<()> {
        self.require_model_identity(data)?;
        let index = self.resolve(id)?;
        let actor = &self.actors[index];
        ensure!(
            actor.ordinary_collision_role(),
            "world {} instance {index}: ordinary obstacle transition object path unavailable",
            self.world
        );
        let control = actor
            .control
            .as_ref()
            .context("ordinary obstacle transition control unavailable")?;
        let group = actor
            .animation
            .as_ref()
            .and_then(|animation| animation.playback.current_group());
        let living = if interrupted(control.living_state, control.queued_motion) {
            0
        } else {
            control.living_state
        };
        let (holding, _, _) = super::super::activation::weapon_predicates(
            living,
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
        let Some(transition) = ordinary_obstacle_transition(ObstacleTransitionInput {
            living_state: control.living_state,
            falling_state: control.falling_state,
            queued_motion: control.queued_motion,
            logical_y: self.spatial(id)?.local.position[1],
            fall_attribute: actor.attributes[32],
            holding_weapon: holding,
            current_group: group,
        }) else {
            return Ok(());
        };
        let animated = match data.actor(actor.name())?["model_load_path"].as_str() {
            Some("animation_table") => true,
            Some("static" | "none") => false,
            path => anyhow::bail!(
                "world {} instance {index}: obstacle model load path {path:?} unavailable",
                self.world
            ),
        };
        let position = super::animation_sound_position(self.spatial.as_ref(), index);
        let actor = &mut self.actors[index];
        let control = actor
            .control
            .as_mut()
            .context("ordinary obstacle transition control unavailable")?;
        control.living_state = transition.living_state;
        control.falling_state = transition.falling_state;
        control.queued_motion = transition.queued_motion;
        control.body_response_flags |= transition.body_response_flags;
        if transition.clear_motion_vector {
            control.motion_vector = [0; 3];
        }
        if let Some(fall) = transition.fall_start {
            control.fall_origin_y = fall.origin_y;
            control.fall_timer = fall.timer;
            if let Some(group) = fall
                .force_group
                .filter(|_| self.updates.animation_system && animated)
            {
                let animation = actor.animation.as_mut().with_context(|| {
                    format!(
                        "world {} instance {index}: obstacle animation receiver not prepared",
                        self.world
                    )
                })?;
                let surface = actor.surface;
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
                            "world {} instance {index} obstacle animation: {error:#}",
                            self.world
                        );
                        actor.animation_failure = Some(message.clone());
                        anyhow::bail!(message);
                    }
                }
            }
        }
        Ok(())
    }

    /// Complete the retained actor-style overlap pass and its required obstacle
    /// repeat. Tiny actor-pair contact ends the pass without the repeat, even
    /// after earlier separation. Other late response stages remain separate.
    pub fn resolve_ordinary_overlap_stage(
        &mut self,
        id: ScriptActorId,
        query: OrdinaryObstacleQuery<'_>,
        environment: ObstacleEnvironment,
        sounds: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<()> {
        let pass =
            self.resolve_ordinary_overlaps(id, query.scene, query.animations, query.prepared)?;
        if matches!(
            pass,
            super::overlap::OrdinaryOverlapPass::Complete {
                needs_polygon_repair: true
            }
        ) {
            self.resolve_ordinary_obstacle_wrapper(id, query, environment, sounds)?;
        }
        Ok(())
    }

    /// Run the three original forward probes and their motion transitions using
    /// one retained candidate record. Selected-player auto-grab needs its own
    /// gameplay owner and remains explicitly unavailable when a responding hit
    /// reaches that service. Non-player actors use the full ordinary wrapper.
    pub fn resolve_ordinary_obstacle_wrapper(
        &mut self,
        id: ScriptActorId,
        query: OrdinaryObstacleQuery<'_>,
        environment: ObstacleEnvironment,
        sounds: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<()> {
        self.require_collision_query(id, query.prepared, query.scene, query.animations)?;
        if let Some(player) = environment.selected_player {
            self.resolve(player)?;
        }
        let post_height = query.scene.settings.post_collide_height as i32;
        let offsets = [
            0,
            self.ordinary_collision_shape(id)?.positive_extent[1].wrapping_sub(post_height >> 2),
            self.ordinary_collision_shape(id)?.positive_extent[1].wrapping_sub(post_height),
        ];
        for (stage, offset) in offsets.into_iter().enumerate() {
            if stage == 1 {
                let control = self.actors[id.index]
                    .control
                    .as_ref()
                    .context("ordinary obstacle wrapper control unavailable")?;
                if control.queued_motion == 0 && control.falling_state == 0 {
                    continue;
                }
            }
            if self.resolve_ordinary_forward_obstacle(
                id,
                OrdinaryObstacleQuery {
                    vertical_offset: offset,
                    respond: stage != 1,
                    ..query
                },
            )? {
                if stage != 1 {
                    self.actors[id.index]
                        .control
                        .as_mut()
                        .context("ordinary obstacle wrapper control unavailable")?
                        .body_response_flags |= 8;
                    ensure!(
                        !(environment.auto_grab && environment.selected_player == Some(id)),
                        "world {} instance {}: selected-player obstacle auto-grab service unavailable",
                        self.world,
                        id.index
                    );
                }
                self.apply_ordinary_obstacle_transition(id, query.animations, environment, sounds)?;
            }
        }
        Ok(())
    }
}
