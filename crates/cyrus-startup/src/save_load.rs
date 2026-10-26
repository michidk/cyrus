//! Book-owned slot selection and host-owned storage requests.
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Gameplay image retained while the book covers the live view.
#[derive(Resource, Default)]
pub struct GameplayThumbnail(pub Option<Vec<u8>>);

/// Requests a screenshot before book presentation replaces the gameplay view.
pub(super) fn capture_gameplay(
    mut commands: Commands,
    next: Res<NextState<cyrus_game::AppState>>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
) {
    if !matches!(*next, NextState::Pending(cyrus_game::AppState::Menu)) || windows.single().is_err()
    {
        return;
    }
    use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
    commands.remove_resource::<GameplayThumbnail>();
    commands.spawn(Screenshot::primary_window()).observe(
        |event: On<ScreenshotCaptured>, mut commands: Commands| match encode_thumbnail(
            event.image.clone(),
        ) {
            Ok(bytes) => commands.insert_resource(GameplayThumbnail(Some(bytes))),
            Err(error) => {
                bevy::log::error!("gameplay snapshot: {error:#}");
                commands.insert_resource(GameplayThumbnail(None));
            }
        },
    );
}

fn encode_thumbnail(image: Image) -> anyhow::Result<Vec<u8>> {
    let image = image.try_into_dynamic()?;
    let resized = image.resize_to_fill(256, 192, image::imageops::FilterType::Triangle);
    let mut output = std::io::Cursor::new(Vec::new());
    resized.write_to(&mut output, image::ImageFormat::Png)?;
    Ok(output.into_inner())
}

/// A thumbnail describes one book visit; saving from live gameplay stores none.
pub(super) fn discard_gameplay(mut commands: Commands) {
    commands.remove_resource::<GameplayThumbnail>();
}

/// Storage operation requested by the installed book action.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SaveLoadRequest {
    List,
    /// Reserved automatic save written when Quit leaves a live world.
    AutoSave,
    /// Removes the reserved automatic save when Quit leaves a dead world.
    DiscardAutoSave,
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
    /// PNG bytes for the 256x192 gameplay view, when captured.
    #[serde(default)]
    pub thumbnail: Option<Vec<u8>>,
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
    fn gameplay_thumbnail_keeps_aspect_and_is_a_256x192_png() {
        let image = Image::new_fill(
            bevy::render::render_resource::Extent3d {
                width: 1280,
                height: 720,
                depth_or_array_layers: 1,
            },
            bevy::render::render_resource::TextureDimension::D2,
            &[10, 20, 30, 255],
            bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
            default(),
        );
        let decoded = image::load_from_memory(&encode_thumbnail(image).unwrap()).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (256, 192));
    }

    #[test]
    fn description_preserves_prefill_and_limits_new_original_characters() {
        let mut editor = DescriptionEditor {
            text: "Mixed case ".into(),
            slot: Some(SaveSlot {
                slot: 18,
                description: "Mixed case ".into(),
                revision: "r18".into(),
                thumbnail: None,
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
