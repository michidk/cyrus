//! Dialogue and combat-taunt presentation share authoritative logical actors.
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
    /// Present an already selected taunt using the authoritative actor and
    /// shared dialogue records. This does not admit combat or consume RNG.
    /// The caller establishes the original animation switch/model-kind gate;
    /// `nominal_ticks` is reused if a request immediately advances animation.
    pub fn present_combat_taunt(
        &mut self,
        id: ScriptActorId,
        taunt: &cyrus_assets::script::CombatTaunt,
        playback: &mut dyn crate::script::CombatTauntPlayback,
        nominal_ticks: u32,
        animation_admitted: bool,
        sounds: &mut dyn FnMut(ScriptActorId, crate::script::ScriptSound) -> Result<()>,
    ) -> Result<i32> {
        let index = self.resolve(id)?;
        let position = super::animation_sound_position(self.spatial.as_ref(), index);
        present_taunt(
            &mut self.actors[index],
            &mut self.globals,
            self.world,
            id,
            position,
            taunt,
            playback,
            nominal_ticks,
            animation_admitted,
            sounds,
        )
    }

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

#[allow(clippy::too_many_arguments)] // Shared receiver state and synchronous presentation dependencies.
pub(super) fn present_taunt(
    actor: &mut ScriptActor,
    globals: &mut crate::script::GameplayGlobals,
    world: u32,
    id: ScriptActorId,
    position: Option<[i32; 3]>,
    taunt: &cyrus_assets::script::CombatTaunt,
    playback: &mut dyn crate::script::CombatTauntPlayback,
    nominal_ticks: u32,
    animation_admitted: bool,
    sounds: &mut dyn FnMut(ScriptActorId, crate::script::ScriptSound) -> Result<()>,
) -> Result<i32> {
    let index = id.index;
    let result = (|| {
        let combat = actor
            .control
            .as_mut()
            .and_then(|control| control.combat.as_mut())
            .with_context(|| {
                format!(
                    "world {} instance {index}: combat taunt state unavailable",
                    world
                )
            })?;
        let dialogue = globals
            .dialogue
            .as_mut()
            .context("combat taunt dialogue records unavailable")?;
        let settings = dialogue.settings().clone();
        if taunt.label != [0; 4] {
            let group = if animation_admitted && taunt.animation != 0 {
                let group = u16::try_from(taunt.animation)
                    .context("combat taunt animation outside unsigned range")?;
                anyhow::ensure!(
                    group <= i16::MAX as u16,
                    "combat taunt signed animation group unsupported"
                );
                actor
                    .animation
                    .as_ref()
                    .context("combat taunt animation not prepared")?;
                Some(group)
            } else {
                None
            };
            let text_width = settings.print_text.then_some(settings.max_text_width);
            playback.prepare(taunt.label, settings.use_speech, text_width)?;
            playback.release(&mut combat.taunt_cache)?;
            if settings.print_text && dialogue.line_active(combat.taunt_dialogue) {
                dialogue.clear_line(combat.taunt_dialogue);
            }
            let cache = playback.load(taunt.label)?;
            combat.taunt_cache = Some(cache);
            let line = playback.present(cache, settings.use_speech, text_width)?;
            if settings.print_text {
                anyhow::ensure!(
                    line.sample.is_none_or(|sample| sample < 16),
                    "combat taunt source returned invalid sample slot"
                );
                combat.taunt_dialogue =
                    dialogue.begin_line(id, line.text, line.sample, line.lifetime);
            }
            if let Some(group) = group {
                let surface = actor.surface;
                let animation = actor.animation.as_mut().expect("prepared taunt animation");
                let frame = animation.playback.request_with_sounds(
                    group,
                    0,
                    nominal_ticks,
                    &mut |sound| {
                        sounds(
                            id,
                            crate::script::ScriptSound::Animation {
                                sound,
                                surface,
                                position,
                            },
                        )
                    },
                )?;
                animation.frame = frame.or(animation.frame);
            }
        }
        if !settings.print_text {
            return Ok(0);
        }
        combat.taunt_active = i32::from(dialogue.line_active(combat.taunt_dialogue));
        Ok(combat.taunt_active)
    })();
    result.with_context(|| {
        format!(
            "world {} instance {} combat taunt presentation",
            world, id.index
        )
    })
}
