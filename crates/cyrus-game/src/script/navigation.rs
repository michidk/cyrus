//! Empty-map Wander follows preservation's SOUP navigation and distance contracts.
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
    maps: Vec<Vec<u8>>,
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
                Ok(vec![0; nodes.len()])
            })
            .collect::<Result<_>>()?;
        Ok(Self { maps })
    }

    pub(super) fn map_count(&self) -> usize {
        self.maps.len()
    }

    fn node(&mut self, map: usize, node: usize) -> Result<&mut u8> {
        self.maps
            .get_mut(map)
            .and_then(|nodes| nodes.get_mut(node))
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

    pub(super) fn reset(&mut self) {
        for nodes in &mut self.maps {
            nodes.fill(0);
        }
    }
}
