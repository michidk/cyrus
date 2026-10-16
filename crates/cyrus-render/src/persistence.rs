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
