//! Original family lookup tables retain vectors independently of native traversal.
use super::*;
use crate::script::spatial::FamilyGroup;
use rgpre::import::savegame::{SaveActorBinding, SaveActorOwners, SaveFamily, SaveObject};

impl ScriptWorld {
    /// Bind all 256 group slots without refreshing a family or actor link cache.
    /// The initialized family traversal remains intact; it overwrites the visited
    /// masters' vector entries on the next pass. Empty slots retain saved vectors.
    pub fn restore_original_families(
        &mut self,
        bindings: &[(ScriptActorId, &SaveObject)],
        saved: &[SaveFamily],
    ) -> Result<()> {
        let mut initialized = Vec::with_capacity(bindings.len());
        let mut objects = Vec::with_capacity(bindings.len());
        let mut receivers = Vec::with_capacity(bindings.len());
        let mut seen = BTreeSet::new();
        for &(id, object) in bindings {
            let index = self.resolve(id)?;
            anyhow::ensure!(
                seen.insert(index),
                "original family duplicate actor receiver {index}"
            );
            let actor = &self.actors[index];
            let expected = match &actor.source {
                ScriptActorSource::Player => -2,
                ScriptActorSource::AlternatePlayer => -3,
                ScriptActorSource::Camera => 2,
                ScriptActorSource::Map(placement)
                    if !placement.object_type.has_separate_record() =>
                {
                    i32::from(
                        actor
                            .runtime_identifier
                            .context("original family runtime identity absent")?
                            as u16,
                    )
                }
                _ => anyhow::bail!("original family actor {index}: unsupported saved owner"),
            };
            anyhow::ensure!(
                expected == object.object_id,
                "original family actor {index}: saved identity differs"
            );
            initialized.push(SaveActorBinding {
                object_id: expected,
                runtime_identifier: actor
                    .runtime_identifier
                    .context("original family runtime identity absent")?
                    as u16,
            });
            objects.push(object.clone());
            receivers.push(index);
        }
        let owners = SaveActorOwners::bind(&objects, &initialized)?;
        let groups = saved
            .iter()
            .map(|group| -> Result<_> {
                let master = if matches!(group.actor_id, 0 | -1) {
                    None
                } else {
                    Some(receivers[owners.receiver(group.actor_id)?])
                };
                Ok(FamilyGroup {
                    master,
                    position_q8: group.position_q8,
                    angles: group.angles,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        self.spatial
            .as_mut()
            .context("original family spatial owner absent")?
            .restore_original_families(groups)
    }
}
