//! Ordinary slope prevalidation precedes support and its response stages.
use super::{ScriptActorId, ScriptWorld};
use anyhow::{Context, Result};
use cyrus_collision::ground::{GroundProbe, GroundSupport};

/// Shared results retained until the first ground prevalidation of the next wrapper.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct OrdinaryGroundPrevalidation {
    pub rollback: bool,
    pub sliding: bool,
}

/// Compare selected current/previous surfaces using preservation's ground rules.
/// When exactly one position misses, the original reads the missing side's class from
/// uninitialised data; here a missing side is neither class 2 nor class 4, so a missing current
/// surface clears both results and a class 2 current surface after a missing previous one takes
/// the heading comparison (docs/DIFFERENCES.md).
pub fn ordinary_ground_prevalidation(
    current: Option<GroundSupport>,
    previous: Option<GroundSupport>,
    displacement: [i32; 3],
    attribute_89: u8,
    default_motion_animation: bool,
) -> Result<OrdinaryGroundPrevalidation> {
    let Some(current) = current else {
        return Ok(OrdinaryGroundPrevalidation::default());
    };
    let current_class = current.classification();
    let previous_class = previous.map_or(0, |previous| previous.classification());
    if current_class == 2 && previous_class == 2 {
        return Ok(OrdinaryGroundPrevalidation {
            rollback: false,
            sliding: true,
        });
    }
    if current_class != 2 {
        return Ok(OrdinaryGroundPrevalidation::default());
    }
    let heading = crate::player::motion::displacement_heading;
    let movement = heading(i64::from(displacement[0]), i64::from(displacement[2]));
    let normal = heading(i64::from(current.normal[0]), i64::from(current.normal[2]));
    let difference = normal.wrapping_sub(1024).wrapping_sub(movement) & 2047;
    let rollback =
        !(512..=1535).contains(&difference) || attribute_89 != 0 || default_motion_animation;
    Ok(OrdinaryGroundPrevalidation {
        rollback,
        sliding: !rollback,
    })
}

impl ScriptWorld {
    /// The first call in an ordinary wrapper updates shared slope results.
    /// Later calls retain them even when the receiver or candidates change.
    /// `alternate_selected_player` is true only for the selected player's mode.
    pub fn query_ordinary_ground_prevalidation(
        &mut self,
        id: ScriptActorId,
        query: super::ground::OrdinaryGroundQuery<'_>,
        alternate_selected_player: bool,
    ) -> Result<OrdinaryGroundPrevalidation> {
        self.resolve(id)?;
        if self.globals.ground_prevalidation_used {
            return self.globals.ground_prevalidation.with_context(|| format!(
                "world {} instance {} ordinary ground prevalidation unavailable after an earlier unsupported query",
                self.world, id.index,
            ));
        }
        self.globals.ground_prevalidation_used = true;
        self.globals.ground_prevalidation = None;
        let result =
            self.calculate_ordinary_ground_prevalidation(id, query, alternate_selected_player)?;
        self.globals.ground_prevalidation = Some(result);
        Ok(result)
    }

    fn calculate_ordinary_ground_prevalidation(
        &self,
        id: ScriptActorId,
        query: super::ground::OrdinaryGroundQuery<'_>,
        alternate_selected_player: bool,
    ) -> Result<OrdinaryGroundPrevalidation> {
        self.require_collision_query(id, query.prepared, query.scene, query.animations)?;
        let actor = &self.actors[id.index];
        if actor.attributes[1] & 2 == 0 || alternate_selected_player {
            return Ok(OrdinaryGroundPrevalidation::default());
        }
        let spatial = self.spatial(id)?;
        let current = spatial.query_pose()?.position;
        let moved = spatial.local.position[0] != spatial.previous_local_position[0]
            || spatial.local.position[2] != spatial.previous_local_position[2];
        let previous = if !moved {
            current
        } else if spatial.parent.is_none() {
            spatial.previous_local_position
        } else {
            spatial
                .previous_effective_position
                .context("ordinary ground prevalidation previous effective position unavailable")?
        };
        let poses = self.ordinary_collision_poses(query.prepared)?;
        let models =
            query.scene.actor_collision.as_ref().context(
                "ordinary ground prevalidation live geometry unavailable; re-export world",
            )?;
        let extent = self.ordinary_collision_shape(id)?.positive_extent[1];
        let surface = |position: [i32; 3]| -> Result<Option<GroundSupport>> {
            let position = position.map(i64::from);
            let probe =
                GroundProbe::new(position, extent, query.scene.settings.post_collide_height)?;
            let contacts = query.scene.static_collision.runtime_ground_contacts(
                &query.prepared.candidates,
                models,
                &poses,
                &probe,
                query.sidedness,
                None,
            )?;
            let terrain = query
                .scene
                .terrain
                .as_ref()
                .and_then(|t| t.sample(position));
            Ok(probe.slope_surface(terrain, &contacts))
        };
        let current_surface = surface(current)?;
        let previous_surface = if moved {
            surface(previous)?
        } else {
            current_surface
        };
        let default_motion = actor.animation.as_ref().is_some_and(|animation| {
            animation.playback.current_group().is_some_and(|group| {
                group == animation.playback.default_group() || (20..=27).contains(&group)
            })
        });
        ordinary_ground_prevalidation(
            current_surface,
            previous_surface,
            std::array::from_fn(|axis| current[axis].wrapping_sub(previous[axis])),
            actor.attributes[89],
            default_motion,
        )
        .with_context(|| {
            format!(
                "world {} instance {} ground prevalidation",
                self.world, id.index
            )
        })
    }

    /// Apply the prevalidation veto before the main support query. The owning
    /// caller retains `sliding` for the later support response on this update.
    pub fn apply_ordinary_ground_prevalidation(
        &mut self,
        id: ScriptActorId,
        result: OrdinaryGroundPrevalidation,
        data: &cyrus_assets::animation::Animations,
        environment: super::transition::ObstacleEnvironment,
        sounds: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<()> {
        let index = self.resolve(id)?;
        if result.rollback {
            let previous = self.spatial(id)?.previous_local_position;
            self.spatial
                .as_mut()
                .context("ordinary ground prevalidation spatial state unavailable")?
                .actor_mut(index)?
                .local
                .position = previous;
            self.actors[index]
                .control
                .as_mut()
                .context("ordinary ground prevalidation control unavailable")?
                .movement_flags = 0;
            self.apply_ordinary_obstacle_transition(id, data, environment, sounds)?;
        }
        Ok(())
    }
}
