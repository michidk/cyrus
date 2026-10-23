//! Portable cinematic metadata and on-demand browser assets from the local export.
#[cfg(target_arch = "wasm32")]
use bevy::asset::{AssetLoader, LoadContext, io::Reader};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;
#[cfg(feature = "parsers")]
pub mod original;

#[derive(Clone, Serialize, Deserialize)]
pub struct MovieDefinition {
    pub name: String,
    pub keys: Vec<u32>,
    pub subtitles: Vec<MovieSubtitle>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct MovieSubtitle {
    pub always: bool,
    pub start: u32,
    pub stop: u32,
    pub text: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct MovieMetadata {
    pub definition: MovieDefinition,
    pub frames: u32,
    pub frame_micros: u32,
    pub has_audio: bool,
    pub font: cyrus_text::Font,
}

/// Read-only native cinematic location. CD sector interpretation belongs to preservation.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone)]
pub enum MovieInput {
    File(PathBuf),
    Disc { path: PathBuf, name: String },
}

#[derive(Asset, TypePath)]
pub struct Prepared {
    #[cfg(not(target_arch = "wasm32"))]
    pub source: MovieInput,
    #[cfg(target_arch = "wasm32")]
    pub video_url: String,
    pub metadata: MovieMetadata,
}
#[cfg(target_arch = "wasm32")]
#[derive(Default, TypePath)]
struct MovieLoader;
#[cfg(target_arch = "wasm32")]
impl AssetLoader for MovieLoader {
    type Asset = Prepared;
    type Settings = ();
    type Error = anyhow::Error;
    async fn load(
        &self,
        reader: &mut dyn Reader,
        _: &(),
        context: &mut LoadContext<'_>,
    ) -> anyhow::Result<Prepared> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        let metadata: MovieMetadata = serde_json::from_slice(&bytes)?;
        metadata.validate()?;
        Ok(Prepared {
            // A URL below the page's origin, where the web server publishes the cache.
            video_url: format!(
                "cyrus-config/cache/{}",
                context.path().path().with_extension("smk").display()
            ),
            metadata,
        })
    }
    fn extensions(&self) -> &[&str] {
        &["movie"]
    }
}

impl MovieMetadata {
    /// Verified installed cinematic viewport, independent of subtitle and skip policy.
    pub fn codec_format(&self) -> cyrus_media::MovieFormat {
        cyrus_media::MovieFormat {
            frames: self.frames,
            frame_micros: self.frame_micros,
            width: 640,
            height: 240,
            doubled_height: true,
        }
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.frames > 0 && self.frames < u32::MAX && self.frame_micros > 0,
            "movie {}: invalid frame count/timing",
            self.definition.name
        );
        anyhow::ensure!(
            self.definition
                .keys
                .iter()
                .all(|key| *key > 0 && *key <= self.frames)
                && self
                    .definition
                    .keys
                    .windows(2)
                    .all(|keys| keys[0] < keys[1]),
            "movie {}: invalid skip points",
            self.definition.name
        );
        anyhow::ensure!(
            self.font.line_height > 0
                && self
                    .font
                    .glyphs
                    .iter()
                    .all(|g| g.pixels.len() == usize::from(g.width) * usize::from(g.height))
                && self
                    .definition
                    .subtitles
                    .iter()
                    .all(|s| s.text.is_ascii() && s.start > 0 && s.start < s.stop),
            "movie {}: invalid subtitle font/text",
            self.definition.name
        );
        Ok(())
    }
}

#[cfg(target_arch = "wasm32")]
pub fn register(app: &mut App) {
    app.init_asset::<Prepared>()
        .init_asset_loader::<MovieLoader>();
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn malformed_movie_metadata_is_rejected() {
        let valid = MovieMetadata {
            definition: MovieDefinition {
                name: "TEST.SMK".into(),
                keys: vec![10, 20],
                subtitles: vec![],
            },
            frames: 20,
            frame_micros: 66660,
            has_audio: false,
            font: cyrus_text::Font {
                line_height: 2,
                ..Default::default()
            },
        };
        valid.validate().unwrap();
        let mut bad = valid.clone();
        bad.frames = 0;
        assert!(bad.validate().is_err());
        let mut bad = valid.clone();
        bad.frame_micros = 0;
        assert!(bad.validate().is_err());
        let mut bad = valid.clone();
        bad.definition.keys = vec![20, 10];
        assert!(bad.validate().is_err());
        let mut bad = valid.clone();
        bad.definition.keys = vec![21];
        assert!(bad.validate().is_err());
        let mut bad = valid.clone();
        bad.font.glyphs.push(cyrus_text::Glyph {
            width: 2,
            height: 2,
            pixels: vec![1],
            ..Default::default()
        });
        assert!(bad.validate().is_err());
        let mut bad = valid;
        bad.definition.subtitles.push(MovieSubtitle {
            always: false,
            start: 1,
            stop: 2,
            text: "é".into(),
        });
        assert!(bad.validate().is_err());
    }
}
