//! Walk-node ownership, free-node search and empty-map Wander follow preservation contracts.
use super::spatial::{ActorSpatial, rounded_distance};
use crate::random::EngineRandom;
use anyhow::{Result, ensure};

pub(super) fn wander_without_maps(
    home: [i32; 3],
    radius: i32,
    frame_scale: u32,
    spatial: &mut ActorSpatial,
    random: &mut EngineRandom,
) -> Result<i32> {
    let radius = radius >> 8;
    ensure!(
        radius > 0,
        "Wander radius must contain a positive map-unit radius"
    );
    let current = spatial.query_pose()?.position.map(|v| v / 256);
    loop {
        let x = home[0]
            .wrapping_add(radius)
            .wrapping_sub(random.rnd(radius * 2)?);
        let z = home[2]
            .wrapping_add(radius)
            .wrapping_sub(random.rnd(radius * 2)?);
        if rounded_distance([x.wrapping_sub(current[0]), 0, z.wrapping_sub(current[2])])?
            > radius / 2
        {
            break;
        }
    }
    let previous = spatial.previous_local_position;
    let displacement = rounded_distance([
        spatial.local.position[0].wrapping_sub(previous[0]),
        0,
        spatial.local.position[2].wrapping_sub(previous[2]),
    ])?;
    let limit = (frame_scale.wrapping_shl(13) as i32) >> 8;
    if displacement > limit {
        spatial.local.position = previous;
    }
    Ok(displacement)
}

/// Runtime ownership bytes in WDNM file-map and node-index order.
/// Loading discards authored reserved bytes. Acquisition/release use the low byte
/// of the actor identifier; periodic sound maintenance clears every reservation.
#[derive(Debug)]
pub(super) struct WalkNodeReservations {
    maps: Vec<Vec<WalkNode>>,
    selected_map: Option<usize>,
}

#[derive(Debug)]
struct WalkNode {
    position: [i32; 3],
    owner: u8,
}

impl WalkNodeReservations {
    pub(super) fn prepare(metadata: &serde_json::Value, world: u32) -> Result<Self> {
        let maps = metadata
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("world {world} walk_node_maps absent or invalid"))?;
        let maps = maps
            .iter()
            .enumerate()
            .map(|(map, value)| {
                let nodes = value["walk_nodes"].as_array().ok_or_else(|| {
                    anyhow::anyhow!(
                        "world {world} walk_node_maps map {map}: walk_nodes absent or invalid"
                    )
                })?;
                ensure!(
                    value["node_count"].as_u64() == Some(nodes.len() as u64),
                    "world {world} walk_node_maps map {map}: node_count does not match walk_nodes"
                );
                nodes
                    .iter()
                    .enumerate()
                    .map(|(index, node)| {
                        let mut position = [0; 3];
                        for (axis, field) in ["node_pos_x", "node_pos_y", "node_pos_z"]
                            .into_iter()
                            .enumerate()
                        {
                            let range = if axis == 1 {
                                i64::from(i16::MIN)..=i64::from(i16::MAX)
                            } else {
                                0..=i64::from(u16::MAX)
                            };
                            let coordinate = node[field]
                                .as_i64()
                                .filter(|value| range.contains(value))
                                .ok_or_else(|| anyhow::anyhow!(
                                    "world {world} walk_node_maps map {map} node {index}: {field} absent or invalid"
                                ))?;
                            position[axis] = coordinate as i32;
                        }
                        Ok(WalkNode { position, owner: 0 })
                    })
                    .collect::<Result<Vec<_>>>()
            })
            .collect::<Result<_>>()?;
        Ok(Self {
            maps,
            selected_map: None,
        })
    }

    pub(super) fn map_count(&self) -> usize {
        self.maps.len()
    }

    fn node(&mut self, map: usize, node: usize) -> Result<&mut u8> {
        self.maps
            .get_mut(map)
            .and_then(|nodes| nodes.get_mut(node))
            .map(|node| &mut node.owner)
            .ok_or_else(|| anyhow::anyhow!("walk-node map {map} node {node}: unavailable"))
    }

    pub(super) fn acquire(&mut self, map: usize, node: usize, actor: i16) -> Result<bool> {
        let owner = self.node(map, node)?;
        let actor = actor as u8;
        if *owner != 0 && *owner != actor {
            return Ok(false);
        }
        *owner = actor;
        Ok(true)
    }

    pub(super) fn release(&mut self, map: usize, node: usize, actor: i16) -> Result<()> {
        let owner = self.node(map, node)?;
        if *owner == actor as u8 {
            *owner = 0;
        }
        Ok(())
    }

    /// The combat search filters reservation bytes and retains the winning map.
    /// An improving candidate below twelve returns before later, closer nodes.
    pub(super) fn nearest_free(
        &mut self,
        position: [i32; 3],
        height: i32,
    ) -> Option<(usize, usize)> {
        let mut position = position.map(|coordinate| coordinate >> 8);
        position[1] = position[1].wrapping_add(height);
        self.selected_map = None;
        let mut best = 4096;
        let mut selected = None;
        // Runtime loading prepends each map; stored indices remain in file order.
        for (map, nodes) in self.maps.iter().enumerate().rev() {
            for (index, node) in nodes.iter().enumerate() {
                if node.owner != 0 {
                    continue;
                }
                let distance = position
                    .iter()
                    .zip(node.position)
                    .fold(0i32, |sum, (a, b)| {
                        sum.wrapping_add(a.wrapping_sub(b).wrapping_abs())
                    });
                if distance < best {
                    self.selected_map = Some(map);
                    selected = Some((map, index));
                    if distance < 12 {
                        return selected;
                    }
                    best = distance;
                }
            }
        }
        selected
    }

    pub(super) fn reset(&mut self) {
        for nodes in &mut self.maps {
            for node in nodes {
                node.owner = 0;
            }
        }
    }
}

/// Reservation bytes retain low-byte ownership, including identifiers that alias.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub(super) struct WalkNodeSnapshot {
    maps: Vec<Vec<u8>>,
    #[serde(default)]
    selected_map: Option<usize>,
}

impl WalkNodeReservations {
    pub(super) fn snapshot(&self) -> WalkNodeSnapshot {
        WalkNodeSnapshot {
            maps: self
                .maps
                .iter()
                .map(|nodes| nodes.iter().map(|node| node.owner).collect())
                .collect(),
            selected_map: self.selected_map,
        }
    }

    /// The aggregate checks exact world content before supplying installed metadata.
    pub(super) fn from_snapshot(
        snapshot: WalkNodeSnapshot,
        metadata: &serde_json::Value,
        world: u32,
    ) -> Result<Self> {
        let mut installed = Self::prepare(metadata, world)?;
        ensure!(
            snapshot.maps.len() == installed.maps.len(),
            "world {world}: saved walk-node map count differs"
        );
        for (map, (saved, expected)) in snapshot.maps.iter().zip(&installed.maps).enumerate() {
            ensure!(
                saved.len() == expected.len(),
                "world {world} walk-node map {map}: saved node count differs"
            );
        }
        ensure!(
            snapshot
                .selected_map
                .is_none_or(|map| map < installed.maps.len()),
            "world {world}: saved selected walk-node map unavailable"
        );
        for (saved, nodes) in snapshot.maps.into_iter().zip(&mut installed.maps) {
            for (owner, node) in saved.into_iter().zip(nodes) {
                node.owner = owner;
            }
        }
        installed.selected_map = snapshot.selected_map;
        Ok(installed)
    }
}
