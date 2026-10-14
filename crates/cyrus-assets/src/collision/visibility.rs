//! Portable model-admission octree in original map coordinates, without render culling.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

/// Present empty preparation means this map has no PVO restriction; missing cache data
/// cannot establish admission. Node IDs are cache indices, not placed-object IDs.
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct ModelVisibility {
    pub center: [i32; 3],
    pub half_extent: u32,
    pub nodes: Vec<VisibilityNode>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct VisibilityNode {
    pub children: [Option<usize>; 8],
    /// Original object IDs in runtime PLST decoding order, retaining duplicates.
    pub objects: Vec<u16>,
}

impl ModelVisibility {
    pub fn validate(&self) -> Result<()> {
        if self.nodes.is_empty() {
            return Ok(());
        }
        ensure!(
            self.half_extent > 0,
            "model visibility has zero root extent"
        );
        let mut states = vec![0u8; self.nodes.len()];
        for root in 0..self.nodes.len() {
            let mut pending = vec![(root, false)];
            while let Some((index, exiting)) = pending.pop() {
                let node = self
                    .nodes
                    .get(index)
                    .with_context(|| format!("model visibility node {index} outside tree"))?;
                if exiting {
                    states[index] = 2;
                    continue;
                }
                match states[index] {
                    2 => continue,
                    1 => anyhow::bail!("model visibility node {index} forms a cycle"),
                    _ => {}
                }
                states[index] = 1;
                pending.push((index, true));
                pending.extend(node.children.iter().flatten().map(|&child| (child, false)));
            }
        }
        Ok(())
    }
}
