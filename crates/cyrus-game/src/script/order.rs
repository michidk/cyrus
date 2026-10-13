//! Fresh-world ordinary actor membership and cached view-window traversal.
use super::{ScriptActor, ScriptActorId, ScriptActorSource};
use anyhow::Result;
use std::collections::BTreeMap;

pub(super) struct ActorLists {
    cells: BTreeMap<[i32; 2], Vec<usize>>,
    persistent: Vec<usize>,
    first: Option<[i32; 2]>,
    selected: Vec<ScriptActorId>,
}

impl ActorLists {
    pub(super) fn new(actors: &[ScriptActor]) -> Result<Self> {
        let mut cells: BTreeMap<_, Vec<_>> = BTreeMap::new();
        let mut persistent = Vec::new();
        for (index, actor) in actors.iter().enumerate().rev() {
            match &actor.source {
                ScriptActorSource::Map(placement) => {
                    if matches!(placement.object_type, 2 | 6) {
                        continue;
                    }
                    if placement.active != 0 {
                        persistent.push(index);
                    } else {
                        let position = [
                            placement.map_position[0] >> 8,
                            placement.map_position[2] >> 8,
                        ];
                        anyhow::ensure!(
                            position.iter().all(|v| (0..=65535).contains(v)),
                            "instance {index} actor {}: ordinary cell position {position:?} outside supported map",
                            actor.name()
                        );
                        cells
                            .entry(position.map(|v| v as i32 / 512))
                            .or_default()
                            .push(index);
                    }
                }
                _ => persistent.push(index),
            }
        }
        Ok(Self {
            cells,
            persistent,
            first: None,
            selected: Vec::new(),
        })
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
        for z in 0..18 {
            for x in 0..18 {
                let border_x = x == 0 || x == 17;
                let border_z = z == 0 || z == 17;
                if border_x && border_z {
                    continue;
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
