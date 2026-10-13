//! Renderer-independent asset catalog and portable preview contracts.
pub mod cheats;
pub mod console;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[cfg(feature = "parsers")]
pub mod decode;
pub mod disc;
#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
pub mod install;
#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
pub mod native;
#[cfg(feature = "parsers")]
pub mod runtime;
#[cfg(feature = "parsers")]
pub mod source;

/// Local generated files, never the original installation.
pub const CACHE_DIR: &str = "cyrus-config/cache";
pub const CATALOG_VERSION: u32 = 19;

/// Content categories, independent of source file extensions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Category {
    Images,
    Palettes,
    Fonts,
    Models,
    Worlds,
    Audio,
    Speech,
    Movies,
    Data,
    Other,
}
impl Category {
    pub const ALL: [Self; 10] = [
        Self::Images,
        Self::Palettes,
        Self::Fonts,
        Self::Models,
        Self::Worlds,
        Self::Audio,
        Self::Speech,
        Self::Movies,
        Self::Data,
        Self::Other,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Images => "Images",
            Self::Palettes => "Palettes",
            Self::Fonts => "Fonts",
            Self::Models => "Models",
            Self::Worlds => "Worlds",
            Self::Audio => "Audio",
            Self::Speech => "Speech",
            Self::Movies => "Movies",
            Self::Data => "Definitions",
            Self::Other => "Other files",
        }
    }
}

/// An install-relative file plus a typed member index preserves duplicate names.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Source {
    File,
    Archive,
    Image(usize),
    Model(usize),
    Effect(usize),
    Speech(usize),
    Splash(usize),
    World(u32),
    CdTrack(u32),
    CdMovie(String),
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    pub name: String,
    pub path: String,
    pub category: Category,
    pub format: String,
    pub source: Source,
    /// Size of the containing source file, shared by its archive members.
    pub bytes: u64,
    pub error: Option<String>,
}
impl Entry {
    /// Archive members list under their archive, never under a file format.
    pub fn is_member(&self) -> bool {
        matches!(
            self.source,
            Source::Image(_) | Source::Model(_) | Source::Effect(_) | Source::Speech(_)
        )
    }
    /// Numbered texture banks share a format; derived previews are not source files.
    pub fn format_label(&self) -> &str {
        match self.source {
            Source::World(_) => "World previews",
            Source::CdTrack(_) => "CD audio",
            _ if self
                .path
                .rsplit('/')
                .next()
                .is_some_and(|name| name.to_ascii_uppercase().starts_with("TEXBSI.")) =>
            {
                "TEXBSI"
            }
            _ if self.category == Category::Other => "Other",
            _ => &self.format,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Palette {
    pub name: String,
    pub colors: Vec<[u8; 3]>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Catalog {
    pub version: u32,
    pub generation: String,
    pub entries: Vec<Entry>,
    pub palettes: Vec<Palette>,
}
impl Catalog {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.version == CATALOG_VERSION,
            "catalog schema changed; reload the asset library"
        );
        validate_identity(&self.generation)?;
        let mut ids = std::collections::HashSet::new();
        for entry in &self.entries {
            validate_identity(&entry.id)?;
            validate_source_path(&entry.path)?;
            ensure!(
                ids.insert(&entry.id),
                "duplicate catalog identity {}",
                entry.id
            );
        }
        for palette in &self.palettes {
            ensure!(
                palette.colors.len() == 256,
                "palette {}: expected 256 colors",
                palette.name
            );
        }
        Ok(())
    }
    pub fn preview_path(&self, entry: &Entry, palette: Option<&str>) -> String {
        let variant = palette
            .map(|p| format!("-{}", blake3::hash(p.as_bytes()).to_hex()))
            .unwrap_or_default();
        format!("viewer/{}/{}{variant}.preview", self.generation, entry.id)
    }
}

/// Decoded RGBA image; indices permit palette inspection without re-export.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Picture {
    pub label: String,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub indices: Option<Vec<u8>>,
    /// The decoded RGBA contains colors from the image's own palette.
    pub embedded_palette: bool,
}
impl Picture {
    pub fn validate(&self) -> Result<()> {
        let pixels = u64::from(self.width) * u64::from(self.height);
        ensure!(
            pixels > 0 && pixels <= 16_777_216,
            "{}: invalid image dimensions",
            self.label
        );
        ensure!(
            self.rgba.len() as u64 == pixels * 4,
            "{}: invalid RGBA length",
            self.label
        );
        if let Some(indices) = &self.indices {
            ensure!(
                indices.len() as u64 == pixels,
                "{}: invalid index length",
                self.label
            );
        }
        Ok(())
    }
    /// Recolors an indexed preview while retaining the decoder's alpha channel.
    pub fn pixels(&self, palette: Option<&Palette>) -> Vec<u8> {
        let mut rgba = self.rgba.clone();
        if let (Some(indices), Some(palette)) = (&self.indices, palette) {
            for (pixel, index) in rgba.as_chunks_mut::<4>().0.iter_mut().zip(indices) {
                if let Some(color) = palette.colors.get(*index as usize) {
                    pixel[..3].copy_from_slice(color);
                }
            }
        }
        rgba
    }
}

/// A decoded visibility cell in the same scene coordinates as exported geometry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpatialCell {
    pub center: [f32; 3],
    pub half_extent: f32,
    pub depth: u32,
    pub visible_indices: usize,
    /// Preserved placed-object visibility IDs for this cell.
    pub visibility_ids: Vec<u16>,
}

/// Preview payloads are produced once and read by both native and browser UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Content {
    Images {
        frames: Vec<Picture>,
    },
    Font {
        glyphs: Vec<Picture>,
        first: u16,
        line_height: u16,
    },
    Palette {
        colors: Vec<[u8; 3]>,
    },
    Model {
        frames: Vec<String>,
        palettes: Vec<String>,
        data: Option<serde_json::Value>,
    },
    Spatial {
        scene: Option<String>,
        cells: Vec<SpatialCell>,
        value: serde_json::Value,
    },
    Audio {
        wav: String,
        peaks: Vec<[f32; 2]>,
        seconds: f64,
    },
    Movie {
        smk: String,
        audio_tracks: Vec<MovieAudioTrack>,
        seconds: f64,
    },
    Data {
        value: serde_json::Value,
    },
    Unavailable {
        reason: String,
    },
}
/// One authored Smacker audio track, decoded to independently playable PCM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MovieAudioTrack {
    /// Zero-based track slot in the source movie.
    pub index: u8,
    pub wav: String,
    pub sample_rate: u32,
    pub channels: u16,
    pub bits_per_sample: u16,
    pub sample_frames: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Preview {
    pub content: Content,
    /// Parsed inspection fields, excluding pixel and audio payloads.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
    /// Authored image frame interval in milliseconds, when supplied by the format.
    #[serde(default)]
    pub frame_interval_ms: Option<f64>,
    pub details: Vec<(String, String)>,
    /// Related catalog IDs, never arbitrary URLs.
    pub related: Vec<String>,
}
impl Preview {
    /// Files required by this preview, relative to the shared cache.
    pub fn files(&self) -> Vec<&str> {
        match &self.content {
            Content::Model { frames, .. } => frames.iter().map(String::as_str).collect(),
            Content::Audio { wav, .. } => vec![wav],
            Content::Spatial { scene, .. } => scene.iter().map(String::as_str).collect(),
            Content::Movie {
                smk, audio_tracks, ..
            } => std::iter::once(smk.as_str())
                .chain(audio_tracks.iter().map(|track| track.wav.as_str()))
                .collect(),
            _ => Vec::new(),
        }
    }

    pub fn validate(&self) -> Result<()> {
        if let Some(interval) = self.frame_interval_ms {
            ensure!(
                interval.is_finite() && interval > 0.,
                "invalid frame interval"
            );
        }
        for id in &self.related {
            validate_identity(id)?;
        }
        match &self.content {
            Content::Images { frames } | Content::Font { glyphs: frames, .. } => {
                ensure!(!frames.is_empty(), "preview has no images");
                for frame in frames {
                    frame.validate()?;
                }
            }
            Content::Model { frames, .. } => {
                ensure!(!frames.is_empty(), "preview has no model");
                for path in frames {
                    validate_path(path)?;
                }
            }
            Content::Audio {
                wav,
                peaks,
                seconds,
            } => {
                validate_path(wav)?;
                ensure!(
                    seconds.is_finite() && *seconds >= 0.,
                    "invalid audio duration"
                );
                ensure!(
                    peaks.iter().flatten().all(|n| n.is_finite()),
                    "invalid waveform"
                );
            }
            Content::Movie {
                smk,
                audio_tracks,
                seconds,
            } => {
                validate_path(smk)?;
                let mut slots = [false; 7];
                for track in audio_tracks {
                    validate_path(&track.wav)?;
                    ensure!(track.index < 7, "invalid movie audio track index");
                    ensure!(
                        !slots[usize::from(track.index)],
                        "duplicate movie audio track"
                    );
                    slots[usize::from(track.index)] = true;
                    ensure!(
                        track.sample_rate > 0
                            && matches!(track.channels, 1 | 2)
                            && matches!(track.bits_per_sample, 8 | 16),
                        "invalid movie audio format"
                    );
                }
                ensure!(
                    seconds.is_finite() && *seconds > 0.,
                    "invalid movie duration"
                );
            }
            Content::Spatial { cells, scene, .. } => {
                if let Some(scene) = scene {
                    validate_path(scene)?;
                }
                ensure!(
                    cells
                        .iter()
                        .all(|cell| cell.center.iter().all(|n| n.is_finite())
                            && cell.half_extent.is_finite()
                            && cell.half_extent > 0.),
                    "invalid visibility cell bounds"
                );
            }
            Content::Palette { colors } => ensure!(colors.len() == 256, "invalid palette"),
            _ => {}
        }
        Ok(())
    }
}

fn validate_identity(value: &str) -> Result<()> {
    ensure!(
        value.len() == 64
            && value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "invalid catalog identity {value:?}"
    );
    Ok(())
}

/// Relative original-file paths may contain literal `#` characters in filenames.
/// HTTP adapters must encode each component before constructing a URL.
pub fn validate_source_path(path: &str) -> Result<()> {
    ensure!(
        !path.is_empty() && !path.starts_with('/') && !path.contains(['\\', ':', '?', '%']),
        "invalid asset path {path:?}"
    );
    ensure!(
        path.split('/')
            .all(|part| !part.is_empty() && part != "." && part != ".."),
        "invalid asset path {path:?}"
    );
    Ok(())
}

/// Portable paths cannot escape the cache or introduce URL query/scheme syntax.
pub fn validate_path(path: &str) -> Result<()> {
    validate_source_path(path)?;
    ensure!(!path.contains('#'), "invalid asset path {path:?}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn manifest_paths_stay_inside_cache() {
        for invalid in [
            "../install",
            "/tmp/file",
            "x/../../y",
            "https://host",
            "x%2f..",
            "x?y",
            "ENGLISH_wav/#bon.wav",
            "a\\b",
            "a//b",
        ] {
            assert!(validate_path(invalid).is_err(), "{invalid}");
        }
        assert!(validate_path("viewer/revision/model.glb").is_ok());
    }
    #[test]
    fn palette_inspection_retains_transparency() {
        let picture = Picture {
            label: "pixel".into(),
            width: 1,
            height: 1,
            rgba: vec![1, 2, 3, 0],
            indices: Some(vec![0]),
            embedded_palette: true,
        };
        let palette = Palette {
            name: "test".into(),
            colors: vec![[9, 8, 7]; 256],
        };
        assert_eq!(picture.pixels(Some(&palette)), [9, 8, 7, 0]);
        assert!(picture.validate().is_ok());
    }
}

#[cfg(feature = "parsers")]
pub mod game;
#[cfg(feature = "parsers")]
mod preparation;

pub mod animation;
#[cfg(feature = "parsers")]
pub mod animation_export;
#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
pub mod audio;
pub mod collision;
pub mod environment;
#[cfg(feature = "parsers")]
pub mod environment_export;
pub mod hud;
#[cfg(feature = "parsers")]
pub mod hud_export;
#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
pub mod loading;
pub mod menu;
pub mod movie;
pub mod player;
#[cfg(feature = "parsers")]
pub mod player_export;
pub mod preferences;
#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
pub mod scene;
pub mod script;
#[cfg(feature = "parsers")]
pub mod script_export;
pub mod sound;
#[cfg(feature = "parsers")]
pub mod sound_export;
#[cfg(feature = "parsers")]
pub mod startup;

#[cfg(feature = "parsers")]
pub mod music;

/// Number of cached Windows Glide splash frames, including the authored fade.
pub const SPLASH_FRAMES: usize = 150;
