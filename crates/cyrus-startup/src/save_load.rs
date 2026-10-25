//! Book-owned slot selection and host-owned storage requests.
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Storage operation requested by the installed book action.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SaveLoadRequest {
    List,
    Load { slot: u16, revision: String },
}

/// Slot metadata supplied by native files or browser IndexedDB.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SaveSlot {
    pub slot: u16,
    pub description: String,
    pub revision: String,
}

/// One operation owns the book's storage boundary until its result is consumed.
#[derive(Resource, Default)]
pub struct SaveLoadHost {
    request: Option<SaveLoadRequest>,
    busy: bool,
    result: Option<Result<Value, String>>,
}

impl SaveLoadHost {
    pub(crate) fn start(&mut self, request: SaveLoadRequest) -> Result<(), String> {
        if self.busy || self.result.is_some() {
            return Err("Save/Load operation pending or result unread.".into());
        }
        self.busy = true;
        self.request = Some(request);
        Ok(())
    }

    /// Take a request once. The host must finish only after storage or restore commits.
    pub fn take_request(&mut self) -> Option<SaveLoadRequest> {
        self.request.take()
    }

    /// Return the existing slot adapter's response, or a contextual failure.
    pub fn finish(&mut self, result: Result<Value, String>) {
        self.result = Some(result);
    }

    pub(crate) fn take_result(&mut self) -> Option<Result<Value, String>> {
        let result = self.result.take()?;
        self.busy = false;
        Some(result)
    }

    pub(crate) fn busy(&self) -> bool {
        self.busy
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_owns_request_until_terminal_result_is_consumed() {
        let mut host = SaveLoadHost::default();
        host.start(SaveLoadRequest::Load {
            slot: 17,
            revision: "revision-a".into(),
        })
        .unwrap();
        assert!(
            matches!(host.take_request(), Some(SaveLoadRequest::Load { slot: 17, revision }) if revision == "revision-a")
        );
        assert!(host.take_request().is_none());
        assert!(host.start(SaveLoadRequest::List).is_err());
        host.finish(Err("slot changed".into()));
        assert!(host.start(SaveLoadRequest::List).is_err());
        assert_eq!(host.take_result(), Some(Err("slot changed".into())));
        host.start(SaveLoadRequest::List).unwrap();
    }
}
