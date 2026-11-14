//! Gameplay services retain host ownership of audio and transactional storage.
use crate::bindings::cyrus::mods::components as raw;

/// The currently displayed speech line, independently of topic selection.
pub struct Dialogue {
    pub(crate) raw: raw::Dialogue,
    pub(crate) id: &'static str,
}
impl Dialogue {
    /// True only during eligible gameplay dialogue with a displayed line.
    pub fn available(&self) -> bool {
        self.raw.available()
    }
    /// Queue completion of one displayed line and its audio. Repeated requests coalesce.
    pub fn advance(&self) -> bool {
        self.raw.advance(self.id)
    }
}

/// Terminal results describe actual native storage completion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveStatus {
    Idle,
    Pending,
    Saved,
    Failed,
}

/// Requests use the reserved autosave slot; manual saves are never overwritten.
pub struct Saves {
    pub(crate) raw: raw::Saves,
    pub(crate) id: &'static str,
}
impl Saves {
    /// Eligible live gameplay and an idle storage boundary. Revalidated on consumption.
    pub fn ready(&self) -> bool {
        self.raw.ready()
    }
    /// Queue an autosave, returning whether it was accepted, not whether it committed.
    pub fn save(&self) -> bool {
        self.raw.save(self.id)
    }
    /// Last status for this guest. A new accepted request replaces its terminal result.
    pub fn status(&self) -> SaveStatus {
        match self.raw.status(self.id).as_str() {
            "pending" => SaveStatus::Pending,
            "saved" => SaveStatus::Saved,
            "failed" => SaveStatus::Failed,
            _ => SaveStatus::Idle,
        }
    }
    /// Contextual storage failure for the most recent request, or empty.
    pub fn error(&self) -> String {
        self.raw.error(self.id)
    }
}
