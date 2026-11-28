//! Application decisions for a completely validated original-slot export.
use super::{
    CompleteSlot, ExistingSlot, Publication, Replacement, destination, publish, save_root,
};
use anyhow::{Context, Result, bail, ensure};
use std::path::{Path, PathBuf};

/// The selected destination shown to the player. Inspecting it grants no consent.
#[derive(Debug, PartialEq, Eq)]
pub enum ExportPrompt<'a> {
    /// Save a new slot without replacing a concurrent writer's slot.
    Create {
        /// Three-decimal-digit destination identity.
        slot: u16,
        /// Complete path displayed before accepting creation.
        path: &'a Path,
    },
    /// Ask the player whether to replace this exact existing slot.
    Replace {
        /// Three-decimal-digit destination identity.
        slot: u16,
        /// Existing spelling of the slot presented for replacement.
        path: &'a Path,
    },
}

/// Explicit response to the corresponding [`ExportPrompt`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportDecision {
    /// Accept creation of a new slot; never grants replacement consent.
    SaveNew,
    /// The player confirmed replacement of the slot presented by this request.
    ConfirmReplacement,
    /// Discard the prepared export without any filesystem writes.
    Cancel,
}

/// Terminal application result. A published result may contain postcommit warnings.
#[derive(Debug)]
#[must_use = "inspect the publication's cleanup/sync warnings; do not retry a committed save"]
pub enum ExportOutcome {
    /// No resources were staged or published.
    Cancelled,
    /// Publication committed; inspect its postcommit warnings.
    Published(Publication),
}

/// A held immutable payload and its destination-bound confirmation state.
///
/// Preparation performs no filesystem writes and retains no filesystem lock while
/// awaiting the player. Consuming the request prevents accidental reuse of a
/// confirmation or repeated publication. Dropping a request simply cancels it.
/// This caller does not supply an inverse mapping or replace native config saves.
#[derive(Debug)]
#[must_use = "present the prompt and respond, or drop the request to cancel"]
pub struct ExportRequest {
    root: PathBuf,
    slot: u16,
    path: PathBuf,
    payload: CompleteSlot,
    existing: Option<ExistingSlot>,
}

impl ExportRequest {
    /// Prepare one complete payload before inspecting or prompting for a target.
    ///
    /// `produce` must validate every required native inverse mapping without
    /// filesystem mutation. Its contextual unsupported-state error is preserved.
    /// There is no default producer: a partial record export cannot satisfy this
    /// interface. `root` is the existing real install SAVEGAME directory.
    pub fn prepare(
        root: &Path,
        slot: u16,
        produce: impl FnOnce() -> Result<CompleteSlot>,
    ) -> Result<Self> {
        (|| {
            ensure!(slot <= 999, "slot identity must fit three decimal digits");
            let payload = produce()?;
            let root = save_root(root)?;
            let existing = destination(&root, slot)?
                .map(|_| ExistingSlot::inspect(&root, slot))
                .transpose()?;
            let path = existing.as_ref().map_or_else(
                || root.join(format!("SAVEGAME.{slot:03}")),
                |selected| selected.path().to_owned(),
            );
            Ok(Self {
                root,
                slot,
                path,
                payload,
                existing,
            })
        })()
        .with_context(|| {
            format!(
                "preparing original export SAVEGAME.{slot:03} in {}",
                root.display()
            )
        })
    }

    /// Show the selected target without publishing or changing the held payload.
    pub fn prompt(&self) -> ExportPrompt<'_> {
        if self.existing.is_some() {
            ExportPrompt::Replace {
                slot: self.slot,
                path: &self.path,
            }
        } else {
            ExportPrompt::Create {
                slot: self.slot,
                path: &self.path,
            }
        }
    }

    /// Consume the player's decision for this request, never a later recapture.
    ///
    /// Wrong-prompt decisions reject before writes. Publication rechecks existing
    /// slot identity/content, or rejects a newly occupied slot without overwriting
    /// it. A stale request must be prepared and presented again for fresh consent.
    pub fn respond(self, decision: ExportDecision) -> Result<ExportOutcome> {
        let context = format!(
            "original export SAVEGAME.{:03} decision {decision:?} in {}",
            self.slot,
            self.root.display()
        );
        (|| {
            let replacement = match (decision, self.existing) {
                (ExportDecision::Cancel, _) => return Ok(ExportOutcome::Cancelled),
                (ExportDecision::SaveNew, None) => Replacement::Forbid,
                (ExportDecision::ConfirmReplacement, Some(selected)) => {
                    Replacement::ConfirmedByPlayer(selected)
                }
                _ => bail!("export decision does not match the presented slot prompt"),
            };
            publish(&self.root, self.slot, &self.payload, replacement).map(ExportOutcome::Published)
        })()
        .with_context(|| context)
    }
}
