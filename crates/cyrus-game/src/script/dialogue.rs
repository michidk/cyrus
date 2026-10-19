//! Shared dialogue records follow preservation's engine/sound.md initialization.
use anyhow::{Context, Result, ensure};
use cyrus_assets::script::DialogueSettings;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Complete ordered dialogue records for a Cyrus session, not an original SAV record.
/// Restore matching actor and audio owners before constructing a replacement.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DialogueSnapshot {
    version: u32,
    settings: DialogueSettings,
    records: Vec<SavedDialogueRecord>,
    skip_latched: bool,
    presented: Option<usize>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SavedDialogueRecord {
    owner: Option<(u64, usize)>,
    text: String,
    active: bool,
    sample: Option<u8>,
    remaining: i32,
}

/// Dialogue ownership persists across logical world replacement.
/// Speech creation and record updates require their own supported native handlers.
#[derive(Debug, Serialize)]
pub struct DialogueState {
    settings: DialogueSettings,
    records: Vec<DialogueRecord>,
    skip_latched: bool,
    presented: Option<usize>,
}

#[derive(Debug, Serialize)]
struct DialogueRecord {
    owner: Option<super::ScriptActorId>,
    text: String,
    active: bool,
    sample: Option<u8>,
    remaining: i32,
}

impl Default for DialogueRecord {
    fn default() -> Self {
        Self {
            owner: None,
            text: String::new(),
            active: false,
            sample: Some(0),
            remaining: 0,
        }
    }
}

/// Synchronous effects required by the dialogue record stage. Missing speaker,
/// audio or presentation ownership must return an error rather than skip work.
pub trait DialoguePlayback {
    fn sample_playing(&mut self, sample: u8) -> Result<bool>;
    fn stop_sample(&mut self, sample: u8) -> Result<()>;
    fn refresh_volume(&mut self, speaker: super::ScriptActorId, sample: u8) -> Result<()>;
    fn present_line(&mut self, index: usize, text: &str) -> Result<()>;
}

/// Game-owned speaker inhibition, alongside synchronous playback/presentation.
pub trait DialogueEffects: DialoguePlayback {
    fn inhibit_speaker(&mut self, speaker: super::ScriptActorId) -> Result<()>;
}

impl DialogueState {
    /// Capture without advancing lifetimes, consuming input or touching playback.
    pub fn snapshot(&self) -> DialogueSnapshot {
        DialogueSnapshot {
            version: 1,
            settings: self.settings.clone(),
            records: self
                .records
                .iter()
                .map(|record| SavedDialogueRecord {
                    owner: record.owner.map(|owner| (owner.generation, owner.index)),
                    text: record.text.clone(),
                    active: record.active,
                    sample: record.sample,
                    remaining: record.remaining,
                })
                .collect(),
            skip_latched: self.skip_latched,
            presented: self.presented,
        }
    }

    /// Validate and return a separate owner without audio, presentation or actor effects.
    ///
    /// Bind every retained actor and sample reference, including inactive records.
    /// Mappings must be injective; additional mappings from the session are allowed.
    /// The host owns destination actor validity and reconstruction of the shared
    /// sixteen-channel audio state. Commit only with that complete session candidate.
    /// A presented index may reference a cleared record until the next update.
    pub fn from_snapshot(
        settings: &DialogueSettings,
        snapshot: DialogueSnapshot,
        actors: &[(super::ScriptActorId, super::ScriptActorId)],
        samples: &BTreeMap<u8, u8>,
    ) -> Result<Self> {
        ensure!(
            snapshot.version == 1,
            "dialogue snapshot version {} is unsupported",
            snapshot.version
        );
        settings.validate()?;
        ensure!(
            snapshot.settings == *settings,
            "dialogue snapshot installed settings differ"
        );
        ensure!(
            snapshot.records.len() == settings.max_dialog_lines as usize,
            "dialogue snapshot record count {} differs from installed count {}",
            snapshot.records.len(),
            settings.max_dialog_lines
        );
        ensure!(
            snapshot
                .presented
                .is_none_or(|index| index < snapshot.records.len()),
            "dialogue snapshot presented index {:?} exceeds record count {}",
            snapshot.presented,
            snapshot.records.len()
        );
        let mut actor_bindings = BTreeMap::new();
        let mut destinations = BTreeSet::new();
        for &(saved, restored) in actors {
            ensure!(
                actor_bindings
                    .insert((saved.generation, saved.index), restored)
                    .is_none(),
                "dialogue actor binding repeats saved actor {saved:?}"
            );
            ensure!(
                destinations.insert((restored.generation, restored.index)),
                "dialogue actor binding repeats restored actor {restored:?}"
            );
        }
        let mut destinations = BTreeSet::new();
        for (&saved, &restored) in samples {
            ensure!(
                saved < 16 && restored < 16,
                "dialogue sample binding {saved}->{restored} exceeds sixteen channels"
            );
            ensure!(
                destinations.insert(restored),
                "dialogue sample binding repeats restored channel {restored}"
            );
        }
        let records = snapshot
            .records
            .into_iter()
            .enumerate()
            .map(|(index, saved)| {
                let prepare = || -> Result<DialogueRecord> {
                    ensure!(
                        !saved.active || saved.owner.is_some(),
                        "active speaker unavailable"
                    );
                    let owner = saved
                        .owner
                        .map(|id| {
                            actor_bindings
                                .get(&id)
                                .copied()
                                .with_context(|| format!("speaker {id:?} binding unavailable"))
                        })
                        .transpose()?;
                    let sample = saved
                        .sample
                        .map(|sample| {
                            ensure!(
                                sample < 16,
                                "saved sample {sample} exceeds sixteen channels"
                            );
                            samples
                                .get(&sample)
                                .copied()
                                .with_context(|| format!("sample {sample} binding unavailable"))
                        })
                        .transpose()?;
                    Ok(DialogueRecord {
                        owner,
                        text: saved.text,
                        active: saved.active,
                        sample,
                        remaining: saved.remaining,
                    })
                };
                prepare().with_context(|| format!("dialogue snapshot record {index}"))
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            settings: snapshot.settings,
            records,
            skip_latched: snapshot.skip_latched,
            presented: snapshot.presented,
        })
    }

    pub(super) fn new(settings: &DialogueSettings) -> Result<Self> {
        settings.validate()?;
        Ok(Self {
            settings: settings.clone(),
            skip_latched: false,
            presented: None,
            records: (0..settings.max_dialog_lines)
                .map(|_| DialogueRecord::default())
                .collect(),
        })
    }

    /// Admit an already formatted line, starting at record one. Exhaustion does
    /// not replace an active line. Audio allocation is owned by the RTX service.
    pub fn begin_line(
        &mut self,
        owner: super::ScriptActorId,
        text: String,
        sample: Option<u8>,
        remaining: i32,
    ) -> Option<usize> {
        let (index, record) = self
            .records
            .iter_mut()
            .enumerate()
            .skip(1)
            .find(|(_, record)| !record.active)?;
        record.owner = Some(owner);
        record.text = text;
        record.sample = sample;
        record.remaining = remaining;
        record.active = true;
        Some(index)
    }

    /// Clear activity, text and the remembered sample without stopping playback
    /// or forgetting the retained speaker. Absent records have no effect.
    pub fn clear_line(&mut self, index: Option<usize>) {
        if let Some(record) = index.and_then(|index| self.records.get_mut(index)) {
            record.active = false;
            record.text.clear();
            record.sample = None;
        }
    }

    /// RTXp replaces the reserved record, whose speaker is the selected player.
    /// The host must supply that identity rather than the script caller.
    pub fn begin_player_line(
        &mut self,
        player: super::ScriptActorId,
        text: String,
        sample: Option<u8>,
        remaining: i32,
    ) {
        if self.presented == Some(0) {
            self.presented = None;
        }
        self.records[0] = DialogueRecord {
            owner: Some(player),
            text,
            active: true,
            sample,
            remaining,
        };
    }

    /// Clear active ordinary records matching an RTX cache entry's remembered
    /// sample. The reserved player record is excluded, even on a matching slot.
    pub fn clear_sample_lines(&mut self, sample: Option<u8>) {
        for record in self.records.iter_mut().skip(1) {
            if record.active && record.sample == sample {
                record.active = false;
                record.text.clear();
                record.sample = None;
            }
        }
    }

    /// Sound maintenance clears every active record, including the reserved
    /// player line, before stopping sample channels. It does not sample input
    /// or change the skip latch. Inactive records retain their stored state.
    pub fn reset_active_lines(&mut self) {
        for record in &mut self.records {
            if record.active {
                record.active = false;
                record.text.clear();
                record.sample = None;
            }
        }
    }

    /// Line selected by the most recent record update, with its original slot.
    /// Rendering must not advance lifetime or choose a different active record.
    pub fn presented_line(&self) -> Option<(usize, &str)> {
        let index = self.presented?;
        let record = self.records.get(index)?;
        record.active.then_some((index, record.text.as_str()))
    }

    /// Speakers retained by active records, in record traversal order.
    pub(super) fn active_speakers(&self) -> impl Iterator<Item = super::ScriptActorId> + '_ {
        self.records
            .iter()
            .filter(|record| record.active)
            .filter_map(|record| record.owner)
    }

    /// Missing record handles are inactive.
    pub fn line_active(&self, index: Option<usize>) -> bool {
        index
            .and_then(|index| self.records.get(index))
            .is_some_and(|record| record.active)
    }

    /// Apply the sampled dialogue skip signal, then update records. The host
    /// supplies the original signal and retained player lockout, not book-menu input.
    /// Unknown lockout is rejected only when a new skip edge needs its value.
    pub fn advance(
        &mut self,
        nominal_ticks: u32,
        speech_enabled: bool,
        skip_signal: bool,
        player_lockout: Option<u16>,
        effects: &mut dyn DialogueEffects,
    ) -> Result<()> {
        self.presented = None;
        if !skip_signal {
            self.skip_latched = false;
        } else if !self.skip_latched {
            let player_lockout =
                player_lockout.context("dialogue player lockout unavailable for skip edge")?;
            self.skip_latched = true;
            if player_lockout == 1 {
                for index in 0..self.records.len() {
                    if let Some(sample) = self.records[index].sample
                        && effects.sample_playing(sample)?
                    {
                        effects.stop_sample(sample)?;
                    }
                    self.clear_line(Some(index));
                }
                return Ok(());
            }
        }
        self.advance_records(nominal_ticks, speech_enabled, effects)
    }

    /// Advance the record stage after the host's dialogue-skip input stage.
    /// Lifetimes use signed wrapping counters and lose 21 units per nominal tick.
    /// The first presented or finished voiced line ends this stage; expired
    /// unvoiced lines can clear before a later record is visited.
    pub fn advance_records(
        &mut self,
        nominal_ticks: u32,
        speech_enabled: bool,
        effects: &mut dyn DialogueEffects,
    ) -> Result<()> {
        self.presented = None;
        for index in 0..self.records.len() {
            let record = &mut self.records[index];
            if !record.active {
                continue;
            }
            if let Some(speaker) = record.owner {
                effects.inhibit_speaker(speaker)?;
            }
            record.remaining = record
                .remaining
                .wrapping_sub((nominal_ticks as i32).wrapping_mul(21));
            if speech_enabled {
                if let Some(sample) = record.sample {
                    if effects.sample_playing(sample)? {
                        let speaker = record.owner.ok_or_else(|| {
                            anyhow::anyhow!(
                                "dialogue record {index}: speaker unavailable for volume update"
                            )
                        })?;
                        effects.refresh_volume(speaker, sample)?;
                        effects.present_line(index, &record.text)?;
                        self.presented = Some(index);
                    } else {
                        self.clear_line(Some(index));
                    }
                    return Ok(());
                }
            } else if record.remaining > 0 {
                effects.present_line(index, &record.text)?;
                self.presented = Some(index);
                return Ok(());
            } else {
                self.clear_line(Some(index));
            }
        }
        Ok(())
    }

    /// Installed settings used to initialize this game's dialogue records.
    pub fn settings(&self) -> &DialogueSettings {
        &self.settings
    }

    /// WaitOnDialog scans all records, independent of the calling actor.
    /// Only the active word equal to one keeps the task pending.
    pub fn waiting(&self) -> bool {
        self.records.iter().any(|record| record.active)
    }
}

/// One source-prepared line, formatted with the installed dialogue font/width.
/// Sample handles refer to the shared sixteen-channel allocator, not entities.
#[derive(Debug)]
pub struct DialogueLine {
    pub text: String,
    pub sample: Option<u8>,
    pub lifetime: i32,
}

/// RTX preparation, optional voice start and original text formatting.
/// Pending retains the task while host assets load, without starting playback.
/// Ready(None) denotes failed lookup; errors denote invalid data or missing services.
/// This service does not mutate game dialogue records.
pub trait DialogueSource: std::fmt::Debug {
    fn player_line(
        &self,
        label: [u8; 4],
        player: super::ScriptActorId,
        text_width: u32,
    ) -> Result<std::task::Poll<Option<DialogueLine>>>;
    /// `RTX` line for the scripted actor. Preparation never depends on the speaker, so the
    /// default is the player line; the speaker only owns the record the caller creates.
    fn speaker_line(
        &self,
        label: [u8; 4],
        speaker: super::ScriptActorId,
        text_width: u32,
    ) -> Result<std::task::Poll<Option<DialogueLine>>> {
        self.player_line(label, speaker, text_width)
    }
    /// Stop the sample channel behind a dialogue record's remembered sample slot.
    fn stop_sample(&self, sample: u8) -> Result<()>;
    /// One `AmbientRtx` run for a started clip: compute the positional volume (with the task's
    /// volume offset) and pan from the receiver's signed Q8 `position`; a zero volume or a killed
    /// receiver stops the clip. Returns whether the clip is still playing and the task keeps running.
    fn update_ambient_rtx(
        &self,
        sample: u8,
        position: [i32; 3],
        volume_offset: i32,
        killed: bool,
    ) -> Result<bool>;
}
