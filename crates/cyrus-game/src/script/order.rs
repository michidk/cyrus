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
    traversal_prepared: bool,
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
                ScriptActorSource::Shell(_) => {}
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
            traversal_prepared: false,
            selected: Vec::new(),
        })
    }

    pub(super) fn retained_selected(&self) -> Option<&[ScriptActorId]> {
        self.traversal_prepared.then_some(self.selected.as_slice())
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
        self.traversal_prepared = true;
        Ok(&self.selected)
    }
}

/// Cell maps use ordered entries so the snapshot is portable to JSON documents.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub(super) struct ActorListsSnapshot {
    cells: Vec<([i32; 2], Vec<usize>)>,
    persistent: Vec<usize>,
    separate_cells: Vec<([i32; 2], Vec<usize>)>,
    membership: Vec<(usize, [i32; 2])>,
    first: Option<[i32; 2]>,
    traversal_prepared: bool,
    selected: Vec<(u64, usize)>,
    separate_selected: Vec<(u64, usize)>,
}

impl ActorLists {
    pub(super) fn snapshot(&self) -> ActorListsSnapshot {
        ActorListsSnapshot {
            cells: self
                .cells
                .iter()
                .map(|(&cell, ids)| (cell, ids.clone()))
                .collect(),
            persistent: self.persistent.clone(),
            separate_cells: self
                .separate_cells
                .iter()
                .map(|(&cell, ids)| (cell, ids.clone()))
                .collect(),
            membership: self
                .membership
                .iter()
                .map(|(&index, &cell)| (index, cell))
                .collect(),
            first: self.first,
            traversal_prepared: self.traversal_prepared,
            selected: self
                .selected
                .iter()
                .map(|id| (id.generation, id.index))
                .collect(),
            separate_selected: self
                .separate_selected
                .iter()
                .map(|id| (id.generation, id.index))
                .collect(),
        }
    }

    pub(super) fn from_snapshot(
        saved: ActorListsSnapshot,
        actors: &[ScriptActor],
        saved_generation: u64,
        generation: u64,
    ) -> Result<Self> {
        use anyhow::ensure;
        use std::collections::BTreeSet;
        let installed = Self::new(actors)?;
        ensure!(
            saved.persistent == installed.persistent,
            "saved persistent actor order differs"
        );
        let decode_cells =
            |entries: Vec<([i32; 2], Vec<usize>)>| -> Result<BTreeMap<[i32; 2], Vec<usize>>> {
                let mut cells = BTreeMap::new();
                for (cell, indices) in entries {
                    ensure!(
                        cell.iter().all(|v| (0..128).contains(v)),
                        "saved actor cell {cell:?} outside map"
                    );
                    ensure!(
                        cells.insert(cell, indices).is_none(),
                        "duplicate saved actor cell {cell:?}"
                    );
                }
                Ok(cells)
            };
        let cells = decode_cells(saved.cells)?;
        let separate_cells = decode_cells(saved.separate_cells)?;
        ensure!(
            separate_cells == installed.separate_cells,
            "saved separate actor membership differs"
        );
        let mut membership = BTreeMap::new();
        for (cell, indices) in &cells {
            for &index in indices {
                ensure!(
                    installed.membership.contains_key(&index),
                    "saved cell actor {index} has wrong membership class"
                );
                ensure!(
                    membership.insert(index, *cell).is_none(),
                    "duplicate saved cell actor {index}"
                );
            }
        }
        ensure!(
            membership.keys().eq(installed.membership.keys()),
            "saved cell actor census differs"
        );
        ensure!(
            saved.membership.len() == membership.len(),
            "saved membership census differs"
        );
        let mut seen = BTreeSet::new();
        for (index, cell) in saved.membership {
            ensure!(
                seen.insert(index) && membership.get(&index) == Some(&cell),
                "actor {index}: saved cell membership inconsistent"
            );
        }
        ensure!(
            saved
                .first
                .is_none_or(|cell| cell.iter().all(|v| (0..=109).contains(v))),
            "saved view window outside map"
        );
        ensure!(
            saved.traversal_prepared
                || (saved.first.is_none()
                    && saved.selected.is_empty()
                    && saved.separate_selected.is_empty()),
            "unprepared saved traversal contains cached state"
        );
        let ordinary: BTreeSet<_> = installed
            .membership
            .keys()
            .chain(installed.persistent.iter())
            .copied()
            .collect();
        let separate: BTreeSet<_> = installed
            .separate_cells
            .values()
            .flatten()
            .copied()
            .collect();
        let rebind = |ids: Vec<(u64, usize)>,
                      allowed: &BTreeSet<usize>|
         -> Result<Vec<ScriptActorId>> {
            let mut seen = BTreeSet::new();
            ids.into_iter().map(|(old_generation, index)| {
                ensure!(old_generation == saved_generation && allowed.contains(&index) && seen.insert(index),
                    "saved traversal actor {index}: stale identity, duplicate or wrong membership");
                Ok(ScriptActorId { generation, index })
            }).collect()
        };
        let selected = rebind(saved.selected, &ordinary)?;
        let separate_selected = rebind(saved.separate_selected, &separate)?;
        if saved.traversal_prepared {
            let indices: Vec<_> = selected.iter().map(|id| id.index).collect();
            ensure!(
                indices.ends_with(&saved.persistent),
                "saved traversal lost persistent actor tail"
            );
        }
        Ok(Self {
            cells,
            persistent: saved.persistent,
            separate_cells,
            separate_selected,
            membership,
            first: saved.first,
            traversal_prepared: saved.traversal_prepared,
            selected,
        })
    }
}
