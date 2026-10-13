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
    pub effective: Option<Pose>,
    pub previous_effective_position: Option<[i32; 3]>,
    /// Local position captured at the ordinary update boundary, in Q8 units.
    pub previous_local_position: [i32; 3],
    /// Previous local angles after snapshot; propagated deltas after family refresh.
    pub angle_history: [u16; 3],
    pub parent: Option<usize>,
    /// Fresh allocation has no linked-position record; dynamic creation is unsupported.
    pub(super) linked_position: Option<[i32; 3]>,
    no_slave_rotation: bool,
    rotation_mode: u8,
}

pub(super) struct SpatialWorld {
    actors: Vec<Option<ActorSpatial>>,
    order: Vec<usize>,
}

impl SpatialWorld {
    pub fn prepare(actors: &[ScriptActor]) -> Result<Self> {
        let mut poses = Vec::with_capacity(actors.len());
        let mut masters = BTreeMap::new();
        for (index, actor) in actors.iter().enumerate() {
            let placement = actor
                .placement()
                .filter(|_| actor.runtime_identifier.is_some());
            let pose = placement.map(|placement| -> Result<_> {
                let mut position = [0; 3];
                for (axis, value) in placement.map_position.into_iter().enumerate() {
                    position[axis] = value.checked_mul(256).and_then(|value| i32::try_from(value).ok())
                        .with_context(|| format!("actor {index} {} map position axis {axis}: outside Q8 i32 range", actor.name()))?;
                }
                let angles = placement.angles.map(|angle| (angle & 2047) as u16);
                Ok(Pose { position, angles, matrix: Matrix::orientation(angles) })
            }).transpose()?;
            if pose.is_some() && actor.attributes()[25] != 0 {
                ensure!(
                    masters.insert(actor.attributes()[25], index).is_none(),
                    "attachment group {}: duplicate masters require original traversal ordering",
                    actor.attributes()[25]
                );
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
                effective: parent.is_none().then_some(original),
                previous_effective_position: None,
                previous_local_position: local.position,
                angle_history: original.angles,
                parent,
                linked_position: None,
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
        })
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

    /// Copy local position and angles at the ordinary actor-update boundary.
    pub fn snapshot(&mut self) {
        self.previous_local_position = self.local.position;
        self.angle_history = self.local.angles;
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

    /// Evaluate published player distance/facing rules against explicit current inputs.
    /// Unsupported query names return None; absent required state is an error.
    pub fn query(
        &self,
        name: &str,
        arguments: &[cyrus_script::Argument],
        inputs: QueryInputs,
    ) -> Result<Option<i32>> {
        let distance = name.eq_ignore_ascii_case("PlayerDistance");
        let facing = name.eq_ignore_ascii_case("PlayerFacing");
        if !distance && !facing {
            return Ok(None);
        }
        if distance {
            ensure!(arguments.is_empty(), "PlayerDistance requires no arguments");
            if inputs
                .distance_override
                .context("PlayerDistance override state unavailable")?
            {
                return Ok(Some(9_999_999));
            }
            let player_position = inputs
                .player
                .map(|player| player.position)
                .or(inputs.fallback_position)
                .context("PlayerDistance player and fallback position unavailable")?;
            let receiver_position = if self.parent.is_some() {
                self.query_pose()?.position
            } else {
                inputs
                    .linked_position
                    .context("PlayerDistance linked-position state unavailable")?
                    .unwrap_or(self.local.position)
            };
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

/// Original mixed-precision Euclidean distance, rounded to a signed integer.
pub fn rounded_distance(differences: [i32; 3]) -> Result<i32> {
    // Stored f32 differences are integers; their squared sum fits u64 exactly.
    let squared: u64 = differences
        .into_iter()
        .map(|difference| {
            let difference = difference as f32 as i64;
            (difference * difference) as u64
        })
        .sum();
    let squared = squared as f64;
    let root = squared.sqrt();
    let mut single = root as f32;
    let neighbor = if f64::from(single) > root {
        single.next_down()
    } else {
        single.next_up()
    };
    let midpoint = (f64::from(single) + f64::from(neighbor)) / 2.0;
    // Correct a double-rounded square root at a single-precision midpoint.
    if root == midpoint {
        let boundary = midpoint * midpoint;
        if squared < boundary {
            single = single.min(neighbor);
        } else if squared > boundary {
            single = single.max(neighbor);
        }
    }
    let length = single.round_ties_even();
    ensure!(
        f64::from(length) <= f64::from(i32::MAX),
        "distance exceeds signed integer range"
    );
    Ok(length as i32)
}
