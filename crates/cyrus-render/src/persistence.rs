//! Presentation time continuation rebased independently of the host's real-time epoch.
pub(crate) mod scene;
pub use crate::console_environment::{EnvironmentEditsCandidate, EnvironmentEditsSnapshot};
pub use crate::presentation_clock::{PresentationClock, PresentationClockSnapshot};
pub use crate::sky::persistence::{SkyCandidate, SkyImages, SkySnapshot};
pub use scene::{SceneCandidate, SceneReplacement};

mod source;
pub use source::{SceneIdentityPlugin, SceneSourceIdentity};

/// Hidden replacement root excluded from live environment-dependent preparation.
/// Remove only when the prepared session is published with its own environment.
#[derive(bevy::prelude::Component)]
pub struct StagedRenderRoot;

/// A live model instance or its frame mesh is still being published.
/// Capture hosts may keep their request pending until a later rendered frame.
#[derive(Debug)]
pub struct PresentationPending(pub String);

impl std::fmt::Display for PresentationPending {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for PresentationPending {}
