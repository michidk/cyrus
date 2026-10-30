//! Immutable world-entry actor lighting inputs, captured before ordinary updates.
use super::{ScriptActorId, ScriptWorld};
use anyhow::{Context, Result, ensure};
use std::collections::BTreeMap;

/// A static actor's prepared instance at the prelight checkpoint. Float bits
/// preserve the original instance matrix independently of later presentation.
/// See preservation's actor-prelighting entry and restoration order.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ActorPrelight {
    model: String,
    position: [u32; 3],
    matrix: [[u32; 3]; 3],
}

impl ActorPrelight {
    /// Resolved static resource key, not an animation-table frame name.
    pub fn model(&self) -> &str {
        &self.model
    }

    /// Prepared translation in original map units, without scene-origin conversion.
    pub fn position(&self) -> [f32; 3] {
        self.position.map(f32::from_bits)
    }

    /// Row-major original instance matrix, without render-axis conversion.
    pub fn matrix(&self) -> [[f32; 3]; 3] {
        self.matrix.map(|row| row.map(f32::from_bits))
    }
}

impl ScriptWorld {
    /// Seal the final fresh or restored entry before ordinary updates. Repeated
    /// calls preserve this generation's checkpoint despite later script changes.
    pub fn prepare_actor_prelighting(&mut self) -> Result<()> {
        if self.actor_prelighting.is_none() {
            self.actor_prelighting = Some(self.actor_prelighting_inputs()?);
        }
        Ok(())
    }

    /// Read a generation-checked actor checkpoint. Missing readiness is an error;
    /// a ready actor outside the static/nonmoving path returns `None`.
    pub fn actor_prelight(&self, actor: ScriptActorId) -> Result<Option<&ActorPrelight>> {
        let index = self.resolve(actor)?;
        Ok(self
            .actor_prelighting
            .as_ref()
            .with_context(|| format!("world {} actor prelighting checkpoint pending", self.world))?
            .get(&index))
    }

    pub(super) fn actor_prelighting_inputs(&self) -> Result<BTreeMap<usize, ActorPrelight>> {
        let mut result = BTreeMap::new();
        for (index, actor) in self.actors.iter().enumerate() {
            if actor.attributes()[122] == 0
                || actor
                    .placement()
                    .is_none_or(|placement| placement.object_type != 1)
            {
                continue;
            }
            ensure!(
                actor.model.prepared,
                "world {} actor {index}: prelight model pending",
                self.world
            );
            if actor.model.static_radius.is_none() {
                continue;
            }
            let model = actor.model.static_model.as_ref().with_context(|| {
                format!(
                    "world {} actor {index}: saved static prelight model identity unavailable",
                    self.world
                )
            })?;
            let position = actor.model.position.with_context(|| {
                format!(
                    "world {} actor {index}: prepared prelight translation unavailable",
                    self.world
                )
            })?;
            let matrix = actor.model.matrix.with_context(|| {
                format!(
                    "world {} actor {index}: prepared prelight matrix unavailable",
                    self.world
                )
            })?;
            ensure!(
                position
                    .iter()
                    .chain(matrix.iter().flatten())
                    .all(|value| value.is_finite()),
                "world {} actor {index}: nonfinite prelight transform",
                self.world
            );
            result.insert(
                index,
                ActorPrelight {
                    model: model.clone(),
                    position: position.map(f32::to_bits),
                    matrix: matrix.map(|row| row.map(f32::to_bits)),
                },
            );
        }
        Ok(result)
    }
}
