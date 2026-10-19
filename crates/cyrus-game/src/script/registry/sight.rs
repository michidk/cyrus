//! Line of sight between two actors follows preservation's combat decision "In sight" rules.
use super::super::combat::{Engagement, combat_target_valid, engagement};
use super::{ScriptActorId, ScriptWorld, obstacle::OrdinaryObstacleQuery};
use anyhow::{Context, Result};
use cyrus_collision::{distance::rounded_distance, ground::SupportSource};

/// Live samples established before the nonplayer decision stage's cleanup and selections.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OrdinaryCombatSample {
    pub target: ScriptActorId,
    /// Rounded three-dimensional query-position distance in Q8 units.
    pub distance: i32,
    pub engaged: bool,
}

impl ScriptWorld {
    /// Run the nonplayer perception prepass before message and decision maintenance.
    /// Timer waiting and reacquisition use live collision/navigation owners. Routed
    /// pursuit retains its transition and invokes the point wrapper. Shared pauses,
    /// immediate arrival and held-node facing are supported. Delay admission consumes
    /// the caller's shared random stream; later routing phases remain unavailable.
    pub fn maintain_combat_perception(
        &mut self,
        id: ScriptActorId,
        selected_player: ScriptActorId,
        ticks: u32,
        inputs: super::ScriptInputs<'_>,
        random: Option<&mut crate::random::EngineRandom>,
        sounds: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<()> {
        let result = (|| -> Result<()> {
            let index = self.resolve(id)?;
            let actor = &self.actors[index];
            let Some(control) = actor.control.as_ref() else {
                return Ok(());
            };
            let Some(combat) = control.combat.as_ref() else {
                return Ok(());
            };
            if id == selected_player || control.death_selector == 8 || combat.flags & 4 == 0 {
                return Ok(());
            }
            let Some(target) = combat.target else {
                return Ok(());
            };
            let limit = combat.spacing_ranges[1].wrapping_mul(2);
            if control.navigation.map.is_none() && self.nearest_free_walk_node(id)?.is_none() {
                return Ok(());
            }
            // Preserve target-before-receiver query ordering.
            let target_position = self.query_position(target)?;
            self.query_position(id)?;
            let geometry = inputs
                .locomotion_geometry
                .context("perception sight geometry unavailable")?;
            let visible = self.actor_in_sight(
                id,
                target,
                limit,
                OrdinaryObstacleQuery {
                    scene: geometry.ground.scene,
                    animations: geometry.ground.animations,
                    prepared: geometry.ground.prepared,
                    vertical_offset: 0,
                    respond: false,
                },
            )?;
            let combat = self.actors[index]
                .control
                .as_mut()
                .expect("admitted control")
                .combat
                .as_mut()
                .expect("admitted combat");
            if visible {
                combat.lost_sight_ticks = 0;
                if combat.lost_sight_routing != 0 {
                    self.actors[index]
                        .control
                        .as_mut()
                        .expect("admitted control")
                        .death_selector = 6;
                    self.actors[index]
                        .control
                        .as_mut()
                        .expect("admitted control")
                        .combat
                        .as_mut()
                        .expect("admitted combat")
                        .lost_sight_routing = 0;
                    self.release_held_walk_nodes(id)?;
                    self.set_combat_coordination(id, 1)?;
                    self.clear_walk_node_reservations()?;
                }
                return Ok(());
            }
            let delay = self
                .node_timer
                .context("installed combat node_timer unavailable")?;
            if combat.lost_sight_ticks <= delay {
                combat.lost_sight_ticks = combat.lost_sight_ticks.wrapping_add(ticks as i32);
                return Ok(());
            }
            if combat.lost_sight_routing == 0
                && let Some(peer) = self.ordinary_active_coordinator(id, selected_player, inputs)?
            {
                self.send_combat_message(id, peer, 0xd002)?;
                self.set_combat_coordination(id, 0)?;
            }
            self.actors[index]
                .control
                .as_mut()
                .expect("admitted control")
                .combat
                .as_mut()
                .expect("admitted combat")
                .lost_sight_routing = 1;
            self.move_ordinary_point(
                id,
                target_position,
                0,
                super::ScriptInputs {
                    nominal_ticks: Some(ticks),
                    ..inputs
                },
                random,
                Some(sounds),
            )?;
            Ok(())
        })();
        result.with_context(|| {
            format!(
                "world {} instance {} combat perception",
                self.world, id.index
            )
        })
    }

    /// Prepare the admitted nonplayer decision stage's target, counter and engagement.
    /// Only lifecycle selector six, not dead and a combat record admit this prefix.
    /// Fallback copies target local position before distance; successful engagement
    /// copies it again before cleanup. Earlier mutations survive later query errors.
    /// Geometry is required only when live attributes request a sight test.
    /// This explicit prefix does not enable automatic decision traversal.
    pub fn prepare_ordinary_combat_decision(
        &mut self,
        id: ScriptActorId,
        selected_player: ScriptActorId,
        query: Option<OrdinaryObstacleQuery<'_>>,
    ) -> Result<Option<OrdinaryCombatSample>> {
        let result = (|| -> Result<_> {
            let index = self.resolve(id)?;
            let actor = &self.actors[index];
            anyhow::ensure!(
                id != selected_player
                    && !matches!(
                        actor.source,
                        super::ScriptActorSource::Player
                            | super::ScriptActorSource::AlternatePlayer
                    ),
                "selected-player combat decision unavailable"
            );
            let control = actor
                .control
                .as_ref()
                .context("combat decision control unavailable")?;
            if control.death_selector != 6 || self.query_is_dead(id, Some(selected_player))? {
                return Ok(None);
            }
            let Some(combat) = control.combat.as_ref() else {
                return Ok(None);
            };
            let valid = match combat.target {
                Some(target) => {
                    let target_index = self.resolve(target)?;
                    self.actors[target_index]
                        .control
                        .as_ref()
                        .is_some_and(|control| combat_target_valid(control, target))
                }
                None => false,
            };
            let target = if valid {
                combat.target.expect("admitted target")
            } else {
                self.resolve(selected_player)?;
                self.actors[index]
                    .control
                    .as_mut()
                    .expect("admitted control")
                    .combat
                    .as_mut()
                    .expect("admitted combat")
                    .target = Some(selected_player);
                let position = self.spatial(selected_player)?.local.position;
                let combat = self.actors[index]
                    .control
                    .as_mut()
                    .expect("admitted control")
                    .combat
                    .as_mut()
                    .expect("admitted combat");
                combat.retained_target_position = position;
                selected_player
            };
            let from = self.query_position(id)?;
            let to = self.query_position(target)?;
            let distance = rounded_distance(std::array::from_fn(|axis| {
                to[axis].wrapping_sub(from[axis])
            }))?;
            let combat = self.actors[index]
                .control
                .as_mut()
                .expect("admitted control")
                .combat
                .as_mut()
                .expect("admitted combat");
            combat.close_distance_passes = if distance < 25000 {
                combat.close_distance_passes.wrapping_add(1)
            } else {
                0
            };
            let engaged = match engagement(&self.actors[index].attributes) {
                Engagement::Always => true,
                Engagement::InSightWithin(limit) => self.actor_in_sight(
                    id,
                    target,
                    limit,
                    query.context("combat decision sight geometry unavailable")?,
                )?,
            };
            if engaged {
                let position = self.spatial(target)?.local.position;
                self.actors[index]
                    .control
                    .as_mut()
                    .expect("admitted control")
                    .combat
                    .as_mut()
                    .expect("admitted combat")
                    .retained_target_position = position;
            }
            Ok(Some(OrdinaryCombatSample {
                target,
                distance,
                engaged,
            }))
        })();
        result.with_context(|| {
            format!(
                "world {} instance {} combat decision prefix",
                self.world, id.index
            )
        })
    }

    /// The actor's query position: the attached pose, the linked position, or the local
    /// position, in that order (the selection shared by the fighting query).
    pub(super) fn sight_position(&self, index: usize) -> Result<[i32; 3]> {
        self.query_position(self.id(index)?)
    }

    /// Whether `target` is within `limit` (Q8 units) of the receiver with nothing in its
    /// prepared candidate list between them. The segment between the two query positions, in
    /// whole map units, is tested against the receiver's candidates collected before its
    /// update; hits on the target's own model do not block. **Inferred:** the model test treats
    /// both faces of a surface as blocking.
    pub fn actor_in_sight(
        &self,
        id: ScriptActorId,
        target: ScriptActorId,
        limit: i32,
        query: OrdinaryObstacleQuery<'_>,
    ) -> Result<bool> {
        self.require_collision_query(id, query.prepared, query.scene, query.animations)?;
        let target_index = self.resolve(target)?;
        let from = self.sight_position(id.index)?;
        let to = self.sight_position(target_index)?;
        let distance = rounded_distance(std::array::from_fn(|axis| {
            to[axis].wrapping_sub(from[axis])
        }))?;
        if distance > limit {
            return Ok(false);
        }
        let poses = self.ordinary_collision_poses(query.prepared)?;
        let contacts = query.scene.static_collision.runtime_segment_contacts(
            &query.prepared.candidates,
            query
                .scene
                .actor_collision
                .as_ref()
                .context("in-sight live geometry unavailable; re-export world")?,
            &poses,
            from.map(|value| value >> 8),
            to.map(|value| value >> 8),
            false,
            None,
        )?;
        Ok(contacts.iter().all(|(source, _)| {
            matches!(source, SupportSource::Actor { actor, .. } if *actor == target_index)
        }))
    }

    /// Whether `id` is engaged with its combat target (preservation's "Engagement" rule).
    /// The target is the receiver's own while that actor is a valid target, otherwise the
    /// selected player; this does not write the selection back to the combat record.
    pub fn combat_engaged(
        &self,
        id: ScriptActorId,
        selected_player: ScriptActorId,
        query: OrdinaryObstacleQuery<'_>,
    ) -> Result<bool> {
        let index = self.resolve(id)?;
        let actor = &self.actors[index];
        let combat = actor
            .control
            .as_ref()
            .and_then(|control| control.combat.as_ref())
            .context("combat engagement requires a combat record")?;
        let own_target_is_valid = |target: ScriptActorId| -> Result<bool> {
            let index = self.resolve(target)?;
            Ok(self.actors[index]
                .control
                .as_ref()
                .is_some_and(|control| combat_target_valid(control, target)))
        };
        let target = match combat.target {
            Some(target) if own_target_is_valid(target)? => target,
            _ => selected_player,
        };
        match engagement(&actor.attributes) {
            Engagement::Always => Ok(true),
            Engagement::InSightWithin(limit) => self.actor_in_sight(id, target, limit, query),
        }
    }
}
