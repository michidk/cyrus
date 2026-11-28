//! Book-owned slot selection and host-owned storage requests.
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Gameplay image retained while the book covers the live view.
#[derive(Resource, Default)]
pub struct GameplayThumbnail(pub Option<Vec<u8>>);

/// The single gameplay screenshot request shared by native packed-image and PNG consumers.
#[derive(Component)]
pub struct GameplayScreenshot;

/// Requests a screenshot before book presentation replaces the gameplay view.
pub(super) fn capture_gameplay(
    mut commands: Commands,
    next: Res<NextState<cyrus_game::AppState>>,
    ending: Option<Res<cyrus_script::script::EndingEntry>>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
) {
    // The book after the ending shows an empty picture frame in the original.
    if !matches!(*next, NextState::Pending(cyrus_game::AppState::Menu))
        || ending.is_some()
        || windows.single().is_err()
    {
        return;
    }
    use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
    commands.remove_resource::<GameplayThumbnail>();
    commands
        .spawn((Screenshot::primary_window(), GameplayScreenshot))
        .observe(|event: On<ScreenshotCaptured>, mut commands: Commands| {
            match encode_thumbnail(event.image.clone()) {
                Ok(bytes) => commands.insert_resource(GameplayThumbnail(Some(bytes))),
                Err(error) => {
                    bevy::log::error!("gameplay snapshot: {error:#}");
                    commands.insert_resource(GameplayThumbnail(None));
                }
            }
        });
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

/// Original slots require complete restoration; unsupported owners reject the load.
pub const ORIGINAL_IMPORT_LIMITATION: &str =
    "Original save import restores the complete session. Unsupported saves cannot be loaded.";

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
    /// Read-only original slot; hosts must never route this to native storage.
    ImportOriginal {
        slot: u16,
        world_id: u32,
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
        if self.text.is_empty()
            || self
                .slot
                .as_ref()
                .is_some_and(|slot| slot.source != SaveSlotSource::Native)
        {
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

/// Storage provenance keeps original install slots out of native write operations.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SaveSlotSource {
    #[default]
    Native,
    Original {
        world_id: u32,
    },
}

/// Slot metadata supplied by native files or browser IndexedDB.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SaveSlot {
    #[serde(default)]
    pub source: SaveSlotSource,
    pub slot: u16,
    pub description: String,
    pub revision: String,
    /// PNG bytes for the 256x192 gameplay view, when captured.
    #[serde(default)]
    pub thumbnail: Option<Vec<u8>>,
}

impl SaveSlot {
    pub(crate) fn load_request(&self) -> Result<SaveLoadRequest, String> {
        match self.source {
            SaveSlotSource::Native => Ok(SaveLoadRequest::Load {
                slot: self.slot,
                revision: self.revision.clone(),
            }),
            SaveSlotSource::Original { world_id } => Ok(SaveLoadRequest::ImportOriginal {
                slot: self.slot,
                world_id,
            }),
        }
    }
}

/// One operation owns the book's storage boundary until its result is consumed.
#[derive(Resource, Default)]
pub struct SaveLoadHost {
    request: Option<SaveLoadRequest>,
    busy: bool,
    result: Option<Result<Value, String>>,
}

impl SaveLoadHost {
    /// Submit a menu or automation request, preserving its storage provenance.
    pub fn start(&mut self, request: SaveLoadRequest) -> Result<(), String> {
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

    /// Consume completion and release ownership for the next menu operation.
    pub fn take_result(&mut self) -> Option<Result<Value, String>> {
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
    fn one_book_readback_serves_png_and_host_observers() {
        use bevy::{
            ecs::system::RunSystemOnce,
            render::view::screenshot::{Screenshot, ScreenshotCaptured},
        };
        #[derive(Resource, Default)]
        struct Observed(bool);
        let mut app = App::new();
        app.insert_resource(NextState::Pending(cyrus_game::AppState::Menu));
        app.init_resource::<Observed>();
        app.world_mut()
            .spawn((Window::default(), bevy::window::PrimaryWindow));
        app.world_mut().run_system_once(capture_gameplay).unwrap();
        let entity = app
            .world_mut()
            .query_filtered::<Entity, (With<Screenshot>, With<GameplayScreenshot>)>()
            .single(app.world())
            .unwrap();
        app.add_observer(
            |_: On<ScreenshotCaptured>, mut observed: ResMut<Observed>| observed.0 = true,
        );
        let image = Image::new_fill(
            bevy::render::render_resource::Extent3d {
                width: 640,
                height: 480,
                depth_or_array_layers: 1,
            },
            bevy::render::render_resource::TextureDimension::D2,
            &[20, 40, 60, 255],
            bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
            default(),
        );
        app.world_mut()
            .trigger(ScreenshotCaptured { entity, image });
        app.world_mut().flush();
        assert!(app.world().resource::<Observed>().0);
        let png = app
            .world()
            .resource::<GameplayThumbnail>()
            .0
            .as_ref()
            .unwrap();
        let decoded = image::load_from_memory(png).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (256, 192));
        assert_eq!(
            app.world_mut()
                .query::<&Screenshot>()
                .iter(app.world())
                .count(),
            1
        );
    }

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
                source: SaveSlotSource::Native,
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

#[cfg(test)]
mod original_slot_tests {
    use super::*;
    #[test]
    fn original_selection_keeps_provenance_at_the_host_boundary() {
        let slot = SaveSlot {
            source: SaveSlotSource::Original { world_id: 15 },
            slot: 2,
            description: "ORIGINAL".into(),
            revision: String::new(),
            thumbnail: None,
        };
        assert!(matches!(
            slot.load_request(),
            Ok(SaveLoadRequest::ImportOriginal {
                slot: 2,
                world_id: 15
            })
        ));
        let mut host = SaveLoadHost::default();
        host.start(slot.load_request().unwrap()).unwrap();
        assert!(matches!(
            host.take_request(),
            Some(SaveLoadRequest::ImportOriginal {
                slot: 2,
                world_id: 15
            })
        ));
        let native = SaveSlot {
            source: SaveSlotSource::Native,
            revision: "r2".into(),
            ..slot
        };
        assert!(matches!(
            native.load_request(),
            Ok(SaveLoadRequest::Load { slot: 2, .. })
        ));
    }
    #[test]
    fn old_host_metadata_defaults_to_native() {
        let slot: SaveSlot = serde_json::from_value(serde_json::json!({
            "slot":2,"description":"NATIVE","revision":"r2"
        }))
        .unwrap();
        assert_eq!(slot.source, SaveSlotSource::Native);
    }
}
