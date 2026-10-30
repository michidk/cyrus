//! Sample-pool and source-decoder replacements for coordinated session assembly.
use super::{Channel, Channels, Panned, PannedSnapshot, PcmWidth, PlayingSound};
use crate::persistence::AudioRestoreBindings;
use anyhow::{Context, Result, ensure};
use bevy::{
    audio::{
        AudioPlayer, AudioSink, AudioSinkPlayback, AudioSource, PlaybackMode, PlaybackSettings,
    },
    prelude::{Assets, Commands, Entity, With, World},
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

/// Complete logical sample pool and its source-decoder continuations. No installed
/// bytes are serialized. Downstream conversion and device queues remain host-owned.
#[derive(Clone, Serialize, Deserialize)]
pub struct SamplePlaybackSnapshot {
    version: u32,
    ownership: super::samples::ChannelOwnershipSnapshot,
    players: Vec<SavedPlayback>,
}

#[derive(Clone, Serialize, Deserialize)]
struct SavedPlayback {
    token: u64,
    entity: u64,
    width: u16,
    transport: PannedSnapshot,
    paused: bool,
    muted: bool,
}

/// One installed source and presentation identity the host must prepare.
pub struct SamplePlaybackSource<'a> {
    pub saved_entity: u64,
    pub source_identity: &'a str,
    pub bits_per_sample: u16,
}

/// Validated replacement, with no sinks or assets allocated during construction.
/// Queue only as part of the aggregate's exclusive, fully validated commit.
pub struct SamplePlaybackCandidate {
    channels: Channels,
    playback: Vec<(Entity, Panned, PlaybackSettings)>,
}

impl SamplePlaybackSnapshot {
    /// Free the channels whose playback entity the backend already despawned (a finished
    /// effect). The sound systems reclaim at the start of the frame, so a sound that ends
    /// later in the frame would otherwise still hold a channel when a snapshot is captured.
    pub fn reclaim_finished(world: &mut World) {
        let alive: std::collections::HashSet<Entity> = world
            .query_filtered::<Entity, With<PlayingSound>>()
            .iter(world)
            .collect();
        if let Some(mut channels) = world.get_resource_mut::<Channels>() {
            channels.reclaim_where(|sound| alive.contains(&sound));
        }
    }

    /// Capture after channel reclamation and cue dispatch, with deferred commands
    /// applied. The host must stop simulation and establish its backend checkpoint
    /// policy before calling; source cursor capture does not freeze device output.
    pub fn capture(world: &World) -> Result<Self> {
        let query = world.try_query_filtered::<Entity, With<PlayingSound>>();
        let channels = world
            .get_resource::<Channels>()
            .context("sample snapshot: channels absent")?;
        let ownership = channels.snapshot_ownership()?;
        let assets = world
            .get_resource::<Assets<Panned>>()
            .context("sample snapshot: transport assets absent")?;
        let mut census = HashMap::new();
        for entity in query
            .into_iter()
            .flat_map(|mut query| query.iter(world).collect::<Vec<_>>())
        {
            let player = world.get::<AudioPlayer<Panned>>(entity);
            let settings = world.get::<PlaybackSettings>(entity);
            let sink = world.get::<AudioSink>(entity);
            let player = player.with_context(|| {
                format!("sample snapshot {entity:?}: source-decoder transport absent")
            })?;
            let transport = assets.get(&player.0).with_context(|| {
                format!("sample snapshot {entity:?}: transport asset unavailable")
            })?;
            let settings = settings
                .with_context(|| format!("sample snapshot {entity:?}: playback settings absent"))?;
            let expected = transport.playback_settings();
            ensure!(
                matches!(settings.mode, PlaybackMode::Despawn)
                    && !settings.spatial
                    && settings.spatial_scale.is_none()
                    && settings.start_position.is_none()
                    && settings.duration.is_none(),
                "sample snapshot {entity:?}: unsupported backend playback configuration"
            );
            let (paused, muted, volume, speed) = if let Some(sink) = sink {
                (
                    sink.is_paused(),
                    sink.is_muted(),
                    sink.volume(),
                    sink.speed(),
                )
            } else {
                (
                    settings.paused,
                    settings.muted,
                    settings.volume,
                    settings.speed,
                )
            };
            ensure!(
                volume.to_linear() == 1.0 && speed == expected.speed,
                "sample snapshot {entity:?}: backend gain/speed differs from source transport"
            );
            census.insert(entity, (transport, paused, muted));
        }
        ensure!(
            census.len() == channels.playing.len(),
            "sample snapshot: playback entity census differs: {} playback entities, {} channels, channels without a sample transport entity {:?}",
            census.len(),
            channels.playing.len(),
            channels
                .playing
                .iter()
                .filter(|(_, channel)| !census.contains_key(&channel.sound))
                .map(|(token, channel)| (*token, channel.sound))
                .collect::<Vec<_>>()
        );
        let mut players = Vec::with_capacity(census.len());
        for (&token, channel) in &channels.playing {
            let (transport, paused, muted) = census.remove(&channel.sound).with_context(|| {
                format!("sample snapshot token {token}: playback entity absent")
            })?;
            let gains = channel.gains.as_ref().with_context(|| {
                format!("sample snapshot token {token}: source gain owner absent")
            })?;
            ensure!(
                std::sync::Arc::ptr_eq(&gains.scale, &transport.gains().scale),
                "sample snapshot token {token}: channel and decoder gain owners differ"
            );
            players.push(SavedPlayback {
                token,
                entity: channel.sound.to_bits(),
                width: match gains.width {
                    PcmWidth::Eight => 8,
                    PcmWidth::Sixteen => 16,
                },
                transport: transport
                    .snapshot()
                    .with_context(|| format!("sample snapshot token {token}"))?,
                paused,
                muted,
            });
        }
        players.sort_by_key(|player| player.token);
        Ok(Self {
            version: 1,
            ownership,
            players,
        })
    }

    /// Untrusted preparation hints; restoration validates identities and the census.
    pub fn sources(&self) -> impl ExactSizeIterator<Item = SamplePlaybackSource<'_>> {
        self.players.iter().map(|player| SamplePlaybackSource {
            saved_entity: player.entity,
            source_identity: player.transport.source_identity(),
            bits_per_sample: player.width,
        })
    }

    /// Saved presentation owners needed by the binding plan, including remembered
    /// animation slots with no active playback. Playback entities are listed by sources.
    pub fn entity_owners(&self) -> std::collections::BTreeSet<u64> {
        self.ownership.entity_owners()
    }

    /// Prepare against installed source bytes and fresh host-reserved entities.
    /// Sample slot numbers remain unchanged; transport tokens are minted above both
    /// saved and live allocators so stale completion callbacks cannot retire them.
    pub fn restore(
        self,
        sources: &BTreeMap<String, AudioSource>,
        bindings: &AudioRestoreBindings,
        live_world: &World,
    ) -> Result<SamplePlaybackCandidate> {
        ensure!(
            self.version == 1,
            "sample playback snapshot version unsupported"
        );
        let live_last_token = live_world
            .get_resource::<Channels>()
            .map_or(0, |channels| channels.samples.last_token());
        let ownership = self.ownership.restore(live_last_token, bindings)?;
        ensure!(
            self.players.len() == ownership.tokens.len(),
            "saved sample transport census differs"
        );
        let mut channels = HashMap::new();
        let mut playback = Vec::with_capacity(self.players.len());
        for saved in self.players {
            let reservation = *ownership
                .tokens
                .get(&saved.token)
                .with_context(|| format!("saved sample token {}: slot absent", saved.token))?;
            let entity = bindings.entity(saved.entity)?;
            let source = sources
                .get(saved.transport.source_identity())
                .with_context(|| {
                    format!(
                        "saved sample token {}: installed source absent",
                        saved.token
                    )
                })?;
            let transport = Panned::from_snapshot(
                source.clone(),
                PcmWidth::try_from(saved.width).map_err(anyhow::Error::msg)?,
                saved.transport,
            )
            .with_context(|| format!("saved sample token {}", saved.token))?;
            let mut settings = transport.playback_settings();
            settings.paused = saved.paused;
            settings.muted = saved.muted;
            ensure!(
                channels
                    .insert(
                        reservation.token,
                        Channel {
                            sound: entity,
                            reservation,
                            gains: Some(transport.gains().clone()),
                        }
                    )
                    .is_none(),
                "saved sample token {} duplicated",
                saved.token
            );
            playback.push((entity, transport, settings));
        }
        let channels = ownership.finish(channels)?;
        Ok(SamplePlaybackCandidate { channels, playback })
    }
}

impl SamplePlaybackCandidate {
    /// Stop prepared samples for an original saved-world load. Remembered slot
    /// numbers and allocation high-water marks remain valid, but no decoder resumes.
    pub fn stop_all(&mut self) {
        self.channels.samples.stop_matching(None, None);
        self.channels.playing.clear();
        self.playback.clear();
    }

    /// Validate target existence and allocation freshness immediately before publication.
    pub fn validate_publication(&self, world: &World) -> anyhow::Result<()> {
        ensure!(
            world.contains_resource::<Assets<Panned>>(),
            "sample publication transport assets absent"
        );
        let live = world
            .get_resource::<Channels>()
            .context("sample publication live channels absent")?;
        ensure!(
            self.channels.samples.last_token() >= live.samples.last_token(),
            "sample publication allocation advanced after preparation"
        );
        for channel in self.channels.playing.values() {
            ensure!(
                channel.reservation.token > live.samples.last_token(),
                "sample publication token is no longer fresh"
            );
        }
        for (entity, _, _) in &self.playback {
            ensure!(
                world.get_entity(*entity).is_ok() && world.get::<PlayingSound>(*entity).is_none(),
                "sample publication target {entity:?} absent or already live"
            );
        }
        Ok(())
    }

    pub fn entities(&self) -> impl Iterator<Item = Entity> + '_ {
        self.playback.iter().map(|(entity, _, _)| *entity)
    }

    /// Queue validated replacements into the aggregate's command buffer. The host
    /// owns reserved entity existence and removal of the previous playback entities.
    /// Do not apply this buffer until all session owners have passed validation.
    pub fn queue(self, commands: &mut Commands, assets: &mut Assets<Panned>) {
        for (entity, transport, settings) in self.playback {
            let gains = transport.gains().clone();
            let handle = assets.add(transport);
            commands
                .entity(entity)
                .insert((PlayingSound, AudioPlayer(handle), settings, gains));
        }
        commands.insert_resource(self.channels);
    }
}
