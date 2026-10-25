//! Book-owned slot selection and host-owned storage requests.
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Storage operation requested by the installed book action.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SaveLoadRequest {
    List,
    Create {
        description: String,
    },
    Overwrite {
        slot: u16,
        revision: String,
        description: String,
    },
    Load {
        slot: u16,
        revision: String,
    },
    Delete {
        slot: u16,
        revision: String,
    },
}

/// Original description input retains prefilled case and uppercases newly typed ASCII.
pub(super) struct DescriptionEditor {
    pub text: String,
    pub slot: Option<SaveSlot>,
    pub background: image::RgbaImage,
}

impl DescriptionEditor {
    pub fn append(&mut self, text: &str) {
        for ch in text
            .chars()
            .filter(|ch| matches!(ch, '\u{b}' | '\r' | ' '..='~'))
        {
            if self.text.len() == 64 {
                break;
            }
            self.text.push(ch.to_ascii_uppercase());
        }
    }

    pub fn request(&self) -> Option<SaveLoadRequest> {
        if self.text.is_empty() {
            return None;
        }
        Some(match &self.slot {
            Some(slot) => SaveLoadRequest::Overwrite {
                slot: slot.slot,
                revision: slot.revision.clone(),
                description: self.text.clone(),
            },
            None => SaveLoadRequest::Create {
                description: self.text.clone(),
            },
        })
    }

    pub fn display(&self) -> String {
        let mut chars: Vec<_> = self.text.chars().collect();
        let mut cursor = 13;
        while cursor + 1 < chars.len() {
            if chars[cursor] == ' ' {
                chars[cursor] = '\n';
                cursor += 14;
            } else {
                cursor += 1;
            }
        }
        chars.into_iter().collect()
    }
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
    fn description_preserves_prefill_and_limits_new_original_characters() {
        let mut editor = DescriptionEditor {
            text: "Mixed case ".into(),
            slot: Some(SaveSlot {
                slot: 18,
                description: "Mixed case ".into(),
                revision: "r18".into(),
            }),
            background: image::RgbaImage::new(640, 480),
        };
        editor.append("new é\t\r\u{b}");
        assert_eq!(editor.text, "Mixed case NEW \r\u{b}");
        editor.append(&"x".repeat(80));
        assert_eq!(editor.text.len(), 64);
        assert!(
            matches!(editor.request(), Some(SaveLoadRequest::Overwrite { slot: 18, revision, description }) if revision == "r18" && description.starts_with("Mixed case NEW"))
        );
        editor.text = "0123456789012 4567890123456 890 ".into();
        assert_eq!(editor.display(), "0123456789012\n4567890123456\n890 ");
        assert_eq!(editor.text, "0123456789012 4567890123456 890 ");
        editor.text.clear();
        assert!(editor.request().is_none());
        editor.slot = None;
        editor.append("harbour");
        assert!(
            matches!(editor.request(), Some(SaveLoadRequest::Create { description }) if description == "HARBOUR")
        );
    }

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
