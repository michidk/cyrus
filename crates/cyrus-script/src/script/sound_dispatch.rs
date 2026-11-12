//! Ordered animation sound admission while logical actors await presentation bindings.
use super::{ScriptActorId, ScriptSound};
use bevy::prelude::Entity;
use cyrus_world::{animation::AnimationSound, sound::SoundCue};

/// Script-side sound routing before a scene node exists. Stops apply before binding,
/// just as they do in the downstream audio owner. Playback and remembered handles
/// remain audio-owned; deferral does not turn a stopped start into later playback.
#[derive(Default)]
pub struct AnimationSoundQueue {
    pending: Vec<(ScriptActorId, ScriptSound)>,
}
impl AnimationSoundQueue {
    /// Number of commands awaiting scene bindings, including Stop/Forget controls.
    pub fn len(&self) -> usize {
        self.pending.len()
    }
    /// Whether no command awaits a scene binding.
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }
    /// Route one logical command. `node` must be the unique binding for `actor`
    /// in the current animation source, or None while that binding is unavailable.
    /// Stop matching uses the requested effect before surface remapping, following
    /// preservation's engine/sound.md. Unrelated owners and control order survive.
    pub fn send(
        &mut self,
        actor: ScriptActorId,
        sound: ScriptSound,
        node: Option<Entity>,
        emit: &mut dyn FnMut(SoundCue) -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        self.pending.retain(|(owner, pending)| {
            let ScriptSound::Animation {
                sound: AnimationSound::Play { effect, .. },
                ..
            } = pending
            else {
                return true;
            };
            match &sound {
                ScriptSound::StopOwned => *owner != actor,
                ScriptSound::StopAll => false,
                ScriptSound::StopEffect {
                    effect: stopped,
                    owned,
                } => i32::from(*effect) != *stopped || (*owned && *owner != actor),
                _ => true,
            }
        });
        let cue = match sound {
            ScriptSound::Animation {
                sound: AnimationSound::Event { .. } | AnimationSound::Transition { .. },
                ..
            } => anyhow::bail!(
                "animation actor event or transition outside an ordinary update is unsupported"
            ),
            ScriptSound::Animation {
                sound,
                surface,
                position,
            } => {
                let node = node.filter(|_| !self.pending.iter().any(|(held, _)| *held == actor));
                let Some(node) = node else {
                    anyhow::ensure!(
                        self.pending.len() < 64,
                        "animation sound scene binding unavailable: {} sounds already wait for a node",
                        self.pending.len()
                    );
                    self.pending.push((
                        actor,
                        ScriptSound::Animation {
                            sound,
                            surface,
                            position,
                        },
                    ));
                    return Ok(());
                };
                SoundCue::Animation {
                    actor: node,
                    sound,
                    surface,
                    position,
                }
            }
            sound => SoundCue::Script {
                generation: actor.generation,
                index: actor.index,
                sound,
            },
        };
        emit(cue)
    }
    /// Release current-generation commands in per-actor order. The caller resolves
    /// unique bindings against its current animation source. Expired worlds drop.
    pub fn release(
        &mut self,
        generation: u64,
        resolve: &mut dyn FnMut(ScriptActorId) -> anyhow::Result<Option<Entity>>,
        emit: &mut dyn FnMut(SoundCue) -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        let mut waiting = Vec::new();
        for (actor, sound) in std::mem::take(&mut self.pending) {
            if actor.generation != generation {
                continue;
            }
            if waiting.iter().any(|(held, _)| *held == actor) {
                waiting.push((actor, sound));
                continue;
            }
            match resolve(actor)? {
                Some(entity) => {
                    let ScriptSound::Animation {
                        sound,
                        surface,
                        position,
                    } = sound
                    else {
                        anyhow::bail!("only animation sounds are deferred");
                    };
                    emit(SoundCue::Animation {
                        actor: entity,
                        sound,
                        surface,
                        position,
                    })?;
                }
                None => waiting.push((actor, sound)),
            }
        }
        self.pending = waiting;
        Ok(())
    }
}
