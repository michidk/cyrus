//! Dialogue updates write inhibition through the authoritative logical actors.
use super::{ScriptActor, ScriptActorId, ScriptWorld};
use crate::script::{DialogueEffects, DialoguePlayback};
use anyhow::{Context, Result};

struct Effects<'a> {
    generation: u64,
    actors: &'a mut [ScriptActor],
    playback: &'a mut dyn DialoguePlayback,
}

impl DialoguePlayback for Effects<'_> {
    fn sample_playing(&mut self, sample: u8) -> Result<bool> {
        self.playback.sample_playing(sample)
    }
    fn stop_sample(&mut self, sample: u8) -> Result<()> {
        self.playback.stop_sample(sample)
    }
    fn refresh_volume(&mut self, speaker: ScriptActorId, sample: u8) -> Result<()> {
        self.playback.refresh_volume(speaker, sample)
    }
    fn present_line(&mut self, index: usize, text: &str) -> Result<()> {
        self.playback.present_line(index, text)
    }
}

impl DialogueEffects for Effects<'_> {
    fn inhibit_speaker(&mut self, speaker: ScriptActorId) -> Result<()> {
        anyhow::ensure!(
            speaker.generation == self.generation,
            "dialogue speaker {} belongs to expired world generation {}",
            speaker.index,
            speaker.generation
        );
        let actor = self
            .actors
            .get_mut(speaker.index)
            .with_context(|| format!("dialogue speaker {} absent", speaker.index))?;
        actor.attributes[117] = 1;
        Ok(())
    }
}

impl ScriptWorld {
    /// Snapshot active speakers for the synchronous audio adapter. Positions are
    /// local signed Q8, matching the dialogue volume rule, after object updates.
    /// Unavailable poses stay absent and fail only if playback requests a volume
    /// refresh. Skip cleanup does not require a speaker pose.
    pub fn dialogue_speaker_positions(&self) -> Result<Vec<(ScriptActorId, [i32; 3])>> {
        let dialogue = self
            .globals
            .dialogue
            .as_ref()
            .context("dialogue records unavailable")?;
        Ok(dialogue
            .active_speakers()
            .filter_map(|speaker| {
                self.spatial(speaker)
                    .ok()
                    .map(|pose| (speaker, pose.local.position))
            })
            .collect())
    }

    /// Update dialogue with this world's retained lockout and logical speakers.
    /// `skip_signal` is the original configured key_a input, independent of Modern
    /// movement interpretation. Playback and presentation must be synchronous.
    pub fn advance_dialogue(
        &mut self,
        nominal_ticks: u32,
        speech_enabled: bool,
        skip_signal: bool,
        playback: &mut dyn DialoguePlayback,
    ) -> Result<()> {
        let lockout = self.globals.lockout();
        let dialogue = self
            .globals
            .dialogue
            .as_mut()
            .context("dialogue records unavailable")?;
        let mut effects = Effects {
            generation: self.generation,
            actors: &mut self.actors,
            playback,
        };
        dialogue
            .advance(
                nominal_ticks,
                speech_enabled,
                skip_signal,
                lockout,
                &mut effects,
            )
            .with_context(|| format!("world {} dialogue update", self.world))
    }
}
