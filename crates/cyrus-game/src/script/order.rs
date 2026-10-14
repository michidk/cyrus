//! Fresh-world ordinary actor membership and cached view-window traversal.
use super::{ScriptActor, ScriptActorId, ScriptActorSource};
use anyhow::Result;
use std::collections::BTreeMap;

pub(super) struct ActorLists {
    cells: BTreeMap<[i32; 2], Vec<usize>>,
    persistent: Vec<usize>,
    separate_cells: BTreeMap<[i32; 2], Vec<usize>>,
    separate_selected: Vec<ScriptActorId>,
    membership: BTreeMap<usize, [i32; 2]>,
    first: Option<[i32; 2]>,
    selected: Vec<ScriptActorId>,
}

impl ActorLists {
    pub(super) fn new(actors: &[ScriptActor]) -> Result<Self> {
        let mut cells: BTreeMap<_, Vec<_>> = BTreeMap::new();
        let mut persistent = Vec::new();
        let mut separate_cells: BTreeMap<[i32; 2], Vec<usize>> = BTreeMap::new();
        let mut membership = BTreeMap::new();
        for (index, actor) in actors.iter().enumerate().rev() {
            match &actor.source {
                ScriptActorSource::Map(placement) => {
                    if matches!(placement.object_type, 2 | 6) {
                        let position = [placement.map_position[0], placement.map_position[2]];
                        anyhow::ensure!(
                            position.iter().all(|value| (0..=65535).contains(value)),
                            "instance {index} actor {}: separate cell position {position:?} outside supported map",
                            actor.name()
                        );
                        separate_cells
                            .entry(position.map(|value| (value / 512) as i32))
                            .or_default()
                            .push(index);
                        continue;
                    }
                    if placement.active != 0 {
                        persistent.push(index);
                    } else {
                        let position = [placement.map_position[0], placement.map_position[2]];
                        anyhow::ensure!(
                            position.iter().all(|v| (0..=65535).contains(v)),
                            "instance {index} actor {}: ordinary cell position {position:?} outside supported map",
                            actor.name()
                        );
                        let cell = position.map(|v| (v / 512) as i32);
                        membership.insert(index, cell);
                        cells.entry(cell).or_default().push(index);
                    }
                }
                _ => persistent.push(index),
            }
        }
        Ok(Self {
            cells,
            persistent,
            separate_cells,
            separate_selected: Vec::new(),
            membership,
            first: None,
            selected: Vec::new(),
        })
    }

    pub(super) fn separate_selected(&self) -> &[ScriptActorId] {
        &self.separate_selected
    }

    /// Complete load-time cell scan for the PVO secondary actor table.
    pub(super) fn secondary_actors(
        &self,
        actors: &[ScriptActor],
        generation: u64,
    ) -> Vec<ScriptActorId> {
        let mut cells: Vec<_> = self.cells.iter().collect();
        cells.sort_by_key(|(cell, _)| [cell[1], cell[0]]);
        cells
            .into_iter()
            .flat_map(|(_, indices)| indices.iter())
            .chain(self.persistent.iter())
            .copied()
            .filter(|&index| actors[index].attributes()[122] != 0)
            .map(|index| ScriptActorId { generation, index })
            .collect()
    }

    /// Deferred relocation retains cached traversal until a distant cell change invalidates it.
    pub(super) fn relocate(
        &mut self,
        index: usize,
        local_position: [i32; 3],
        linked_position: Option<[i32; 3]>,
        view: [i32; 3],
    ) -> Result<()> {
        let Some(previous) = self.membership.get(&index).copied() else {
            return Ok(());
        };
        let position = linked_position.unwrap_or(local_position);
        let cell = [position[0], position[2]].map(|v| ((v >> 8) & 65535) / 512);
        if cell == previous {
            return Ok(());
        }
        let distance = super::spatial::rounded_distance([
            view[0].wrapping_sub(local_position[0]),
            0,
            view[2].wrapping_sub(local_position[2]),
        ])?;
        if let Some(actors) = self.cells.get_mut(&previous) {
            actors.retain(|&actor| actor != index);
        }
        self.cells.entry(cell).or_default().insert(0, index);
        self.membership.insert(index, cell);
        if distance >> 8 > 3584 {
            self.first = None;
        }
        Ok(())
    }

    pub(super) fn select(
        &mut self,
        actors: &[ScriptActor],
        generation: u64,
        view: [i32; 2],
    ) -> Result<&[ScriptActorId]> {
        anyhow::ensure!(
            view.iter().all(|v| (0..=65535).contains(v)),
            "ordinary actor view {view:?} outside supported map"
        );
        let first = view.map(|v| {
            (if v < 4608 {
                0
            } else if v + 4608 > 65535 {
                56319
            } else {
                v - 4608
            }) / 512
        });
        if self.first == Some(first) {
            return Ok(&self.selected);
        }
        self.selected.clear();
        self.separate_selected.clear();
        for z in 0..18 {
            for x in 0..18 {
                let border_x = x == 0 || x == 17;
                let border_z = z == 0 || z == 17;
                if border_x && border_z {
                    continue;
                }
                if let Some(cell) = self.separate_cells.get(&[first[0] + x, first[1] + z]) {
                    self.separate_selected.extend(
                        cell.iter()
                            .map(|&index| ScriptActorId { generation, index }),
                    );
                }
                if let Some(cell) = self.cells.get(&[first[0] + x, first[1] + z]) {
                    for &index in cell {
                        if !(border_x || border_z) || actors[index].attributes()[113] == 0 {
                            self.selected.push(ScriptActorId { generation, index });
                        }
                    }
                }
            }
        }
        self.selected.extend(
            self.persistent
                .iter()
                .map(|&index| ScriptActorId { generation, index }),
        );
        self.first = Some(first);
        Ok(&self.selected)
    }
}
