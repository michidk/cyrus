//! CD track decoder checkpoints and off-live music replacement construction.
use super::Music;
use crate::sound::{Panned, PannedDecoder, PannedGains, PannedSnapshot, PcmWidth};
use anyhow::{Result, ensure};
use bevy::{
    audio::{AudioSource, Decodable, PlaybackSettings, Volume},
    prelude::{Asset, TypePath},
};
use serde::{Deserialize, Serialize};

/// Prepared CD PCM with exact source-decoder progress. Drive gain stays on the
/// sink; this wrapper applies unity scaling to the prepared stereo16 samples.
#[derive(Asset, TypePath)]
pub struct MusicTransport {
    transport: Panned,
    #[cfg(target_arch = "wasm32")]
    clock: std::sync::atomic::AtomicU32,
    #[cfg(target_arch = "wasm32")]
    pub(super) prepared: std::sync::atomic::AtomicU32,
}
impl Clone for MusicTransport {
    fn clone(&self) -> Self {
        Self::wrap(self.transport.clone())
    }
}
impl MusicTransport {
    fn wrap(transport: Panned) -> Self {
        Self {
            transport,
            #[cfg(target_arch = "wasm32")]
            clock: std::sync::atomic::AtomicU32::new(0),
            #[cfg(target_arch = "wasm32")]
            prepared: std::sync::atomic::AtomicU32::new(0),
        }
    }
    /// Prepare browser device buffers without starting or replacing live audio.
    /// Native playback retains its normal backend preparation.
    fn prepare_output(&self, settings: &PlaybackSettings) -> Result<()> {
        #[cfg(target_arch = "wasm32")]
        {
            let id = super::browser::prepare(self, settings)?;
            self.prepared
                .store(id, std::sync::atomic::Ordering::Relaxed);
        }
        #[cfg(not(target_arch = "wasm32"))]
        let _ = settings;
        Ok(())
    }

    /// Switch prepared browser output at a validated exclusive publication boundary.
    /// Call immediately before applying the infallible entity replacement queue.
    pub fn publish_output(
        &self,
        world: &mut bevy::prelude::World,
        entity: bevy::prelude::Entity,
        settings: &PlaybackSettings,
    ) -> Result<()> {
        #[cfg(target_arch = "wasm32")]
        super::browser::publish(self, world, entity, settings)?;
        #[cfg(not(target_arch = "wasm32"))]
        let _ = (world, entity, settings);
        Ok(())
    }

    /// Validate a host-prepared stereo16 CD WAV without starting playback.
    pub fn new(source: AudioSource) -> Result<Self> {
        Ok(Self::wrap(Panned::with_playback(
            source,
            PannedGains::new(PcmWidth::Sixteen, 127, 256, 64),
            true,
            1.0,
        )?))
    }

    /// Immutable prepared bytes for the owning track, never serialized.
    pub fn source(&self) -> &AudioSource {
        self.transport.source()
    }

    /// The decoder owns looping; apply linear drive gain and pause to these settings.
    pub fn playback_settings(&self) -> PlaybackSettings {
        self.transport.playback_settings()
    }

    fn snapshot(&self) -> Result<PannedSnapshot> {
        let snapshot = self.transport.snapshot()?;
        #[cfg(target_arch = "wasm32")]
        {
            let clock = self.clock.load(std::sync::atomic::Ordering::Relaxed);
            if clock != 0 {
                return super::browser::snapshot(self.source(), snapshot, clock);
            }
        }
        Ok(snapshot)
    }

    #[cfg(target_arch = "wasm32")]
    pub(super) fn browser_clock(&self) -> &std::sync::atomic::AtomicU32 {
        &self.clock
    }

    #[cfg(target_arch = "wasm32")]
    pub(super) fn initial_snapshot(&self) -> Result<PannedSnapshot> {
        self.transport.snapshot()
    }

    fn from_snapshot(source: AudioSource, snapshot: PannedSnapshot) -> Result<Self> {
        ensure!(
            snapshot.is_unscaled_loop(),
            "CD music transport is not an unscaled normal-speed loop"
        );
        // A Modern music session captured the muted bytes; the host supplies the installed ones.
        let source = if snapshot.source_identity() == blake3::hash(&source.bytes).to_hex().as_str()
        {
            source
        } else {
            super::muted(&source).unwrap_or(source)
        };
        Ok(Self::wrap(Panned::from_snapshot(
            source,
            PcmWidth::Sixteen,
            snapshot,
        )?))
    }
}
impl Decodable for MusicTransport {
    type Decoder = PannedDecoder;
    fn decoder(&self) -> Self::Decoder {
        self.transport.decoder()
    }
}

/// Opaque music checkpoint. It excludes downstream resampler/device buffers.
#[derive(Clone, Serialize, Deserialize)]
pub struct MusicSnapshot {
    version: u32,
    track: Option<u32>,
    gain: f32,
    playback: Option<TrackSnapshot>,
}
#[derive(Clone, Serialize, Deserialize)]
struct TrackSnapshot {
    transport: PannedSnapshot,
    paused: bool,
}
impl MusicSnapshot {
    /// Installed CD track whose original bytes the host must prepare.
    pub fn track(&self) -> Option<u32> {
        self.track
    }

    /// Expected identity of the prepared track bytes; absent for stopped music.
    pub fn source_identity(&self) -> Option<&str> {
        self.playback
            .as_ref()
            .map(|state| state.transport.source_identity())
    }
}

/// Off-live music replacement. The session owner commits these together with
/// other validated resources and allocates the optional transport asset then.
pub struct MusicCandidate {
    /// Retained track and drive gain, without an asynchronous loading task.
    pub music: Music,
    /// Optional source and initial sink settings. Constructing them starts nothing.
    pub playback: Option<(MusicTransport, PlaybackSettings)>,
}

impl MusicCandidate {
    /// Apply live host inhibition before publishing replacement output. A zero
    /// Music preference or active cinematic pauses the candidate without clearing
    /// its saved pause, changing its gain, or advancing its decoder cursor.
    pub fn enforce_live_pause(&mut self, world: &bevy::prelude::World) {
        let inhibited = world
            .get_resource::<cyrus_assets::sound::SoundSettings>()
            .is_some_and(|settings| settings.redbook_volume == 0)
            || world
                .iter_entities()
                .any(|entity| entity.contains::<crate::CinematicAudio>());
        if inhibited && let Some((_, settings)) = &mut self.playback {
            settings.paused = true;
        }
    }
}

fn valid_gain(gain: f32) -> bool {
    gain == 1.0 || (0..=127).any(|volume| super::redbook_gain(volume) == Some(gain))
}

impl Music {
    /// Capture at the host's scheduler boundary. Supply the complete playback
    /// census and each player's actual pause state, including queued starts.
    /// In-flight preparation and inconsistent ownership reject capture.
    pub fn snapshot(&self, players: &[(&MusicTransport, bool)]) -> Result<MusicSnapshot> {
        ensure!(
            self.load.is_none(),
            "CD music track preparation is in flight"
        );
        ensure!(valid_gain(self.gain), "CD music drive gain is invalid");
        let playback = match (self.track, players) {
            (None, []) => None,
            (Some(track), [(transport, paused)]) if track >= 2 => Some(TrackSnapshot {
                transport: transport.snapshot()?,
                paused: *paused,
            }),
            _ => anyhow::bail!(
                "CD music track {:?} has inconsistent playback census {}",
                self.track,
                players.len()
            ),
        };
        Ok(MusicSnapshot {
            version: 1,
            track: self.track,
            gain: self.gain,
            playback,
        })
    }

    /// Reconstruct from the host's installed track bytes. No request, I/O,
    /// sink start or live resource replacement occurs here. Browser output buffers
    /// are prepared silently and owned by the candidate until publication.
    pub fn from_snapshot(
        snapshot: MusicSnapshot,
        source: Option<AudioSource>,
    ) -> Result<MusicCandidate> {
        ensure!(
            snapshot.version == 1,
            "CD music snapshot version {} is unsupported",
            snapshot.version
        );
        ensure!(valid_gain(snapshot.gain), "CD music drive gain is invalid");
        let playback = match (snapshot.track, snapshot.playback, source) {
            (None, None, None) => None,
            (Some(track), Some(saved), Some(source)) if track >= 2 => {
                let transport = MusicTransport::from_snapshot(source, saved.transport)?;
                let mut settings = transport
                    .playback_settings()
                    .with_volume(Volume::Linear(snapshot.gain));
                settings.paused = saved.paused;
                transport.prepare_output(&settings)?;
                Some((transport, settings))
            }
            _ => anyhow::bail!("CD music snapshot track/source ownership differs"),
        };
        Ok(MusicCandidate {
            music: Music {
                track: snapshot.track,
                gain: snapshot.gain,
                world_selection: super::WorldSelection::Restored,
                load: None,
            },
            playback,
        })
    }
}

#[cfg(any(target_arch = "wasm32", test))]
pub(super) struct CdBuffer {
    pub left: Vec<f32>,
    pub right: Vec<f32>,
    pub rate: u32,
}

#[cfg(any(target_arch = "wasm32", test))]
pub(super) fn cd_buffer(source: &AudioSource, initial: &PannedSnapshot) -> Result<CdBuffer> {
    use anyhow::Context;
    let mut reader = hound::WavReader::new(std::io::Cursor::new(&*source.bytes))
        .context("browser CD buffer WAV metadata")?;
    let spec = reader.spec();
    ensure!(
        spec.channels == 2
            && spec.bits_per_sample == 16
            && spec.sample_format == hound::SampleFormat::Int,
        "browser CD buffer requires stereo16 PCM"
    );
    let samples = reader
        .samples::<i16>()
        .collect::<std::result::Result<Vec<_>, _>>()
        .context("browser CD buffer PCM")?;
    ensure!(
        !samples.is_empty() && samples.len() % 2 == 0,
        "browser CD buffer has incomplete frames"
    );
    let offset = initial.input_samples() as usize % samples.len();
    let mut left = Vec::with_capacity(samples.len() / 2);
    let mut right = Vec::with_capacity(samples.len() / 2);
    for frame in 0..samples.len() / 2 {
        left.push(f32::from(samples[(offset + frame * 2) % samples.len()]) / 32768.0);
        right.push(f32::from(samples[(offset + frame * 2 + 1) % samples.len()]) / 32768.0);
    }
    Ok(CdBuffer {
        left,
        right,
        rate: spec.sample_rate,
    })
}

#[cfg(test)]
mod buffer_tests {
    use super::*;
    use bevy::audio::Decodable;

    #[test]
    fn browser_buffer_and_render_cursor_continue_the_native_cd_transport() {
        use cyrus_assets::audio::pcm_wav;
        use rgpre::import::sfx::AudioType;
        let pcm: Vec<_> = [16384i16, -16384, 8192, -8192, 32767, -32768]
            .into_iter()
            .flat_map(i16::to_le_bytes)
            .collect();
        let source = AudioSource {
            bytes: pcm_wav(AudioType::Stereo16, 44100, &pcm).unwrap().into(),
        };
        for consumed in 0..18 {
            let track = MusicTransport::new(source.clone()).unwrap();
            let mut native = track.decoder();
            native.by_ref().take(consumed).for_each(drop);
            let initial = track.snapshot().unwrap();
            let buffer = cd_buffer(&source, &initial).unwrap();
            assert_eq!(buffer.rate, 44100);
            let interleaved: Vec<_> = buffer
                .left
                .iter()
                .zip(&buffer.right)
                .flat_map(|(left, right)| [*left, *right])
                .collect();
            assert_eq!(native.by_ref().take(6).collect::<Vec<_>>(), interleaved);
            let advanced = (initial.input_samples() + 4) % 6;
            let saved = initial.at_stereo_input_sample(advanced);
            let restored = MusicTransport::from_snapshot(source.clone(), saved).unwrap();
            assert_eq!(
                restored.decoder().take(8).collect::<Vec<_>>(),
                interleaved
                    .into_iter()
                    .cycle()
                    .skip(4)
                    .take(8)
                    .collect::<Vec<_>>()
            );
        }
    }
}

#[cfg(target_arch = "wasm32")]
impl Drop for MusicTransport {
    fn drop(&mut self) {
        let id = *self.prepared.get_mut();
        if id != 0 {
            super::browser::discard(id);
        }
    }
}
