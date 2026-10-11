//! Portable cinematic metadata and on-demand browser assets from the local export.
#[cfg(target_arch = "wasm32")]
use bevy::asset::{AssetLoader, LoadContext, io::Reader};
use bevy::{audio::AudioSource, prelude::*};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct MovieDefinition {
    pub name: String,
    pub keys: Vec<u32>,
    pub subtitles: Vec<MovieSubtitle>,
}
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct MovieSubtitle {
    pub always: bool,
    pub start: u32,
    pub stop: u32,
    pub text: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct MovieMetadata {
    pub definition: MovieDefinition,
    pub frames: u32,
    pub frame_micros: u32,
    pub has_audio: bool,
    pub font: cyrus_text::Font,
    pub line_height: u16,
    pub font_color: [u8; 3],
}

#[derive(Asset, TypePath)]
pub(super) struct Prepared {
    pub data: Arc<[u8]>,
    pub metadata: MovieMetadata,
    pub audio: Option<AudioSource>,
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
        let video_path = bevy::asset::AssetPath::from(context.path().path().with_extension("smk"))
            .with_source(context.path().source().clone_owned());
        let data = context.read_asset_bytes(video_path).await?;
        let smk = smk::Smk::open_memory(&data)?;
        let info = smk.info();
        let video = smk.info_video();
        anyhow::ensure!(
            video.width == 640
                && video.height == 240
                && matches!(
                    video.y_scale,
                    smk::YScaleMode::Double | smk::YScaleMode::Interlace
                ),
            "movie {}: unsupported viewport",
            metadata.definition.name
        );
        anyhow::ensure!(
            info.frame_count == metadata.frames
                && (info.microseconds_per_frame - f64::from(metadata.frame_micros)).abs() < 0.5,
            "movie {}: cache/header mismatch; re-export startup",
            metadata.definition.name
        );
        let audio = if metadata.has_audio {
            let path = bevy::asset::AssetPath::from(context.path().path().with_extension("wav"))
                .with_source(context.path().source().clone_owned());
            Some(AudioSource {
                bytes: context.read_asset_bytes(path).await?.into(),
            })
        } else {
            None
        };
        Ok(Prepared {
            data: data.into(),
            metadata,
            audio,
        })
    }
    fn extensions(&self) -> &[&str] {
        &["movie"]
    }
}

impl MovieMetadata {
    pub(super) fn validate(&self) -> anyhow::Result<()> {
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
            self.line_height > 0
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
pub(super) fn register(app: &mut App) {
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
            font: cyrus_text::Font::default(),
            line_height: 2,
            font_color: [240; 3],
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
