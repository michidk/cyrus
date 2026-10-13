//! The browser's effect source and the portable cue message every sound trigger sends.
//! Native builds export the bank next to the scenes; the browser loads it and plays
//! cues through the same [`crate::sound`] mixer as the native build.
#[cfg(target_arch = "wasm32")]
use bevy::asset::{AssetLoader, LoadContext, io::Reader};
use bevy::prelude::*;

/// Loaded sound bank with presentation-owned audio dependencies.
#[derive(Asset, TypePath)]
pub struct SoundBank {
    pub data: cyrus_assets::sound::SoundBank,
    #[dependency]
    pub audio: Vec<Handle<AudioSource>>,
}
impl std::ops::Deref for SoundBank {
    type Target = cyrus_assets::sound::SoundBank;
    fn deref(&self) -> &Self::Target {
        &self.data
    }
}
#[cfg(target_arch = "wasm32")]
#[derive(Default, TypePath)]
pub(crate) struct SoundBankLoader;
#[cfg(target_arch = "wasm32")]
impl AssetLoader for SoundBankLoader {
    type Asset = SoundBank;
    type Settings = ();
    type Error = anyhow::Error;
    async fn load(
        &self,
        reader: &mut dyn Reader,
        _: &(),
        context: &mut LoadContext<'_>,
    ) -> Result<SoundBank, Self::Error> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        let data: cyrus_assets::sound::SoundBank = anyhow::Context::context(
            serde_json::from_slice(&bytes),
            "sounds/bank.sounds: invalid sound metadata; re-export sound assets",
        )?;
        let audio = (0..data.effects.len())
            .map(|i| context.load(format!("cache://sounds/{i:03}.wav")))
            .collect();
        Ok(SoundBank { data, audio })
    }
    fn extensions(&self) -> &[&str] {
        &["sounds"]
    }
}

#[cfg(target_arch = "wasm32")]
/// Browser effects preparation handle; hosts inspect load failures before entering the menu.
#[derive(Resource)]
pub struct SoundBankHandle(pub Handle<SoundBank>);
