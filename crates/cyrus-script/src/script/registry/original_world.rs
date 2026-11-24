//! Saved rope owners bind against an independently initialized rope allocation.
use super::*;
use rgpre::import::savegame::{SaveObject, SaveRope};

impl ScriptWorld {
    /// Restore an unattached stationary original rope without advancing its oscillator.
    ///
    /// Identities belong to the initialized allocation, in native construction order.
    /// More general rope list topology and swinging geometry remain unavailable.
    /// All validation occurs before the candidate world's rope state changes.
    pub fn restore_original_ropes(
        &mut self,
        initialized_ids: &[u32],
        records: &[SaveRope],
        objects: &[SaveObject],
    ) -> Result<()> {
        anyhow::ensure!(
            initialized_ids.len() == self.ropes.len() && records.len() == self.ropes.len(),
            "world {} original ropes: initialized and saved owner counts disagree",
            self.world
        );
        anyhow::ensure!(
            records.len() <= 1,
            "original multiple-rope topology unsupported"
        );
        anyhow::ensure!(
            objects
                .iter()
                .all(|object| object.rope_id == 0 && object.attachment.is_none()),
            "original rope attachments require complete attachment binding"
        );
        let mut replacements = Vec::with_capacity(records.len());
        for (index, (&id, rope)) in initialized_ids.iter().zip(&self.ropes).enumerate() {
            let record = records
                .iter()
                .find(|record| record.rope_id == id)
                .with_context(|| {
                    format!(
                        "world {} initialized rope {id}: saved owner absent",
                        self.world
                    )
                })?;
            anyhow::ensure!(
                id != 0 && u32::from(record.runtime_identifier()) == id,
                "world {} rope {id}: runtime identity disagrees with saved owner",
                self.world
            );
            anyhow::ensure!(
                record.list_references() == [0; 3],
                "rope {id}: linked rope topology unsupported"
            );
            anyhow::ensure!(
                record.active() == 1,
                "rope {id}: inactive rope state unsupported"
            );
            anyhow::ensure!(
                usize::try_from(record.link_count()).ok() == Some(rope.links),
                "rope {id}: saved link count disagrees with initialized allocation"
            );
            let swing = record.swing();
            anyhow::ensure!(
                [
                    swing.amplitude,
                    swing.phase,
                    swing.step,
                    swing.push,
                    swing.previous_sum
                ] == [0; 5]
                    && swing.direction <= 1,
                "rope {id}: swinging published geometry reconstruction unsupported"
            );
            let position = cyrus_game::rope::world_position(
                record.position_q8().map(|value| f64::from(value) / 256.0),
            );
            let mut replacement = rope.clone();
            replacement.position = position;
            replacement.heading = record.heading();
            replacement.kind = record.kind();
            replacement.maximum = record.maximum();
            replacement.speed = record.speed();
            replacement.restore_state(&cyrus_game::rope::RopeState {
                amplitude: swing.amplitude,
                phase: swing.phase,
                step: swing.step,
                push: swing.push,
                direction: swing.direction,
                previous_sum: swing.previous_sum,
                geometry: None,
                attachments: vec![],
            });
            replacements.push((index, replacement));
        }
        for (index, replacement) in replacements {
            self.ropes[index] = replacement;
        }
        Ok(())
    }
}
