//! Renderer-independent asset catalog and portable preview contracts.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

pub mod decode;
pub mod disc;
#[cfg(not(target_arch = "wasm32"))]
pub mod install;
pub mod media;
#[cfg(not(target_arch = "wasm32"))]
pub mod native;
pub mod runtime;
pub mod source;

/// Local generated files, never the original installation.
pub const CACHE_DIR: &str = "cyrus-config/cache";
pub const CATALOG_VERSION: u32 = 3;

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
            validate_path(&entry.path)?;
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
    pub fn preview_path(&self, entry: &Entry) -> String {
        format!("viewer/{}/{}.preview", self.generation, entry.id)
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
    },
    Audio {
        wav: String,
        peaks: Vec<[f32; 2]>,
        seconds: f64,
    },
    Movie {
        smk: String,
        wav: Option<String>,
        seconds: f64,
    },
    Data {
        value: serde_json::Value,
    },
    Unavailable {
        reason: String,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Preview {
    pub content: Content,
    pub details: Vec<(String, String)>,
    /// Related catalog IDs, never arbitrary URLs.
    pub related: Vec<String>,
}
impl Preview {
    /// Files required by this preview, relative to the shared cache.
    pub fn files(&self) -> Vec<&str> {
        match &self.content {
            Content::Model { frames } => frames.iter().map(String::as_str).collect(),
            Content::Audio { wav, .. } => vec![wav],
            Content::Movie { smk, wav, .. } => std::iter::once(smk.as_str())
                .chain(wav.as_deref())
                .collect(),
            _ => Vec::new(),
        }
    }

    pub fn validate(&self) -> Result<()> {
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
            Content::Model { frames } => {
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
            Content::Movie { smk, wav, seconds } => {
                validate_path(smk)?;
                if let Some(wav) = wav {
                    validate_path(wav)?;
                }
                ensure!(
                    seconds.is_finite() && *seconds > 0.,
                    "invalid movie duration"
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

/// Portable paths cannot escape the cache or introduce URL query/scheme syntax.
pub fn validate_path(path: &str) -> Result<()> {
    ensure!(
        !path.is_empty() && !path.starts_with('/') && !path.contains(['\\', ':', '?', '#', '%']),
        "invalid asset path {path:?}"
    );
    ensure!(
        path.split('/')
            .all(|part| !part.is_empty() && part != "." && part != ".."),
        "invalid asset path {path:?}"
    );
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
        };
        let palette = Palette {
            name: "test".into(),
            colors: vec![[9, 8, 7]; 256],
        };
        assert_eq!(picture.pixels(Some(&palette)), [9, 8, 7, 0]);
        assert!(picture.validate().is_ok());
    }
}
