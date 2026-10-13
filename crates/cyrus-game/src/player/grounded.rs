//! Composed static ground queries from preservation's ordinary player update rules.
use super::data::PlayerScene;
use anyhow::Context;
use bevy::math::{Mat3, Vec3};
use cyrus_collision::{
    collision_world::StaticCandidates,
    ground::{FallbackProbe, GroundProbe, GroundSupport, SelectedSupport, SupportHistory},
};

/// Inputs supplied by the admitted ordinary movement stage.
pub struct ForwardWalk {
    pub heading: u16,
    /// Eight fractional bits, distinct from the nominal animation tick count.
    pub frame_scale: u32,
    pub nominal_ticks: u32,
}

/// One caller-admitted ordinary sheathed update, with explicit original timing.
pub struct GroundedUpdate {
    pub motion: ForwardWalk,
    pub walk: bool,
}

/// Player-local walking state; shared support history and veto remain caller-owned.
pub struct GroundedWalk {
    pub position: [i64; 3],
    pub surface: u8,
    pub startup: super::walk::WalkingStartup,
}

/// Advance the admitted ordinary animation stage before selecting collision candidates.
/// Each invocation reuses nominal ticks; the returned frame retains the old pose on `None`.
pub fn advance_animation(
    scene: &PlayerScene,
    ticks: u32,
    history: &mut SupportHistory,
    animation: &mut cyrus_world::animation::ActorAnimation,
    sounds: &mut dyn FnMut(cyrus_world::animation::AnimationSound) -> anyhow::Result<()>,
) -> anyhow::Result<Option<u32>> {
    history.begin_player_update(ticks);
    let default_group = u16::from(
        scene
            .definition
            .as_ref()
            .context("grounded update: player definition unavailable")?
            .ordinary_default_group,
    );
    let frame = animation.select_default_with_sounds(default_group, ticks, sounds)?;
    let frame = animation.advance_with_sounds(ticks, sounds)?.or(frame);
    Ok(animation
        .request_with_sounds(default_group, 0, ticks, sounds)?
        .or(frame))
}

impl GroundedWalk {
    /// Run admitted ordinary walking and collision after animation/candidate preparation.
    /// Returns a frame only when the movement request selects one; retain the prior pose otherwise.
    pub fn move_grounded(
        &mut self,
        query: &StaticGroundQuery<'_>,
        input: &mut GroundedUpdate,
        shared: (&mut SupportHistory, &mut i32),
        animation: &mut cyrus_world::animation::ActorAnimation,
        sounds: &mut dyn FnMut(cyrus_world::animation::AnimationSound) -> anyhow::Result<()>,
    ) -> anyhow::Result<Option<u32>> {
        let previous = self.position;
        let definition = query
            .scene
            .definition
            .as_ref()
            .context("grounded update: player definition unavailable")?;
        let mut frame = None;
        let step = if input.walk {
            self.forward(
                query,
                &input.motion,
                (shared.0, shared.1),
                animation,
                sounds,
            )?
        } else {
            None
        };
        if let Some(step) = &step {
            frame = step.frame.or(frame);
        }
        let mut accepted = step.is_some();
        if !accepted && animation.current_group() == Some(20) {
            let probe = definition.forward_probe(self.position, query.rotation)?;
            if query
                .forward_support(probe, shared.0, &mut self.surface, shared.1)?
                .is_some()
            {
                // Continuation must preserve the pending default, without another request.
                let distance = self.startup.advance(
                    definition.forward_walk,
                    definition.forward_speed,
                    input.motion.frame_scale,
                    !animation.translation_suppressed(),
                );
                self.position =
                    super::motion::translate(self.position, input.motion.heading, distance as i64);
                accepted = true;
            }
        }
        self.resolve_ground(
            query,
            previous,
            &mut input.motion.heading,
            shared.0,
            shared.1,
        )?;
        self.startup.finish_update(accepted);
        Ok(frame)
    }

    /// Query post-movement contacts, assign ordinary support height, then resolve walls.
    /// Caller excludes airborne, sliding and special movement states. Missing support
    /// requires a falling-state transition and is reported instead of clamping Y.
    /// Requeries contacts for residual separation; remaining actor-update stages follow.
    pub fn resolve_ground(
        &mut self,
        query: &StaticGroundQuery<'_>,
        previous: [i64; 3],
        heading: &mut u16,
        history: &mut SupportHistory,
        surface_veto: &mut i32,
    ) -> anyhow::Result<()> {
        let contacts = query
            .scene
            .static_collision
            .body_contacts(
                query.candidates,
                self.position,
                query.rotation,
                &query.spheres()?,
                query.sidedness,
            )?
            .iter()
            .map(super::contact::GroundedContact::from_body)
            .collect::<anyhow::Result<Vec<_>>>()?;
        let support = query
            .support(self.position, history, &mut self.surface, surface_veto)?
            .context("grounded response: missing support; falling transition unsupported")?;
        let definition = query
            .scene
            .definition
            .as_ref()
            .context("grounded response: player definition unavailable")?;
        anyhow::ensure!(
            super::support::classification(&support, query.collision_attribute) == 4,
            "grounded response: nonordinary support requires separate movement state"
        );
        self.position[1] = support.actor_height(
            definition.positive_extent[1],
            query.scene.settings.standing_height,
        );
        self.position = super::contact::resolve_grounded_contacts(
            previous,
            self.position,
            heading,
            &contacts,
            query.scene.settings.slide_range,
        );
        let distance = super::contact::residual_distance(previous, self.position)?;
        let normals = query
            .scene
            .static_collision
            .body_contacts(
                query.candidates,
                self.position,
                query.rotation,
                &query.spheres()?,
                query.sidedness,
            )?
            .iter()
            .map(|contact| {
                super::contact::GroundedContact::from_body(contact).map(|contact| contact.normal)
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        super::contact::separate_residual(
            distance,
            &mut self.position,
            &normals,
            query.scene.settings.poly_push_units,
        );
        Ok(())
    }

    /// Probe support, request the walking animation, then translate in original axes.
    /// Caller establishes ordinary sheathed eligibility and supplies actor orientation.
    /// A successful zero-distance result still counts as accepted forward movement.
    /// Obstacle response and final support must follow before presenting the position.
    pub fn forward(
        &mut self,
        query: &StaticGroundQuery<'_>,
        input: &ForwardWalk,
        shared: (&mut SupportHistory, &mut i32),
        animation: &mut cyrus_world::animation::ActorAnimation,
        sounds: &mut dyn FnMut(cyrus_world::animation::AnimationSound) -> anyhow::Result<()>,
    ) -> anyhow::Result<Option<super::walk::WalkingStep>> {
        let definition = query
            .scene
            .definition
            .as_ref()
            .context("forward walk: player definition unavailable")?;
        let probe = definition.forward_probe(self.position, query.rotation)?;
        if query
            .forward_support(probe, shared.0, &mut self.surface, shared.1)?
            .is_none()
        {
            return Ok(None);
        }
        let step = self.startup.request(
            animation,
            definition,
            input.frame_scale,
            input.nominal_ticks,
            sounds,
        )?;
        if let Some(step) = &step {
            self.position =
                super::motion::translate(self.position, input.heading, step.distance as i64);
        }
        Ok(step)
    }
}

/// Caller-selected surface updater admission; actor-state eligibility is separate.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SurfaceUpdate {
    Excluded,
    QueryOnly,
    Assign,
}

/// An admitted ordinary player query against a prepared static candidate list.
/// The caller supplies orientation, collision modes and surface-update eligibility.
/// Dynamic/transient models and the preceding slope rollback are not inferred.
pub struct StaticGroundQuery<'a> {
    pub scene: &'a PlayerScene,
    pub candidates: &'a StaticCandidates,
    pub rotation: Mat3,
    pub collision_attribute: u8,
    pub alternate_collision: bool,
    pub sidedness: bool,
    pub surface_update: SurfaceUpdate,
}

impl StaticGroundQuery<'_> {
    fn spheres(&self) -> anyhow::Result<Vec<(Vec3, f32)>> {
        let definition = self
            .scene
            .definition
            .as_ref()
            .context("body query: player definition unavailable")?;
        Ok(definition
            .body_spheres(self.scene.settings.post_collide_height)?
            .into_iter()
            .map(|sphere| {
                (
                    Vec3::from_array(sphere.center.map(|v| v as f32)),
                    sphere.radius as f32,
                )
            })
            .collect())
    }
    fn terrain(&self, position: [i64; 3]) -> anyhow::Result<Option<GroundSupport>> {
        self.scene
            .terrain
            .as_ref()
            .map(|terrain| {
                terrain.sample(position).with_context(|| {
                    format!(
                        "world {} ground query at {position:?}: water or terrain edge unsupported",
                        self.scene.world
                    )
                })
            })
            .transpose()
    }

    /// Query support, commit shared history, then update surface state even after a miss.
    /// History and veto are shared across actors; `surface` belongs to this player.
    /// Water/lava transition effects remain explicit errors after their state assignment.
    pub fn support(
        &self,
        position: [i64; 3],
        history: &mut SupportHistory,
        surface: &mut u8,
        surface_veto: &mut i32,
    ) -> anyhow::Result<Option<SelectedSupport>> {
        let definition = self
            .scene
            .definition
            .as_ref()
            .context("ground query: player definition unavailable")?;
        let probe = GroundProbe::new(
            position,
            definition.positive_extent[1],
            self.scene.settings.post_collide_height,
        )?;
        let fallback = FallbackProbe::new(
            position,
            [0; 3],
            definition.positive_extent[1],
            history.ticks() != 0,
        )?;
        let selected = self.scene.static_collision.selected_player_support(
            self.candidates,
            &probe,
            self.terrain(position)?,
            &fallback,
            self.sidedness,
            history,
        )?;
        if self.collision_attribute & 4 != 0 {
            let previous = *surface;
            let resolved = if self.surface_update == SurfaceUpdate::Excluded {
                0
            } else {
                selected
                    .as_ref()
                    .map(|support| self.scene.support_surface(support, position))
                    .transpose()?
                    .unwrap_or(0)
            };
            if self.surface_update == SurfaceUpdate::Assign {
                *surface = resolved;
                *surface_veto = i32::from(matches!(resolved, 5 | 11));
            }
            anyhow::ensure!(
                resolved == previous
                    || (!(1..=5).contains(&previous) && !(1..=5).contains(surface)),
                "world {} ground query: water/lava transition effects unsupported",
                self.scene.world
            );
        }
        Ok(selected)
    }

    /// Body clearance, slope selection, support side effects, then final admission.
    /// `proposed` is the caller's oriented forward probe, not a permanent player move.
    pub fn forward_support(
        &self,
        proposed: [i64; 3],
        history: &mut SupportHistory,
        surface: &mut u8,
        surface_veto: &mut i32,
    ) -> anyhow::Result<Option<SelectedSupport>> {
        let definition = self
            .scene
            .definition
            .as_ref()
            .context("forward query: player definition unavailable")?;
        let spheres = self.spheres()?;
        if self.scene.static_collision.blocks_body(
            self.candidates,
            proposed,
            self.rotation,
            &spheres,
            self.sidedness,
        )? {
            return Ok(None);
        }
        if self.collision_attribute & 2 != 0 && !self.alternate_collision {
            let probe = GroundProbe::new(
                proposed,
                definition.positive_extent[1],
                self.scene.settings.post_collide_height,
            )?;
            let contacts = self.scene.static_collision.ground_contacts(
                self.candidates,
                &probe,
                self.sidedness,
            )?;
            let slope = probe
                .slope_surface(self.terrain(proposed)?, &contacts)
                .context("forward query: missing slope surface; fallback unsupported")?;
            if slope.classification() == 2 {
                return Ok(None);
            }
        }
        let selected = self.support(proposed, history, surface, surface_veto)?;
        Ok(selected.filter(|support| {
            super::support::allows_forward(support, self.collision_attribute, *surface_veto)
        }))
    }
}
