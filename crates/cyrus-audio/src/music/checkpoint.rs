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
#[derive(Asset, TypePath, Clone)]
pub struct MusicTransport(Panned);
impl MusicTransport {
    /// Validate a host-prepared stereo16 CD WAV without starting playback.
    pub fn new(source: AudioSource) -> Result<Self> {
        Ok(Self(Panned::with_playback(
            source,
            PannedGains::new(PcmWidth::Sixteen, 127, 256, 64),
            true,
            1.0,
        )?))
    }

    /// Immutable prepared bytes for the owning track, never serialized.
    pub fn source(&self) -> &AudioSource {
        self.0.source()
    }

    /// The decoder owns looping; apply linear drive gain and pause to these settings.
    pub fn playback_settings(&self) -> PlaybackSettings {
        self.0.playback_settings()
    }

    fn snapshot(&self) -> Result<PannedSnapshot> {
        self.0.snapshot()
    }

    fn from_snapshot(source: AudioSource, snapshot: PannedSnapshot) -> Result<Self> {
        ensure!(
            snapshot.is_unscaled_loop(),
            "CD music transport is not an unscaled normal-speed loop"
        );
        Ok(Self(Panned::from_snapshot(
            source,
            PcmWidth::Sixteen,
            snapshot,
        )?))
    }
}
impl Decodable for MusicTransport {
    type Decoder = PannedDecoder;
    fn decoder(&self) -> Self::Decoder {
        self.0.decoder()
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

    /// Reconstruct from the host's installed track bytes. No request, I/O, asset
    /// allocation, sink start or live resource replacement occurs here.
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
                Some((transport, settings))
            }
            _ => anyhow::bail!("CD music snapshot track/source ownership differs"),
        };
        Ok(MusicCandidate {
            music: Music {
                track: snapshot.track,
                gain: snapshot.gain,
                load: None,
            },
            playback,
        })
    }
}
