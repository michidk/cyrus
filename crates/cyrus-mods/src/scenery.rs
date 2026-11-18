//! Placed static scenery mods may tilt about its base, such as swaying plants.
use bevy::prelude::*;
use cyrus_render::hooks::{Scenery as Pieces, SceneryTilts};
use std::collections::HashMap;
use wasvy::prelude::*;

/// The placed static models of the loaded world and the tilts mods wrote this frame.
#[derive(Component, Reflect, Default, WasvyComponent)]
#[reflect(Component)]
pub struct Scenery {
    revision: u64,
    models: Vec<String>,
    bases: Vec<Vec<f32>>,
    tilts: HashMap<u32, Vec<f32>>,
}

// Queried `Mut` for `tilt`, so every method takes `&mut self` (see `api`).
#[wasvy::methods]
impl Scenery {
    pub fn revision(&mut self) -> u64 {
        self.revision
    }
    pub fn count(&mut self) -> u32 {
        self.models.len() as u32
    }
    pub fn model(&mut self, index: u32) -> String {
        self.models.get(index as usize).cloned().unwrap_or_default()
    }
    pub fn base(&mut self, index: u32) -> Vec<f32> {
        self.bases.get(index as usize).cloned().unwrap_or_default()
    }
    pub fn tilt(&mut self, index: u32, rotation: Vec<f32>) {
        self.tilts.insert(index, rotation);
    }
}

/// Publishes the scenery and hands this frame's valid tilts to the presentation.
pub(crate) fn publish(
    mut host: Query<&mut Scenery>,
    pieces: Res<Pieces>,
    mut tilts: ResMut<SceneryTilts>,
) {
    let Ok(mut scenery) = host.single_mut() else {
        return;
    };
    let written = std::mem::take(&mut scenery.tilts);
    // Tilts written for an older list would turn the wrong pieces.
    tilts.0 = if scenery.revision == pieces.revision {
        valid_tilts(written, pieces.pieces.len())
    } else {
        HashMap::new()
    };
    if scenery.revision != pieces.revision {
        scenery.revision = pieces.revision;
        scenery.models = pieces.pieces.iter().map(|p| p.model.clone()).collect();
    }
    scenery.bases = pieces
        .pieces
        .iter()
        .map(|p| p.base.to_array().to_vec())
        .collect();
}

/// Keeps rotations of four finite values for pieces that exist, normalized; a mod's NaN, zero
/// quaternion or stale index never reaches a transform.
fn valid_tilts(written: HashMap<u32, Vec<f32>>, count: usize) -> HashMap<u32, Quat> {
    written
        .into_iter()
        .filter(|(index, _)| (*index as usize) < count)
        .filter_map(|(index, rotation)| match rotation[..] {
            [x, y, z, w] => {
                let q = Quat::from_xyzw(x, y, z, w);
                (q.is_finite() && q.length_squared() > 1e-6).then(|| (index, q.normalize()))
            }
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_finite_rotations_of_existing_pieces_pass() {
        let written = HashMap::from([
            (0, vec![0.0, 0.0, 0.0, 2.0]),
            (1, vec![f32::NAN, 0.0, 0.0, 1.0]),
            (2, vec![0.0, 0.0, 0.0, 0.0]),
            (3, vec![0.0, 0.0, 1.0]),
            (9, vec![0.0, 0.0, 0.0, 1.0]),
        ]);
        let tilts = valid_tilts(written, 4);
        assert_eq!(tilts.len(), 1);
        assert_eq!(tilts[&0], Quat::IDENTITY);
    }
}
