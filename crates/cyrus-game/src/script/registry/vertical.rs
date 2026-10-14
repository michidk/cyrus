//! Queued and falling vertical arithmetic retains the original byte timer.
use super::{ScriptActorId, ScriptWorld};
use anyhow::{Context, Result};

/// Factor and clock inputs follow preservation's vertical motion rules;
/// the caller supplies the entry for the current timer and the entry after an
/// ascending advance. Falling with a nonpositive timer uses entry zero instead.
#[derive(Clone, Copy, Debug)]
pub struct OrdinaryVerticalInputs {
    pub factor: i32,
    pub advanced_factor: i32,
    pub fine_ticks: u32,
    pub additional_ticks: u32,
    pub post_collide_height: u32,
    pub model_controls_vertical: bool,
}

/// Resolve the documented fixed factor sequence without copying engine table data.
/// Only indices zero through 69 are factors; later or negative indices are unsupported.
pub fn ordinary_vertical_factor(index: i8) -> Result<i32> {
    anyhow::ensure!(
        (0..70).contains(&index),
        "ordinary vertical factor index {index}: outside supported factor sequence"
    );
    let index = i32::from(index);
    Ok(if index < 30 {
        let remaining = 29 - index;
        256 * (900 - remaining * remaining) / 900
    } else {
        256
    })
}

impl OrdinaryVerticalInputs {
    /// Resolve both queued factor reads with the original signed byte indexing.
    /// Model-controlled motion skips all factor reads, including unsupported indices.
    pub fn for_queued(
        timer: u8,
        fine_ticks: u32,
        additional_ticks: u32,
        post_collide_height: u32,
        model_controls_vertical: bool,
    ) -> Result<Self> {
        let advanced = timer.wrapping_add(fine_ticks.wrapping_add(additional_ticks) as u8);
        Ok(Self {
            factor: if model_controls_vertical {
                0
            } else {
                ordinary_vertical_factor(timer as i8)?
            },
            advanced_factor: if model_controls_vertical {
                0
            } else {
                ordinary_vertical_factor(advanced as i8)?
            },
            fine_ticks,
            additional_ticks,
            post_collide_height,
            model_controls_vertical,
        })
    }

    /// Nonpositive signed falling timers resolve entry zero; positive timers
    /// resolve their current entry. No advanced-factor read occurs in falling.
    pub fn for_falling(
        timer: u8,
        fine_ticks: u32,
        additional_ticks: u32,
        post_collide_height: u32,
        model_controls_vertical: bool,
    ) -> Result<Self> {
        Ok(Self {
            factor: if model_controls_vertical {
                0
            } else {
                ordinary_vertical_factor((timer as i8).max(0))?
            },
            advanced_factor: 0,
            fine_ticks,
            additional_ticks,
            post_collide_height,
            model_controls_vertical,
        })
    }
}

/// Retained logical Q8 position, byte timer and queued/falling state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OrdinaryVerticalState {
    pub y: i32,
    pub origin_y: i32,
    pub timer: u8,
    pub queued: u8,
    pub falling: u8,
}

/// Apply one original ascending arithmetic stage, without animation dispatch.
pub fn ordinary_queued_vertical(
    mut state: OrdinaryVerticalState,
    attribute_32: u8,
    input: OrdinaryVerticalInputs,
) -> OrdinaryVerticalState {
    if input.model_controls_vertical {
        return state;
    }
    state.y = state
        .origin_y
        .wrapping_sub(i32::from(attribute_32).wrapping_mul(input.factor));
    state.timer = state
        .timer
        .wrapping_add(input.fine_ticks.wrapping_add(input.additional_ticks) as u8);
    if input.advanced_factor > 255 {
        state.falling = state.queued;
        state.queued = 0;
    }
    state
}

/// Apply one original falling arithmetic stage. `factor` is entry zero when
/// the signed timer is nonpositive; otherwise it is the current timer entry.
pub fn ordinary_falling_vertical(
    mut state: OrdinaryVerticalState,
    attribute_32: u8,
    input: OrdinaryVerticalInputs,
) -> OrdinaryVerticalState {
    if input.model_controls_vertical {
        return state;
    }
    let limit = input
        .post_collide_height
        .wrapping_mul(512)
        .wrapping_sub(256) as i32;
    let ticks = input.fine_ticks.wrapping_add(input.additional_ticks);
    let scaled = i32::from(attribute_32).wrapping_mul(input.factor);
    if (state.timer as i8) > 0 {
        state.y = state
            .origin_y
            .wrapping_add(scaled.wrapping_neg().min(limit));
        state.timer = state.timer.wrapping_sub(ticks as u8);
    } else {
        state.y = state
            .y
            .wrapping_add(scaled.wrapping_mul(ticks.wrapping_add(2) as i32).min(limit));
    }
    state
}

impl ScriptWorld {
    /// Resolve queued-motion clearance from retained face models and dry terrain.
    /// Water and terrain boundaries remain unsupported.
    /// This is not the ordinary ground-support rank selection or player fallback.
    pub fn query_ordinary_queued_clearance(
        &self,
        id: ScriptActorId,
        query: super::ground::OrdinaryGroundQuery<'_>,
    ) -> Result<i32> {
        self.require_collision_query(id, query.prepared, query.scene, query.animations)?;
        let current = self.spatial(id)?.query_pose()?.position;
        let start = current.map(|value| value >> 8);
        let mut end = start;
        end[1] = end[1].wrapping_add(self.ordinary_collision_shape(id)?.positive_extent[1]);
        let poses = self.ordinary_collision_poses(query.prepared)?;
        let models = query
            .scene
            .actor_collision
            .as_ref()
            .context("ordinary queued clearance live geometry unavailable")?;
        let contacts = query.scene.static_collision.runtime_segment_contacts(
            &query.prepared.candidates,
            models,
            &poses,
            start,
            end,
            query.sidedness,
            None,
        )?;
        let mut height = 999999;
        for (_, contact) in contacts {
            let contact = contact.integer()?;
            let support = cyrus_collision::ground::GroundSupport {
                height: f64::from(contact.point[1]),
                normal: contact.normal,
            };
            if matches!(support.classification(), 2 | 4) && contact.point[1] < height {
                height = contact.point[1];
            }
        }
        if let Some(terrain) = &query.scene.terrain {
            let support = terrain.sample(current.map(i64::from)).with_context(|| {
                format!(
                    "world {} instance {} queued clearance terrain water/grid boundary unsupported",
                    self.world, id.index
                )
            })?;
            let truncated = support.height.trunc();
            anyhow::ensure!(
                truncated.is_finite()
                    && truncated >= f64::from(i32::MIN)
                    && truncated <= f64::from(i32::MAX),
                "queued clearance terrain height outside integer range"
            );
            let terrain_height = truncated as i32;
            if start[1] < terrain_height && terrain_height < height {
                height = terrain_height;
            }
        }
        Ok(if height == 999999 {
            256000
        } else {
            height.wrapping_mul(256).wrapping_sub(current[1])
        })
    }

    /// Commit queued vertical motion and its high-bit transition animation.
    /// The owning queued handler establishes admission and resolves table entries.
    pub fn apply_ordinary_queued_vertical(
        &mut self,
        id: ScriptActorId,
        input: OrdinaryVerticalInputs,
        data: &cyrus_assets::animation::Animations,
        ticks: u32,
        sounds: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<()> {
        self.require_model_identity(data)?;
        let index = self.resolve(id)?;
        let actor = &self.actors[index];
        let control = actor
            .control
            .as_ref()
            .context("ordinary queued control unavailable")?;
        let before = OrdinaryVerticalState {
            y: self.spatial(id)?.local.position[1],
            origin_y: control.fall_origin_y,
            timer: control.fall_timer,
            queued: control.queued_motion,
            falling: control.falling_state,
        };
        let after = ordinary_queued_vertical(before, actor.attributes[32], input);
        let force = !input.model_controls_vertical
            && input.advanced_factor > 255
            && before.queued & 128 != 0
            && control.torch_burning == 0;
        let animated = match data.actor(actor.name())?["model_load_path"].as_str() {
            Some("animation_table") => true,
            Some("static" | "none") => false,
            path => anyhow::bail!(
                "world {} instance {index}: queued model load path {path:?} unavailable",
                self.world
            ),
        };
        self.spatial
            .as_mut()
            .context("ordinary queued spatial state unavailable")?
            .actor_mut(index)?
            .local
            .position[1] = after.y;
        let position = super::animation_sound_position(self.spatial.as_ref(), index);
        let actor = &mut self.actors[index];
        let control = actor
            .control
            .as_mut()
            .expect("validated ordinary queued control");
        control.fall_timer = after.timer;
        control.queued_motion = after.queued;
        control.falling_state = after.falling;
        if force && animated && self.updates.animation_system {
            let surface = actor.surface;
            let animation = actor
                .animation
                .as_mut()
                .context("ordinary queued animation receiver not prepared")?;
            let result = animation
                .playback
                .force_request_with_sounds(58, 0, ticks, &mut |sound| {
                    sounds(
                        id,
                        super::super::ScriptSound::Animation {
                            sound,
                            surface,
                            position,
                        },
                    )
                });
            match result {
                Ok(frame) => animation.frame = frame.or(animation.frame),
                Err(error) => {
                    let message = format!(
                        "world {} instance {index} queued animation: {error:#}",
                        self.world
                    );
                    actor.animation_failure = Some(message.clone());
                    anyhow::bail!(message);
                }
            }
        }
        Ok(())
    }

    /// Commit the falling vertical writer at its owning response stage. Model
    /// motion suppression must be established by the caller's current animation.
    pub fn apply_ordinary_falling_vertical(
        &mut self,
        id: ScriptActorId,
        input: OrdinaryVerticalInputs,
    ) -> Result<()> {
        let index = self.resolve(id)?;
        let control = self.actors[index]
            .control
            .as_ref()
            .context("ordinary falling control unavailable")?;
        let state = ordinary_falling_vertical(
            OrdinaryVerticalState {
                y: self.spatial(id)?.local.position[1],
                origin_y: control.fall_origin_y,
                timer: control.fall_timer,
                queued: control.queued_motion,
                falling: control.falling_state,
            },
            self.actors[index].attributes[32],
            input,
        );
        self.spatial
            .as_mut()
            .context("ordinary falling spatial state unavailable")?
            .actor_mut(index)?
            .local
            .position[1] = state.y;
        self.actors[index]
            .control
            .as_mut()
            .expect("validated ordinary falling control")
            .fall_timer = state.timer;
        Ok(())
    }
}
