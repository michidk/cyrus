//! Local and composed actor poses follow preservation's SOUP attachment rules.
use super::ScriptActor;
use anyhow::{Context, Result, ensure};
use cyrus_world::transform::Matrix;
use serde::Serialize;
use std::collections::BTreeMap;

/// Current selected-player query input, borrowed from the owning player system.
/// Coordinates have eight fractional bits; heading has 2048 units per revolution.
#[derive(Clone, Copy, Debug)]
pub struct PlayerQuery {
    pub position: [i32; 3],
    pub heading: u16,
}

/// Explicit global query inputs; absence is unsupported, never an assumed zero.
#[derive(Clone, Copy, Debug)]
pub struct QueryInputs {
    pub player: Option<PlayerQuery>,
    pub distance_override: Option<bool>,
    pub fallback_position: Option<[i32; 3]>,
    /// None means link ownership is unknown; Some(None) establishes no linked record.
    pub linked_position: Option<Option<[i32; 3]>>,
}

/// Original Q8 coordinates and pitch/yaw/roll in wrapped 1/2048 turns.
/// The stored matrix retains its single-precision update history.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Pose {
    pub position: [i32; 3],
    pub angles: [u16; 3],
    #[serde(serialize_with = "serialize_matrix")]
    pub matrix: Matrix,
}

/// One actor's pose owner within its ScriptWorld generation.
/// Attached local coordinates are relative to the parent; effective coordinates
/// remain unavailable until an explicit family refresh establishes them.
#[derive(Clone, Debug, Serialize)]
pub struct ActorSpatial {
    pub local: Pose,
    /// Original home followed by authored RALC destinations, all in Q8 units.
    pub(super) locations: Vec<[i32; 3]>,
    pub effective: Option<Pose>,
    pub previous_effective_position: Option<[i32; 3]>,
    /// Effective angles before the latest family refresh.
    pub previous_effective_angles: Option<[u16; 3]>,
    /// Local position captured at the ordinary update boundary, in Q8 units.
    pub previous_local_position: [i32; 3],
    /// End-of-update position retained by dry or falling updates for water veto.
    /// None leaves the fresh-allocation record explicitly unavailable.
    pub water_rollback_position: Option<[i32; 3]>,
    /// Previous local angles after snapshot; propagated deltas after family refresh.
    pub angle_history: [u16; 3],
    /// Angles the actor was created with; `ResetRotation` restores axes from them.
    pub initial_angles: [u16; 3],
    pub parent: Option<usize>,
    /// Fresh allocation has no linked-position record; dynamic creation is unsupported.
    /// The record's cached position while it names an owner, for effective-position queries.
    pub(super) linked_position: Option<[i32; 3]>,
    /// The record's owner and the offset in the owner's frame, set by `Offset`.
    pub(super) link: Option<PositionLink>,
    /// The record's cached position, retained when it no longer names an owner.
    pub(super) link_cache: [i32; 3],
    /// The cached position before the latest refresh.
    pub(super) link_previous: [i32; 3],
    no_slave_rotation: bool,
    rotation_mode: u8,
}

/// A linked-position record that names an owner: the actor's position is the owner's effective
/// position plus `offset` rotated by the owner's orientation, plus the actor's own local position.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct PositionLink {
    /// Owner actor index within the world.
    pub owner: usize,
    /// Q8 offset in the owner's frame.
    pub offset: [i32; 3],
}

pub(super) struct SpatialWorld {
    actors: Vec<Option<ActorSpatial>>,
    order: Vec<usize>,
    /// Family refreshes since the world's families were initialized; attached supports
    /// carry riders only from the third refresh on.
    refreshes: u32,
}

impl SpatialWorld {
    pub fn prepare(actors: &[ScriptActor]) -> Result<Self> {
        let mut poses = Vec::with_capacity(actors.len());
        let mut masters = BTreeMap::new();
        for (index, actor) in actors.iter().enumerate() {
            let placement = actor.placement();
            let pose = placement.map(|placement| -> Result<_> {
                let mut position = [0; 3];
                for (axis, value) in placement.map_position.into_iter().enumerate() {
                    position[axis] = value.checked_mul(256).and_then(|value| i32::try_from(value).ok())
                        .with_context(|| format!("actor {index} {} map position axis {axis}: outside Q8 i32 range", actor.name()))?;
                }
                let angles = if matches!(placement.object_type, 2 | 6) {
                    [0; 3]
                } else {
                    placement.angles.map(|angle| (angle & 2047) as u16)
                };
                Ok(Pose { position, angles, matrix: Matrix::orientation(angles) })
            }).transpose()?;
            if pose.is_some() && actor.attributes()[25] != 0 {
                if let Some(&previous) = masters.get(&actor.attributes()[25]) {
                    let previous_actor: &ScriptActor = &actors[previous];
                    let same_list = match (previous_actor.placement(), actor.placement()) {
                        (Some(a), Some(b)) => {
                            (a.active != 0 && b.active != 0)
                                || (a.active == 0
                                    && b.active == 0
                                    && [a.map_position[0] / 512, a.map_position[2] / 512]
                                        == [b.map_position[0] / 512, b.map_position[2] / 512])
                        }
                        _ => false,
                    };
                    ensure!(
                        same_list
                            && previous_actor.attributes()[26] == 0
                            && actor.attributes()[26] == 0,
                        "attachment group {}: duplicate masters across source lists or child families require original traversal ordering",
                        actor.attributes()[25]
                    );
                }
                // Fresh actors prepend to their source list. The later allocation
                // is its first matching root; an unselected duplicate stays unattached.
                masters.insert(actor.attributes()[25], index);
            }
            poses.push(pose);
        }
        let mut states = Vec::with_capacity(actors.len());
        for (index, actor) in actors.iter().enumerate() {
            let Some(original) = poses[index] else {
                states.push(None);
                continue;
            };
            let parent = (actor.attributes()[26] != 0)
                .then(|| {
                    masters
                        .get(&actor.attributes()[26])
                        .copied()
                        .with_context(|| {
                            format!(
                                "actor {index} {}: attachment group {} has no master",
                                actor.name(),
                                actor.attributes()[26]
                            )
                        })
                })
                .transpose()?;
            let mut local = original;
            if let Some(parent) = parent {
                let parent_pose = poses[parent].context("attachment master pose absent")?;
                local.position =
                    parent_pose
                        .matrix
                        .inverse_transform(std::array::from_fn(|axis| {
                            original.position[axis].wrapping_sub(parent_pose.position[axis])
                        }))?;
                if actors[parent].attributes()[33] == 0 {
                    local.angles = std::array::from_fn(|axis| {
                        original.angles[axis].wrapping_sub(parent_pose.angles[axis]) & 2047
                    });
                }
            }
            states.push(Some(ActorSpatial {
                local,
                locations: std::iter::once(original.position)
                    .chain(actor.location_offsets.iter().map(|offset| {
                        std::array::from_fn(|axis| {
                            original.position[axis].wrapping_add(offset[axis])
                        })
                    }))
                    .collect(),
                effective: parent.is_none().then_some(original),
                previous_effective_position: None,
                previous_effective_angles: None,
                previous_local_position: local.position,
                water_rollback_position: None,
                angle_history: original.angles,
                initial_angles: original.angles,
                parent,
                linked_position: None,
                link: None,
                link_cache: [0; 3],
                link_previous: [0; 3],
                no_slave_rotation: actor.attributes()[33] != 0,
                rotation_mode: actor.attributes()[28],
            }));
        }
        let mut order = Vec::new();
        let mut marks = vec![0; states.len()];
        fn visit(
            index: usize,
            states: &[Option<ActorSpatial>],
            marks: &mut [u8],
            order: &mut Vec<usize>,
        ) -> Result<()> {
            ensure!(
                marks[index] != 1,
                "actor {index}: cyclic attachment groups unsupported"
            );
            if marks[index] == 2 {
                return Ok(());
            }
            marks[index] = 1;
            if let Some(parent) = states[index].as_ref().and_then(|state| state.parent) {
                visit(parent, states, marks, order)?;
            }
            marks[index] = 2;
            if states[index].is_some() {
                order.push(index);
            }
            Ok(())
        }
        for index in 0..states.len() {
            visit(index, &states, &mut marks, &mut order)?;
        }
        Ok(Self {
            actors: states,
            order,
            refreshes: 0,
        })
    }

    /// Account for an actor started after preparation that has no spatial state, such as a shell.
    pub(super) fn push_absent(&mut self) {
        self.actors.push(None);
    }

    /// Start spatial state for an object created after preparation: a local pose at `position`
    /// with the given wrapped angles, parented to its creator for the family refresh.
    pub(super) fn push_created(
        &mut self,
        actor: &ScriptActor,
        position: [i32; 3],
        angles: [u16; 3],
        creator: usize,
    ) -> Result<()> {
        ensure!(
            angles.iter().all(|angle| *angle < 2048),
            "created object angles outside wrapped range"
        );
        ensure!(
            self.actors.get(creator).is_some_and(Option::is_some),
            "created object creator has no spatial state"
        );
        let pose = Pose {
            position,
            angles,
            matrix: Matrix::orientation(angles),
        };
        let index = self.actors.len();
        self.actors.push(Some(ActorSpatial {
            local: pose,
            locations: std::iter::once(position)
                .chain(actor.location_offsets.iter().map(|offset| {
                    std::array::from_fn(|axis| position[axis].wrapping_add(offset[axis]))
                }))
                .collect(),
            effective: Some(pose),
            previous_effective_position: None,
            previous_effective_angles: None,
            previous_local_position: position,
            water_rollback_position: None,
            angle_history: angles,
            initial_angles: angles,
            parent: None,
            linked_position: None,
            link: None,
            link_cache: [0; 3],
            link_previous: [0; 3],
            no_slave_rotation: actor.attributes()[33] != 0,
            rotation_mode: actor.attributes()[28],
        }));
        self.order.push(index);
        Ok(())
    }

    pub fn initialize_player(
        &mut self,
        index: usize,
        actor: &ScriptActor,
        position: [i32; 3],
        angles: [u16; 3],
    ) -> Result<()> {
        self.place_player(index, actor, position, angles, false)
    }

    pub fn place_player(
        &mut self,
        index: usize,
        actor: &ScriptActor,
        position: [i32; 3],
        angles: [u16; 3],
        replace: bool,
    ) -> Result<()> {
        let slot = self
            .actors
            .get_mut(index)
            .context("player spatial slot absent")?;
        ensure!(
            replace || slot.is_none(),
            "actor {index}: spatial state already initialized"
        );
        ensure!(
            angles.iter().all(|angle| *angle < 2048),
            "actor {index}: player angles outside wrapped range"
        );
        ensure!(
            actor.attributes()[25] == 0 && actor.attributes()[26] == 0,
            "actor {index}: attached player initialization unsupported"
        );
        let pose = Pose {
            position,
            angles,
            matrix: Matrix::orientation(angles),
        };
        let initialized = slot.is_some();
        *slot = Some(ActorSpatial {
            local: pose,
            locations: std::iter::once(position)
                .chain(actor.location_offsets.iter().map(|offset| {
                    std::array::from_fn(|axis| position[axis].wrapping_add(offset[axis]))
                }))
                .collect(),
            effective: Some(pose),
            previous_effective_position: None,
            previous_effective_angles: None,
            previous_local_position: position,
            water_rollback_position: None,
            angle_history: angles,
            initial_angles: angles,
            parent: None,
            linked_position: None,
            link: None,
            link_cache: [0; 3],
            link_previous: [0; 3],
            no_slave_rotation: actor.attributes()[33] != 0,
            rotation_mode: actor.attributes()[28],
        });
        if !initialized {
            self.order.push(index);
        }
        Ok(())
    }

    /// Family refreshes since initialization.
    pub fn refreshes(&self) -> u32 {
        self.refreshes
    }

    pub fn is_initialized(&self, index: usize) -> Result<bool> {
        Ok(self
            .actors
            .get(index)
            .with_context(|| format!("actor {index}: outside spatial registry"))?
            .is_some())
    }

    pub fn actor(&self, index: usize) -> Result<&ActorSpatial> {
        self.actors
            .get(index)
            .and_then(Option::as_ref)
            .with_context(|| format!("actor {index}: spatial state unavailable"))
    }

    pub fn actor_mut(&mut self, index: usize) -> Result<&mut ActorSpatial> {
        self.actors
            .get_mut(index)
            .and_then(Option::as_mut)
            .with_context(|| format!("actor {index}: spatial state unavailable"))
    }

    /// One family pass. Does not run scripts or refresh ordinary root matrices.
    pub fn refresh_families(&mut self) -> Result<()> {
        self.refreshes = self.refreshes.saturating_add(1);
        for &index in &self.order {
            let state = self.actors[index].as_ref().unwrap();
            let Some(parent_index) = state.parent else {
                continue;
            };
            let parent = self.actor(parent_index)?;
            let parent_pose = parent.query_pose()?;
            let parent_delta = if parent.parent.is_some() {
                parent.angle_history
            } else {
                std::array::from_fn(|axis| {
                    parent.local.angles[axis].wrapping_sub(parent.angle_history[axis]) & 2047
                })
            };
            let state = self.actors[index].as_mut().unwrap();
            let offset = parent_pose.matrix.transform(state.local.position)?;
            let position =
                std::array::from_fn(|axis| parent_pose.position[axis].wrapping_add(offset[axis]));
            let angles;
            let matrix;
            if state.no_slave_rotation {
                let delta = std::array::from_fn(|axis| {
                    state.local.angles[axis].wrapping_sub(state.angle_history[axis]) & 2047
                });
                state.local.matrix.rotate(delta);
                matrix = state.local.matrix;
                angles = state.local.angles;
            } else {
                let mut composed = parent_pose.matrix;
                composed.rotate(state.local.angles);
                matrix = composed;
                angles = std::array::from_fn(|axis| {
                    parent_pose.angles[axis].wrapping_add(state.local.angles[axis]) & 2047
                });
                state.angle_history = std::array::from_fn(|axis| {
                    state.local.angles[axis]
                        .wrapping_sub(state.angle_history[axis])
                        .wrapping_add(parent_delta[axis])
                        & 2047
                });
            }
            state.previous_effective_position = Some(
                state
                    .effective
                    .map_or(state.local.position, |pose| pose.position),
            );
            state.previous_effective_angles = Some(
                state
                    .effective
                    .map_or(state.local.angles, |pose| pose.angles),
            );
            state.effective = Some(Pose {
                position,
                angles,
                matrix,
            });
        }
        Ok(())
    }
}

impl ActorSpatial {
    /// Current ordinary pose, or the last explicitly composed attached pose.
    pub fn query_pose(&self) -> Result<Pose> {
        if self.parent.is_none() {
            Ok(self.local)
        } else {
            self.effective
                .context("attached pose requires an explicit family refresh")
        }
    }

    /// The linked record's cached position while it names an owner, as effective-position
    /// queries use it; `None` without an owner.
    pub fn linked_position(&self) -> Option<[i32; 3]> {
        self.linked_position
    }

    /// `Offset`: name the owner and the offset in its frame. The cached position stays as it was
    /// until the next refresh.
    pub(super) fn link_to(&mut self, owner: usize, offset: [i32; 3]) {
        self.link = Some(PositionLink { owner, offset });
        self.linked_position = Some(self.link_cache);
    }

    /// `UnOffset`: the record keeps its cache but names no owner.
    pub(super) fn unlink(&mut self) {
        self.link = None;
        self.linked_position = None;
    }

    /// Publish a refreshed cached position, keeping the one it replaces.
    pub(super) fn set_link_cache(&mut self, position: [i32; 3]) {
        self.link_previous = self.link_cache;
        self.link_cache = position;
        self.linked_position = Some(position);
    }

    /// Copy local position and angles at the ordinary actor-update boundary.
    pub fn snapshot(&mut self) {
        self.previous_local_position = self.local.position;
        self.angle_history = self.local.angles;
    }

    /// `ResetRotation`: restore each selected axis from the creation angles, make the
    /// previous-angle history equal the result, and rebuild the matrix from the angles
    /// (identity, then negated yaw, pitch and roll). Rotation mode one's alternate matrix
    /// builder is not established and is rejected before any change.
    pub fn reset_rotation(&mut self, axes: [bool; 3]) -> Result<()> {
        ensure!(
            self.rotation_mode != 1,
            "ResetRotation: rotation mode one matrix builder unsupported"
        );
        for (axis, selected) in axes.into_iter().enumerate() {
            if selected {
                self.local.angles[axis] = self.initial_angles[axis];
            }
        }
        self.angle_history = self.local.angles;
        self.local.matrix = Matrix::orientation(self.local.angles);
        if self.parent.is_none() && self.effective.is_some() {
            self.effective = Some(self.local);
        }
        Ok(())
    }

    /// `ResetPosition(x, y, z)`: every axis whose flag is set takes the actor's home position (the
    /// first stored location, even for an attached actor), and the previous-position snapshot
    /// becomes the result. Returns the change per axis so the selected player's view origin can
    /// follow it; the matrix refresh is the caller's.
    pub fn reset_position(&mut self, axes: [bool; 3]) -> Result<[i32; 3]> {
        let home = *self
            .locations
            .first()
            .context("ResetPosition: home position unavailable")?;
        let before = self.local.position;
        for (axis, selected) in axes.into_iter().enumerate() {
            if selected {
                self.local.position[axis] = home[axis];
            }
        }
        self.previous_local_position = self.local.position;
        Ok(std::array::from_fn(|axis| {
            self.local.position[axis].wrapping_sub(before[axis])
        }))
    }

    /// Teleport to `position` facing `yaw`: the previous-position snapshot follows at once and the
    /// yaw change turns the matrix on the next transform refresh, the caller's.
    pub fn relocate(&mut self, position: [i32; 3], yaw: u16) {
        self.local.position = position;
        self.previous_local_position = position;
        self.local.angles[1] = yaw & 2047;
    }

    /// Restore locked axes after ordinary script/movement work, before matrix refresh.
    /// Uses current attribute bytes and the pre-update snapshot; slaves bypass restoration.
    /// The matrix is unchanged, following preservation's SOUP ordinary axis-lock rules.
    pub fn restore_axis_locks(&mut self, attributes: &[u8]) -> Result<()> {
        ensure!(
            attributes.len() == 256,
            "axis locks require 256 actor attribute bytes"
        );
        if attributes[26] == 0 {
            for (axis, attribute) in [40, 41, 42].into_iter().enumerate() {
                if attributes[attribute] != 0 {
                    self.local.position[axis] = self.previous_local_position[axis];
                }
            }
            for (axis, attribute) in [38, 37, 39].into_iter().enumerate() {
                if attributes[attribute] != 0 {
                    self.local.angles[axis] = self.angle_history[axis];
                }
            }
        }
        Ok(())
    }

    /// Refresh an ordinary unattached matrix using the retained angle snapshot.
    pub fn refresh_transform(&mut self) -> Result<()> {
        ensure!(
            self.parent.is_none(),
            "attached matrices require a family refresh"
        );
        match self.rotation_mode {
            0 if self.local.angles == self.angle_history => {}
            0 => self.local.matrix.rotate(std::array::from_fn(|axis| {
                self.local.angles[axis].wrapping_sub(self.angle_history[axis]) & 2047
            })),
            2 => self.local.matrix = Matrix::orientation(self.local.angles),
            mode => anyhow::bail!("actor rotation mode {mode} unsupported"),
        }
        self.effective = Some(self.local);
        Ok(())
    }

    /// Receiver and selected-player positions shared by `PlayerDistance` and the `RTX` distance gate.
    fn distance_positions(&self, inputs: QueryInputs) -> Result<([i32; 3], [i32; 3])> {
        let player_position = inputs
            .player
            .map(|player| player.position)
            .or(inputs.fallback_position)
            .context("player distance: player and fallback position unavailable")?;
        let receiver_position = if self.parent.is_some() {
            self.query_pose()?.position
        } else {
            inputs
                .linked_position
                .context("player distance: linked-position state unavailable")?
                .unwrap_or(self.local.position)
        };
        Ok((receiver_position, player_position))
    }

    /// `DistanceToLocation`: the rounded Euclidean length, in Q8 units, from the receiver to the
    /// selected location. A selector below zero or above the location count selects the home
    /// location; one equal to the count reads past the table in the original and is unsupported.
    pub fn location_distance(&self, location: i32, inputs: QueryInputs) -> Result<i32> {
        let receiver = if self.parent.is_some() {
            self.query_pose()?.position
        } else {
            inputs
                .linked_position
                .context("DistanceToLocation: linked-position state unavailable")?
                .unwrap_or(self.local.position)
        };
        let count = i32::try_from(self.locations.len())
            .context("DistanceToLocation: location count exceeds signed range")?;
        let target = if location < 0 || location > count {
            self.locations.first()
        } else {
            self.locations.get(location as usize)
        }
        .context("DistanceToLocation: location selector equals the location count: original out-of-range access unsupported")?;
        rounded_distance(std::array::from_fn(|axis| {
            receiver[axis].wrapping_sub(target[axis])
        }))
    }

    /// Distance the `RTX` task compares with `dialog_max_distance`: the rounded Euclidean length of
    /// the receiver-to-player difference, shifted right eight bits (arithmetic) to whole units.
    /// Unlike `PlayerDistance` it is not replaced by the `neversaydie` override.
    pub fn speaker_distance(&self, inputs: QueryInputs) -> Result<i32> {
        let (receiver, player) = self.distance_positions(inputs)?;
        let difference = [0, 1, 2].map(|axis| receiver[axis].wrapping_sub(player[axis]));
        Ok(rounded_distance(difference)? >> 8)
    }

    /// Evaluate published player distance/facing/looking/arc rules against explicit current inputs.
    /// Unsupported query names return None; absent required state is an error.
    pub fn query(
        &self,
        name: &str,
        arguments: &[cyrus_script::Argument],
        inputs: QueryInputs,
    ) -> Result<Option<i32>> {
        let accurate = name.eq_ignore_ascii_case("AcuratePlayerDistance");
        let distance = name.eq_ignore_ascii_case("PlayerDistance") || accurate;
        let facing = name.eq_ignore_ascii_case("PlayerFacing");
        let looking = name.eq_ignore_ascii_case("PlayerLooking");
        let arc = name.eq_ignore_ascii_case("PlayerArc");
        let arctan = name.eq_ignore_ascii_case("PlayerArctan");
        if !distance && !facing && !looking && !arc && !arctan {
            return Ok(None);
        }
        if arctan {
            ensure!(arguments.is_empty(), "PlayerArctan requires no arguments");
            let (receiver, player) = self.distance_positions(inputs)?;
            return Ok(Some(super::player_status::player_arctan([
                receiver[0].wrapping_sub(player[0]),
                receiver[2].wrapping_sub(player[2]),
            ])));
        }
        if arc {
            let [offset, tolerance] = arguments else {
                anyhow::bail!("PlayerArc requires two arguments");
            };
            let player = inputs
                .player
                .context("PlayerArc selected-player input unavailable")?;
            let pose = self.query_pose()?;
            let receiver = if self.parent.is_some() {
                pose.position
            } else {
                inputs
                    .linked_position
                    .context("PlayerArc linked-position state unavailable")?
                    .unwrap_or(self.local.position)
            };
            let x = receiver[0].wrapping_sub(player.position[0]);
            let z = receiver[2].wrapping_sub(player.position[2]);
            let target = i32::from(query_bearing(x, z));
            let heading = i32::from(pose.angles[1]).wrapping_sub(offset.integer()?) & 2047;
            let difference = (heading - target + 1024).rem_euclid(2048) - 1024;
            return Ok(Some(i32::from(difference.abs() <= tolerance.integer()?)));
        }
        if looking {
            let [tolerance] = arguments else {
                anyhow::bail!("PlayerLooking requires one argument");
            };
            let player = inputs
                .player
                .context("PlayerLooking selected-player input unavailable")?;
            let receiver = if self.parent.is_some() {
                self.query_pose()?.position
            } else {
                inputs
                    .linked_position
                    .context("PlayerLooking linked-position state unavailable")?
                    .unwrap_or(self.local.position)
            };
            let x = player.position[0].wrapping_sub(receiver[0]);
            let z = player.position[2].wrapping_sub(receiver[2]);
            let bearing = query_bearing(x, z);
            let target = (i32::from(player.heading) + 1024) & 2047;
            let difference = (target - i32::from(bearing) + 1024).rem_euclid(2048) - 1024;
            return Ok(Some(i32::from(difference.abs() <= tolerance.integer()?)));
        }
        if distance {
            ensure!(arguments.is_empty(), "{name} requires no arguments");
            if inputs
                .distance_override
                .context("PlayerDistance override state unavailable")?
            {
                return Ok(Some(9_999_999));
            }
            let (receiver_position, player_position) = self.distance_positions(inputs)?;
            if accurate {
                // The rounded Euclidean length of the three-axis difference, not the Manhattan sum.
                return Ok(Some(rounded_distance(std::array::from_fn(|axis| {
                    receiver_position[axis].wrapping_sub(player_position[axis])
                }))?));
            }
            let distance = receiver_position
                .into_iter()
                .zip(player_position)
                .fold(0i32, |sum, (a, b)| {
                    sum.wrapping_add(a.wrapping_sub(b).wrapping_abs())
                });
            Ok(Some(distance))
        } else {
            let player = inputs
                .player
                .context("PlayerFacing selected-player input unavailable")?;
            let pose = self.query_pose()?;
            let [offset, tolerance] = arguments else {
                anyhow::bail!("PlayerFacing requires two arguments");
            };
            let target = i32::from(pose.angles[1]).wrapping_add(offset.integer()?) & 2047;
            let difference = (i32::from(player.heading) - target + 1024).rem_euclid(2048) - 1024;
            Ok(Some(i32::from(difference.abs() <= tolerance.integer()?)))
        }
    }
}

fn serialize_matrix<S: serde::Serializer>(
    matrix: &Matrix,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    matrix.0.serialize(serializer)
}

pub use cyrus_collision::distance::rounded_distance;
use cyrus_collision::distance::single_precision_length;

/// Original single-precision normalization to Q16, using nearest-even integers.
pub(super) fn normalized_direction(differences: [i32; 3]) -> [i32; 3] {
    let divisor = single_precision_length(differences) / 65536.0;
    if divisor == 0.0 {
        return [0; 3];
    }
    differences.map(|component| ((component as f32) / divisor).round_ties_even() as i32)
}

/// Stored poses use float bits so serialization cannot round retained matrices.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub(super) struct SpatialSnapshot {
    actors: Vec<Option<SavedSpatialActor>>,
    order: Vec<usize>,
    #[serde(default)]
    refreshes: u32,
}

impl SpatialSnapshot {
    /// Pose used by off-live light preparation, without refreshing families.
    pub(super) fn actor_pose(&self, index: usize) -> Result<Option<Pose>> {
        self.actors
            .get(index)
            .context("saved light spatial actor absent")?
            .as_ref()
            .map(|actor| {
                actor
                    .effective
                    .as_ref()
                    .unwrap_or(&actor.local)
                    .clone()
                    .restore()
            })
            .transpose()
    }
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub(super) struct SavedPose {
    position: [i32; 3],
    angles: [u16; 3],
    matrix_bits: [[u32; 3]; 3],
}
impl From<Pose> for SavedPose {
    fn from(pose: Pose) -> Self {
        Self {
            position: pose.position,
            angles: pose.angles,
            matrix_bits: pose.matrix.0.map(|row| row.map(f32::to_bits)),
        }
    }
}
impl SavedPose {
    pub(super) fn restore(self) -> Result<Pose> {
        ensure!(
            self.angles.iter().all(|&angle| angle < 2048),
            "saved pose angle outside wrapped range"
        );
        let matrix = self.matrix_bits.map(|row| row.map(f32::from_bits));
        ensure!(
            matrix.iter().flatten().all(|value| value.is_finite()),
            "saved pose matrix is nonfinite"
        );
        Ok(Pose {
            position: self.position,
            angles: self.angles,
            matrix: Matrix(matrix),
        })
    }
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct SavedSpatialActor {
    local: SavedPose,
    locations: Vec<[i32; 3]>,
    effective: Option<SavedPose>,
    previous_effective_position: Option<[i32; 3]>,
    #[serde(default)]
    previous_effective_angles: Option<[u16; 3]>,
    previous_local_position: [i32; 3],
    water_rollback_position: Option<[i32; 3]>,
    angle_history: [u16; 3],
    initial_angles: [u16; 3],
    parent: Option<usize>,
    linked_position: Option<[i32; 3]>,
    #[serde(default)]
    link: Option<PositionLink>,
    #[serde(default)]
    link_cache: [i32; 3],
    #[serde(default)]
    link_previous: [i32; 3],
    no_slave_rotation: bool,
    rotation_mode: u8,
}
impl From<&ActorSpatial> for SavedSpatialActor {
    fn from(actor: &ActorSpatial) -> Self {
        Self {
            local: actor.local.into(),
            locations: actor.locations.clone(),
            effective: actor.effective.map(Into::into),
            previous_effective_position: actor.previous_effective_position,
            previous_effective_angles: actor.previous_effective_angles,
            previous_local_position: actor.previous_local_position,
            water_rollback_position: actor.water_rollback_position,
            angle_history: actor.angle_history,
            initial_angles: actor.initial_angles,
            parent: actor.parent,
            linked_position: actor.linked_position,
            link: actor.link,
            link_cache: actor.link_cache,
            link_previous: actor.link_previous,
            no_slave_rotation: actor.no_slave_rotation,
            rotation_mode: actor.rotation_mode,
        }
    }
}
impl SpatialWorld {
    pub(super) fn snapshot(&self) -> SpatialSnapshot {
        SpatialSnapshot {
            actors: self
                .actors
                .iter()
                .map(|actor| actor.as_ref().map(Into::into))
                .collect(),
            order: self.order.clone(),
            refreshes: self.refreshes,
        }
    }

    /// Retain family ownership as initialized, independently of later attribute writes.
    /// The aggregate validates installed identity; no family refresh or startup runs.
    pub(super) fn from_snapshot(snapshot: SpatialSnapshot, actors: &[ScriptActor]) -> Result<Self> {
        ensure!(
            snapshot.actors.len() == actors.len(),
            "spatial snapshot actor census differs"
        );
        let mut restored = Vec::with_capacity(actors.len());
        for (index, saved) in snapshot.actors.into_iter().enumerate() {
            let Some(saved) = saved else {
                ensure!(
                    actors[index].placement().is_none(),
                    "actor {index}: saved map spatial owner absent"
                );
                restored.push(None);
                continue;
            };
            let home = if let Some(placement) = actors[index].placement() {
                let mut home = [0; 3];
                for (axis, value) in placement.map_position.into_iter().enumerate() {
                    home[axis] = value
                        .checked_mul(256)
                        .and_then(|value| i32::try_from(value).ok())
                        .with_context(|| {
                            format!("actor {index}: installed home axis {axis} outside Q8 range")
                        })?;
                }
                home
            } else {
                ensure!(
                    matches!(
                        actors[index].source,
                        super::ScriptActorSource::Player
                            | super::ScriptActorSource::AlternatePlayer
                            | super::ScriptActorSource::Camera
                            | super::ScriptActorSource::Created(_)
                    ),
                    "actor {index}: unexpected standalone spatial owner"
                );
                ensure!(
                    saved.parent.is_none(),
                    "actor {index}: standalone parent unsupported"
                );
                *saved
                    .locations
                    .first()
                    .with_context(|| format!("actor {index}: saved home absent"))?
            };
            let locations: Vec<_> = std::iter::once(home)
                .chain(actors[index].location_offsets.iter().map(|offset| {
                    std::array::from_fn(|axis| home[axis].wrapping_add(offset[axis]))
                }))
                .collect();
            ensure!(
                saved.locations == locations,
                "actor {index}: saved authored locations differ"
            );
            ensure!(
                saved
                    .angle_history
                    .iter()
                    .chain(&saved.initial_angles)
                    .all(|&angle| angle < 2048),
                "actor {index}: saved angle history outside wrapped range"
            );
            restored.push(Some(ActorSpatial {
                local: saved
                    .local
                    .restore()
                    .with_context(|| format!("actor {index} saved local pose"))?,
                locations: saved.locations,
                effective: saved
                    .effective
                    .map(SavedPose::restore)
                    .transpose()
                    .with_context(|| format!("actor {index} saved effective pose"))?,
                previous_effective_position: saved.previous_effective_position,
                previous_effective_angles: saved.previous_effective_angles,
                previous_local_position: saved.previous_local_position,
                water_rollback_position: saved.water_rollback_position,
                angle_history: saved.angle_history,
                initial_angles: saved.initial_angles,
                parent: saved.parent,
                linked_position: saved.linked_position,
                link: saved.link,
                link_cache: saved.link_cache,
                link_previous: saved.link_previous,
                no_slave_rotation: saved.no_slave_rotation,
                rotation_mode: saved.rotation_mode,
            }));
        }
        let mut visited = std::collections::BTreeSet::new();
        for &index in &snapshot.order {
            let state = restored
                .get(index)
                .and_then(Option::as_ref)
                .with_context(|| {
                    format!("saved family order actor {index}: spatial owner absent")
                })?;
            ensure!(
                visited.insert(index),
                "saved family order duplicates actor {index}"
            );
            if let Some(parent) = state.parent {
                ensure!(
                    parent != index && visited.contains(&parent),
                    "actor {index}: saved family parent {parent} absent, cyclic or ordered after child"
                );
            }
        }
        ensure!(
            visited.len() == restored.iter().flatten().count(),
            "saved family traversal census differs"
        );
        Ok(Self {
            actors: restored,
            order: snapshot.order,
            refreshes: snapshot.refreshes,
        })
    }
}

/// Signed runtime-coordinate bearing per preservation's PlayerLooking contract.
pub(super) fn query_bearing(x: i32, z: i32) -> u16 {
    let (mut ax, mut az) = (x.wrapping_abs(), z.wrapping_abs());
    while ax > 511 || az > 511 {
        ax >>= 1;
        az >>= 1;
    }
    if ax == 0 {
        return if z < 0 { 1024 } else { 0 };
    }
    if az == 0 {
        return if x < 0 { 512 } else { 1536 };
    }
    let mut angle = if ax == az {
        if ax.wrapping_add(az) == 0 { 0 } else { 256 }
    } else {
        let (small, large) = if az > ax { (ax, az) } else { (az, ax) };
        let index = ((small.wrapping_shl(10) >> 1) / large) & 511;
        #[allow(clippy::approx_constant)] // Published original angle-table constant.
        let octant = ((f64::from(index) / 512.0).atan() * 65536.0 / 3.141592654) as i32 >> 6;
        if az < ax { 512 - octant } else { octant }
    };
    angle = match (x < 0, z < 0) {
        (true, true) => angle + 1024,
        (true, false) => -angle,
        (false, true) => 1024 - angle,
        (false, false) => angle,
    };
    ((-angle) & 2047) as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    fn receiver(position: [i32; 3], locations: Vec<[i32; 3]>) -> ActorSpatial {
        let pose = Pose {
            position,
            angles: [0; 3],
            matrix: Matrix::IDENTITY,
        };
        ActorSpatial {
            local: pose,
            locations,
            effective: None,
            previous_effective_position: None,
            previous_effective_angles: None,
            previous_local_position: position,
            water_rollback_position: None,
            angle_history: [0; 3],
            initial_angles: [0; 3],
            parent: None,
            linked_position: None,
            link: None,
            link_cache: [0; 3],
            link_previous: [0; 3],
            no_slave_rotation: false,
            rotation_mode: 0,
        }
    }

    fn inputs() -> QueryInputs {
        QueryInputs {
            player: None,
            distance_override: None,
            fallback_position: None,
            linked_position: Some(None),
        }
    }

    #[test]
    fn reset_position_restores_only_the_flagged_axes_from_home() {
        let mut actor = receiver([10, 20, 30], vec![[1, 2, 3], [100, 200, 300]]);
        let change = actor.reset_position([true, false, true]).unwrap();
        assert_eq!(actor.local.position, [1, 20, 3]);
        assert_eq!(actor.previous_local_position, [1, 20, 3]);
        assert_eq!(change, [-9, 0, -27]);
        assert_eq!(actor.reset_position([false; 3]).unwrap(), [0; 3]);
        assert!(receiver([0; 3], vec![]).reset_position([true; 3]).is_err());
    }

    #[test]
    fn relocate_moves_the_actor_and_its_previous_position_and_wraps_the_yaw() {
        let mut actor = receiver([10, 20, 30], vec![[1, 2, 3]]);
        actor.relocate([7, 8, 9], 2048 + 5);
        assert_eq!(actor.local.position, [7, 8, 9]);
        assert_eq!(actor.previous_local_position, [7, 8, 9]);
        assert_eq!(actor.local.angles[1], 5);
    }

    #[test]
    fn distance_to_location_matches_the_original_routine() {
        // Rows from the original routine in an emulator: receiver position, location, result.
        type Row = ([i32; 3], [i32; 3], i32);
        let rows: Vec<Row> = serde_json::from_str(
            r#"[
[[1000,2000,3000],[5000,6000,7000],6928],
[[1000,2000,3000],[-2560,0,256],4920],
[[1000,2000,3000],[100000,-20000,65536],119146],
[[1000,2000,3000],[1000,2000,3000],0],
[[1000,2000,3000],[-2000000000,5,5],2000001024],
[[0,0,0],[5000,6000,7000],10488],
[[0,0,0],[-2560,0,256],2573],
[[0,0,0],[100000,-20000,65536],121223],
[[0,0,0],[1000,2000,3000],3742],
[[0,0,0],[-2000000000,5,5],2000000000],
[[-5000,700,12345],[5000,6000,7000],12516],
[[-5000,700,12345],[-2560,0,256],12353],
[[-5000,700,12345],[100000,-20000,65536],119511],
[[-5000,700,12345],[1000,2000,3000],11181],
[[-5000,700,12345],[-2000000000,5,5],1999995008],
[[2000000000,0,0],[5000,6000,7000],1999995008],
[[2000000000,0,0],[-2560,0,256],2000002560],
[[2000000000,0,0],[100000,-20000,65536],1999900032],
[[2000000000,0,0],[1000,2000,3000],1999998976],
[[2000000000,0,0],[-2000000000,5,5],294967296]
]"#,
        )
        .unwrap();
        for (position, location, expected) in rows {
            let actor = receiver(position, vec![[0; 3], location]);
            assert_eq!(
                actor.location_distance(1, inputs()).unwrap(),
                expected,
                "{position:?} to {location:?}"
            );
        }
    }

    #[test]
    fn distance_to_location_selects_home_out_of_range_and_rejects_the_count() {
        let actor = receiver([0; 3], vec![[256, 0, 0], [0, 512, 0]]);
        assert_eq!(actor.location_distance(0, inputs()).unwrap(), 256);
        assert_eq!(actor.location_distance(1, inputs()).unwrap(), 512);
        assert_eq!(actor.location_distance(-1, inputs()).unwrap(), 256);
        assert_eq!(actor.location_distance(3, inputs()).unwrap(), 256);
        assert!(actor.location_distance(2, inputs()).is_err());
    }
}
