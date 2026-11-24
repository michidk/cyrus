//! Installed fixture ownership and public runtime assertions.
use super::*;
mod admission;
mod host;
mod inputs;
pub use admission::AdmissionCompletion;
pub use host::{HostFixture, prepare_host};
pub use inputs::InstalledSources;
