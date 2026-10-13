//! Scoped script feedback and queued native/browser cue consumption share Channels.
use super::*;
use bevy::ecs::system::SystemParam;
use std::cell::RefCell;

/// Plays [`SoundCue`]s from game code through the same channels as control requests.
#[derive(SystemParam)]
struct SoundFrame<'w, 's> {
    commands: Commands<'w, 's>,
    effects: Res<'w, SoundEffects>,
    settings: Option<Res<'w, SoundSettings>>,
    listener: Option<Res<'w, crate::AudioListener>>,
    surfaces: Option<Res<'w, SurfaceSounds>>,
    actors: Query<
        'w,
        's,
        (
            &'static GlobalTransform,
            Option<&'static SoundSurface>,
            Has<cyrus_game::player::Player>,
        ),
    >,
    bindings: Query<'w, 's, &'static cyrus_game::script::ScriptBinding>,
    alive: Query<'w, 's, (), With<PlayingSound>>,
    sources: Option<Res<'w, Assets<AudioSource>>>,
    panned: Option<ResMut<'w, Assets<Panned>>>,
    channels: ResMut<'w, Channels>,
    #[cfg(target_arch = "wasm32")]
    pending: ResMut<'w, PendingSounds>,
    #[cfg(target_arch = "wasm32")]
    bank_loading: Option<Res<'w, crate::sound_bank::SoundBankHandle>>,
}

#[cfg(target_arch = "wasm32")]
#[derive(Resource, Default)]
pub(super) struct PendingSounds(Vec<PendingCue>);

impl SoundFrame<'_, '_> {
    fn prepare(&mut self, ownership: Option<Option<u64>>) {
        let Self {
            commands,
            #[cfg(target_arch = "wasm32")]
            settings,
            actors,
            #[cfg(target_arch = "wasm32")]
            effects,
            #[cfg(target_arch = "wasm32")]
            listener,
            alive,
            #[cfg(target_arch = "wasm32")]
            sources,
            #[cfg(target_arch = "wasm32")]
            panned,
            channels,
            #[cfg(target_arch = "wasm32")]
            pending,
            #[cfg(target_arch = "wasm32")]
            bank_loading,
            ..
        } = self;
        channels.reclaim(alive);
        #[cfg(target_arch = "wasm32")]
        let mut mixer = Mixer {
            master: settings.as_ref().map_or(256, |s| s.master_volume),
            sources: sources.as_deref(),
            panned: panned.as_deref_mut(),
        };
        let current_owner = |owner: Owner| match owner {
            Owner::Entity(_) => true,
            Owner::Script { generation, .. } => {
                ownership.is_none_or(|current| current == Some(generation))
            }
        };
        let expired = channels.samples.stop_expired(current_owner);
        channels.retire(commands, expired);
        #[cfg(target_arch = "wasm32")]
        pending
            .0
            .retain(|cue| cue.owner().is_none_or(current_owner));
        // Preserve authored cue order while the browser bank is loading.
        #[cfg(target_arch = "wasm32")]
        if bank_loading.is_none() {
            for cue in pending.0.drain(..) {
                match cue {
                    PendingCue::Flat(owner, flat) => {
                        channels.start_flat(commands, &mut mixer, effects, flat, owner)
                    }
                    PendingCue::Positional(owner, cue) => {
                        if let (Some(settings), Some(listener)) =
                            (settings.as_deref(), listener.as_deref())
                        {
                            channels.start_positional(
                                commands,
                                &mut mixer,
                                effects,
                                cue,
                                owner,
                                (settings, listener),
                            );
                        } else {
                            warn!(
                                "queued script Sound effect {}: listener/settings unavailable",
                                cue.effect
                            );
                        }
                    }
                }
            }
        }
        // A despawned actor's remembered sound is forgotten, not stopped.
        channels
            .animation
            .retain(|actor, _| actors.contains(*actor));
    }

    fn emit(&mut self, cue: &SoundCue, ownership: Option<Option<u64>>) {
        let Self {
            commands,
            effects,
            settings,
            listener,
            surfaces,
            actors,
            bindings,
            sources,
            panned,
            channels,
            #[cfg(target_arch = "wasm32")]
            pending,
            #[cfg(target_arch = "wasm32")]
            bank_loading,
            ..
        } = self;
        let mut mixer = Mixer {
            master: settings.as_ref().map_or(256, |s| s.master_volume),
            sources: sources.as_deref(),
            panned: panned.as_deref_mut(),
        };
        let current_owner = |owner: Owner| match owner {
            Owner::Entity(_) => true,
            Owner::Script { generation, .. } => {
                ownership.is_none_or(|current| current == Some(generation))
            }
        };
        for cue in std::iter::once(cue) {
            match cue {
                SoundCue::Script {
                    generation,
                    index,
                    sound,
                } => {
                    let owner = Owner::Script {
                        generation: *generation,
                        index: *index,
                    };
                    if !current_owner(owner) {
                        continue;
                    }
                    match sound {
                        ScriptSound::Positional {
                            effect,
                            volume,
                            rate_offset_hz,
                            position,
                        } => {
                            let cue = PositionalCue {
                                effect: *effect,
                                volume: *volume,
                                rate_offset_hz: *rate_offset_hz,
                                position: *position,
                            };
                            #[cfg(target_arch = "wasm32")]
                            if bank_loading.is_some() {
                                pending.0.push(PendingCue::Positional(owner, cue));
                                continue;
                            }
                            if let (Some(settings), Some(listener)) =
                                (settings.as_deref(), listener.as_deref())
                            {
                                channels.start_positional(
                                    commands,
                                    &mut mixer,
                                    effects,
                                    cue,
                                    owner,
                                    (settings, listener),
                                );
                            } else {
                                warn!(
                                    "script Sound effect {effect}: listener/settings unavailable"
                                );
                            }
                        }
                        ScriptSound::Flat {
                            effect,
                            volume,
                            rate_offset_hz,
                        } => {
                            if let Ok(effect) = usize::try_from(*effect) {
                                #[cfg(target_arch = "wasm32")]
                                if bank_loading.is_some() {
                                    pending.0.push(PendingCue::Flat(
                                        Some(owner),
                                        (effect, *volume, *rate_offset_hz),
                                    ));
                                    continue;
                                }
                                channels.start_flat(
                                    commands,
                                    &mut mixer,
                                    effects,
                                    (effect, *volume, *rate_offset_hz),
                                    Some(owner),
                                );
                            }
                        }
                        ScriptSound::StopEffect { effect, owned } => {
                            #[cfg(target_arch = "wasm32")]
                            pending.0.retain(|cue| {
                                cue.effect() != Some(*effect)
                                    || (*owned && cue.owner() != Some(owner))
                            });
                            channels.stop_matching(commands, owned.then_some(owner), Some(*effect));
                        }
                        ScriptSound::StopOwned => {
                            #[cfg(target_arch = "wasm32")]
                            pending.0.retain(|cue| cue.owner() != Some(owner));
                            channels.stop_matching(commands, Some(owner), None);
                        }
                        ScriptSound::Animation { .. } => {
                            warn!("script animation sound requires a scene binding")
                        }
                    }
                }
                SoundCue::Flat {
                    effect,
                    volume,
                    rate_offset_hz,
                } => {
                    // Browser cues wait for the exported bank instead of being lost.
                    #[cfg(target_arch = "wasm32")]
                    if bank_loading.is_some() {
                        pending
                            .0
                            .push(PendingCue::Flat(None, (*effect, *volume, *rate_offset_hz)));
                        continue;
                    }
                    channels.start_flat(
                        commands,
                        &mut mixer,
                        effects,
                        (*effect, *volume, *rate_offset_hz),
                        None,
                    );
                }
                SoundCue::Animation {
                    actor,
                    sound,
                    surface,
                    position,
                } => match sound {
                    AnimationSound::Play {
                        effect: requested,
                        rate_offset_hz,
                    } => {
                        channels.animation.remove(actor);
                        let Ok((transform, _, is_player)) = actors.get(*actor) else {
                            warn!("sound: animation cue for {actor} without a transform");
                            continue;
                        };
                        let index = match surface {
                            Some(0) => u8::try_from(*requested).ok().map(usize::from),
                            Some(surface) => {
                                let Some(surfaces) = surfaces.as_deref() else {
                                    warn_once!("sound: SURFACE.INI remaps are unavailable");
                                    continue;
                                };
                                surfaces.animation_sound(*surface, *requested)
                            }
                            None => {
                                warn_once!(
                                    "sound: animation sound for an actor without a known surface is unsupported"
                                );
                                continue;
                            }
                        };
                        let Some((index, effect)) =
                            index.and_then(|i| Some((i, effects.0.get(i)?.as_ref()?)))
                        else {
                            continue;
                        };
                        let (Some(settings), Some(listener)) =
                            (settings.as_deref(), listener.as_deref())
                        else {
                            continue;
                        };
                        let Some(distance) = settings.sound_distance else {
                            continue;
                        };
                        let mixed = match position {
                            Some(position) => positional_q8(*position, settings, listener, 0),
                            None => positional(transform.translation(), &listener.0, distance, 0),
                        };
                        let (volume, pan) = match mixed {
                            Ok(mix) => mix,
                            Err(e) => {
                                warn!("sound: effect {index}: {e}");
                                continue;
                            }
                        };
                        let started = channels.start(
                            commands,
                            &mut mixer,
                            Start {
                                owner: Some(bindings.get(*actor).ok().map_or(
                                    Owner::Entity(*actor),
                                    |binding| Owner::Script {
                                        generation: binding.actor.generation,
                                        index: binding.actor.index,
                                    },
                                )),
                                effect: Some(i32::from(*requested)),
                                audio: effect.audio.clone(),
                                looping: effect.looping,
                                volume,
                                pan: Some(pan),
                                speed: playback_speed(effect.sample_rate, *rate_offset_hz),
                                start_index: if is_player { 0 } else { 6 },
                            },
                        );
                        channels.start = 6;
                        if let Some(sound) = started {
                            channels.animation.insert(*actor, sound);
                        }
                    }
                    AnimationSound::Stop => {
                        if let Some(slot) = channels.animation.remove(actor) {
                            channels.stop_slot(commands, slot);
                        }
                    }
                    AnimationSound::Forget => {
                        channels.animation.remove(actor);
                    }
                },
            }
        }
    }
}

fn play_cues(
    mut frame: SoundFrame,
    mut cues: MessageReader<SoundCue>,
    script_actors: Option<Res<cyrus_game::script::ScriptActors>>,
) {
    let ownership = script_actors
        .as_ref()
        .map(|actors| actors.current.as_ref().map(|world| world.generation()));
    frame.prepare(ownership);
    for cue in cues.read() {
        frame.emit(cue, ownership);
    }
}

// Cached systems share their message cursors between the pre-ordinary drain and
// the post-animation drain. A second reader would replay earlier commands.
pub(super) fn play_audio(world: &mut World) {
    if let Err(error) = world.run_system_cached(play_requests) {
        error!("sound control consumer: {error}");
    }
    if let Err(error) = world.run_system_cached(play_cues) {
        error!("sound cue consumer: {error}");
    }
}

pub(super) fn execute_ordinary(world: &mut World) {
    play_audio(world);
    if let Err(error) = world.run_system_cached(scoped_ordinary) {
        world
            .resource_mut::<cyrus_game::script::ScriptActors>()
            .report_runtime_error(format!("ordinary audio host: {error}"));
    }
}

struct ScopedSounds<'a, 'w, 's> {
    frame: RefCell<&'a mut SoundFrame<'w, 's>>,
    generation: Option<u64>,
}

impl std::fmt::Debug for ScopedSounds<'_, '_, '_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ScopedSounds")
            .field("generation", &self.generation)
            .finish_non_exhaustive()
    }
}

impl cyrus_world::sound::AmbientSoundDriver for ScopedSounds<'_, '_, '_> {
    fn update(
        &self,
        request: cyrus_world::sound::AmbientSoundRequest,
    ) -> Result<cyrus_world::sound::AmbientSoundUpdate, String> {
        if self.generation != Some(request.generation) {
            return Err(format!(
                "ambient actor {}: expired world {}",
                request.actor_index, request.generation
            ));
        }
        if request.channel.is_some_and(|channel| channel >= 16) {
            return Err(format!(
                "ambient actor {}: invalid sample slot {:?}",
                request.actor_index, request.channel
            ));
        }
        let mut frame = self.frame.borrow_mut();
        let SoundFrame {
            commands,
            effects,
            settings,
            listener,
            sources,
            panned,
            channels,
            ..
        } = &mut **frame;
        let settings = settings
            .as_deref()
            .ok_or("AmbientSound: SYSTEM.INI settings unavailable")?;
        let listener = listener
            .as_deref()
            .ok_or("AmbientSound: listener unavailable")?;
        let (volume, pan) = positional_q8(request.position, settings, listener, request.volume)?;
        let mut mixer = Mixer {
            master: settings.master_volume,
            sources: sources.as_deref(),
            panned: panned.as_deref_mut(),
        };
        channels.ambient(commands, &mut mixer, effects, request, volume, pan)
    }
}

/// Borrowed native/browser sample service. All starts and stops use the same
/// sixteen live Channels as ordinary cues and control audio. This adapter is
/// synchronous; deferred backend entity creation does not determine slot status.
#[derive(SystemParam)]
pub struct ScriptSounds<'w, 's> {
    frame: SoundFrame<'w, 's>,
}

impl ScriptSounds<'_, '_> {
    /// Lend feedback and immediate sound dispatch for one logical-world update.
    /// The caller owns timing/reset inputs and must drain earlier queued cues
    /// before invoking this service. Neither reference may escape the callback.
    pub fn with_driver<R>(
        &mut self,
        generation: Option<u64>,
        update: impl FnOnce(
            &dyn cyrus_world::sound::AmbientSoundDriver,
            &mut dyn FnMut(SoundCue) -> anyhow::Result<()>,
        ) -> R,
    ) -> R {
        let sounds = ScopedSounds {
            generation,
            frame: RefCell::new(&mut self.frame),
        };
        update(&sounds, &mut |cue| {
            sounds.frame.borrow_mut().emit(&cue, Some(generation));
            Ok(())
        })
    }
}

fn scoped_ordinary(context: cyrus_game::script::OrdinaryContext, mut sounds: ScriptSounds) {
    let generation = context.generation();
    // Reset ownership remains with game maintenance; absent ownership is not
    // replaced by an assumed false flag.
    sounds.with_driver(generation, |driver, emit| {
        cyrus_game::script::run_ordinary(context, Some(driver), None, emit);
    });
}
