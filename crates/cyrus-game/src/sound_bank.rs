//! The browser's effect source and the portable cue message every sound trigger sends.
//! Native builds export the bank next to the scenes; the browser loads it and plays
//! cues through the same [`crate::sound`] mixer as the native build.
#[cfg(target_arch = "wasm32")]
use bevy::asset::{AssetLoader, LoadContext, io::Reader};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// A sound trigger from game code. Consumed by [`crate::sound::SoundPlugin`] on
/// native and browser builds; senders never play audio themselves.
#[derive(Message, Debug)]
pub enum SoundCue {
    /// The original's `FlatSound`: pan 64, volume `clamp(base_volume + volume, 0, 127)`,
    /// rate `clamp(sample_rate + rate_offset_hz, 512, 44000)` Hz, no owner.
    Flat {
        effect: usize,
        volume: i32,
        rate_offset_hz: i32,
    },
    /// An authored animation sound of `actor`, which must carry a `GlobalTransform` and
    /// a [`SoundSurface`]; without the surface its starts are reported and not played.
    Animation {
        actor: Entity,
        sound: crate::animation::AnimationSound,
    },
}

/// The SURFACE.INI surface type of the face an actor stands on (preservation
/// `docs/engine/sound.md`), set by the code that knows the ground; 0 means no remap.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct SoundSurface(pub u8);

/// `MAIN.SFX` metadata and the `SYSTEM.INI` values the mixer needs, exported for the browser.
#[derive(Asset, TypePath, Deserialize, Serialize)]
pub struct SoundBank {
    pub effects: Vec<SoundEffect>,
    /// `[system] sound`; when off no effect plays.
    pub enabled: bool,
    /// `[system] volume`; 256 when absent.
    pub master_volume: i32,
    /// `[system] sound_distance`; `None` when absent.
    pub sound_distance: Option<i32>,
    #[serde(skip)]
    #[dependency]
    pub audio: Vec<Handle<AudioSource>>,
}

#[derive(Deserialize, Serialize)]
pub struct SoundEffect {
    pub looping: bool,
    pub sample_rate: u32,
    pub channels: u16,
    pub bits_per_sample: u16,
    /// SFX header byte `0x0C`, the volume of non-positional starts.
    pub base_volume: u8,
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
        let mut bank: SoundBank = serde_json::from_slice(&bytes)?;
        bank.audio = (0..bank.effects.len())
            .map(|i| context.load(format!("cache://sounds/{i:03}.wav")))
            .collect();
        Ok(bank)
    }
    fn extensions(&self) -> &[&str] {
        &["sounds"]
    }
}

#[cfg(target_arch = "wasm32")]
#[derive(Resource)]
pub(crate) struct SoundBankHandle(pub Handle<SoundBank>);
