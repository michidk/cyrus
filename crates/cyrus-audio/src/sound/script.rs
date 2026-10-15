//! Scoped script feedback and queued native/browser cue consumption share Channels.
use super::*;
use bevy::ecs::system::SystemParam;
use std::{cell::RefCell, task::Poll};

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
    bindings: Query<'w, 's, (Entity, &'static cyrus_game::script::ScriptBinding)>,
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
                            warn_once!(
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
        channels
            .animation_checkpoint
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
                        ScriptSound::AnimationState(state) => {
                            let mut nodes = bindings.iter().filter(|(_, binding)| {
                                binding.actor.generation == *generation
                                    && binding.actor.index == *index
                            });
                            let Some((actor, _)) = nodes.next() else {
                                warn_once!("script animation snapshot requires a scene binding");
                                continue;
                            };
                            if nodes.next().is_some() {
                                warn_once!(
                                    "script animation snapshot has ambiguous scene bindings"
                                );
                                continue;
                            }
                            match state {
                                cyrus_world::sound::AnimationSoundState::Capture => {
                                    let slot = channels.animation.get(&actor).copied();
                                    channels.animation_checkpoint.insert(actor, slot);
                                }
                                cyrus_world::sound::AnimationSoundState::Restore => {
                                    match channels.animation_checkpoint.get(&actor).copied() {
                                        Some(Some(slot)) => {
                                            channels.animation.insert(actor, slot);
                                        }
                                        Some(None) => {
                                            channels.animation.remove(&actor);
                                        }
                                        None => warn_once!(
                                            "script animation restore requires a captured slot"
                                        ),
                                    }
                                }
                            }
                        }
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
                                warn_once!(
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
                        ScriptSound::StopAll => {
                            #[cfg(target_arch = "wasm32")]
                            pending.0.clear();
                            channels.stop_all(commands);
                        }
                        ScriptSound::Animation { .. } => {
                            warn_once!("script animation sound requires a scene binding")
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
                            warn_once!("sound: animation cue for {actor} without a transform");
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
                                warn_once!("sound: effect {index}: {e}");
                                continue;
                            }
                        };
                        let started = channels.start(
                            commands,
                            &mut mixer,
                            Start {
                                owner: Some(bindings.get(*actor).ok().map_or(
                                    Owner::Entity(*actor),
                                    |(_, binding)| Owner::Script {
                                        generation: binding.actor.generation,
                                        index: binding.actor.index,
                                    },
                                )),
                                effect: Some(i32::from(*requested)),
                                audio: effect.audio.clone(),
                                bits_per_sample: effect.bits_per_sample,
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
        error_once!("sound control consumer: {error}");
    }
    if let Err(error) = world.run_system_cached(play_cues) {
        error_once!("sound cue consumer: {error}");
    }
}

pub(super) fn execute_ordinary(world: &mut World) {
    play_audio(world);
    if let Err(error) = world.run_system_cached(scoped_ordinary) {
        world
            .resource_mut::<cyrus_game::script::ScriptActors>()
            .report_runtime_error(format!("ordinary audio host: {error}"));
    }
    if let Err(error) = world.run_system_cached(finish_ordinary) {
        world
            .resource_mut::<cyrus_game::script::ScriptActors>()
            .report_runtime_error(format!("ordinary dialogue host: {error}"));
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
    /// Advance the explicit game-owned maintenance timer before object updates.
    /// The caller owns player-controller inhibition and world-entry resets.
    /// A successful reset releases RTX payloads, clears dialogue, stops the shared
    /// sample pool, then releases walk-node reservations. Partial failure is sticky
    /// in `maintenance`; no reset flag may be supplied to scripts after an error.
    pub fn maintain_sound(
        &mut self,
        maintenance: &mut cyrus_game::script::SoundMaintenance,
        nominal_ticks: u32,
        world: &mut cyrus_game::script::ScriptWorld,
        cache: &mut cyrus_assets::sound::DialogueCache<super::dialogue::DialogueClip>,
    ) -> anyhow::Result<bool> {
        use cyrus_game::script::SoundResetStage;
        maintenance.advance(nominal_ticks, &mut |stage| {
            match stage {
                SoundResetStage::ReleaseRtx => cache.reset(self)?,
                SoundResetStage::ClearDialogue => {
                    anyhow::Context::context(
                        world.globals_mut().dialogue_mut(),
                        "sound reset: dialogue records unavailable",
                    )?
                    .reset_active_lines();
                }
                SoundResetStage::StopSamples => {
                    self.frame.channels.stop_all(&mut self.frame.commands);
                }
                SoundResetStage::ClearNodeReservations => {
                    world.clear_walk_node_reservations()?;
                }
            }
            Ok(())
        })
    }

    fn check_dialogue_dependencies(
        &self,
        clip: &super::dialogue::DialogueClip,
    ) -> anyhow::Result<()> {
        let settings = anyhow::Context::context(
            self.frame.settings.as_deref(),
            "RTX start: sound settings unavailable",
        )?;
        let Some(voice) = clip.entry.voice.as_ref().filter(|_| settings.speech) else {
            return Ok(());
        };
        anyhow::ensure!(
            voice.loop_flag == 0,
            "RTX {:?}: looping voice playback unsupported",
            clip.entry.label
        );
        let audio = anyhow::Context::context(
            clip.audio.as_ref(),
            "RTX start: voice audio dependency absent",
        )?;
        anyhow::ensure!(
            self.frame
                .sources
                .as_deref()
                .is_some_and(|sources| sources.contains(audio)),
            "RTX {:?}: voice audio dependency not ready",
            clip.entry.label
        );
        anyhow::ensure!(
            self.frame.panned.is_some(),
            "RTX start: panned audio assets unavailable"
        );
        Ok(())
    }

    /// Start one loaded RTX voice on the shared sample allocator. The caller owns
    /// the cache entry and must remember the returned slot; text is not formatted
    /// or presented here. Missing dependencies fail before sample allocation.
    /// Speech disabled, text-only entries and channel exhaustion use text lifetime.
    /// See preservation engine/sound.md for start index and duration rules.
    pub fn start_dialogue(
        &mut self,
        clip: &super::dialogue::DialogueClip,
    ) -> anyhow::Result<super::dialogue::DialogueStart> {
        self.check_dialogue_dependencies(clip)?;
        let settings = self
            .frame
            .settings
            .as_deref()
            .expect("checked dialogue settings");
        let Some(voice) = clip.entry.voice.as_ref().filter(|_| settings.speech) else {
            return Ok(super::dialogue::DialogueStart {
                sample: None,
                lifetime: clip.entry.text_lifetime,
            });
        };
        let audio = clip.audio.as_ref().expect("checked dialogue audio");
        let mut mixer = Mixer {
            master: settings.master_volume,
            sources: self.frame.sources.as_deref(),
            panned: self.frame.panned.as_deref_mut(),
        };
        let sample = self.frame.channels.start(
            &mut self.frame.commands,
            &mut mixer,
            Start {
                owner: None,
                effect: None,
                audio: audio.clone(),
                bits_per_sample: voice.bits_per_sample,
                looping: false,
                volume: voice.base_volume.min(127),
                pan: Some(64),
                speed: playback_speed(voice.sample_rate, 0),
                start_index: 0,
            },
        );
        self.frame.channels.start = 6;
        Ok(super::dialogue::DialogueStart {
            sample,
            lifetime: if sample.is_some() {
                voice.lifetime
            } else {
                clip.entry.text_lifetime
            },
        })
    }

    /// Resolve one already prepared entry through the original twenty-slot cache,
    /// start its voice only when no sample is remembered, and format with the
    /// supplied installed REDSEL font. The host owns asset readiness and cache aging.
    pub fn cached_dialogue_line(
        &mut self,
        cache: &mut cyrus_assets::sound::DialogueCache<super::dialogue::DialogueClip>,
        label: [u8; 4],
        read: impl FnOnce([u8; 4]) -> anyhow::Result<Poll<super::dialogue::DialogueClip>>,
        hud: &cyrus_assets::hud::HudAsset,
        text_width: u32,
    ) -> anyhow::Result<Poll<cyrus_game::script::DialogueLine>> {
        let width = i32::try_from(text_width)
            .map_err(|_| anyhow::anyhow!("RTX text width exceeds signed range"))?;
        anyhow::ensure!(
            width >= 150,
            "RTX text width is below installed allocation minimum"
        );
        let index = if let Some(index) = cache.reuse(label) {
            index
        } else {
            let Poll::Ready(clip) = read(label)? else {
                return Ok(Poll::Pending);
            };
            anyhow::ensure!(
                clip.entry.label == label,
                "RTX prepared entry label mismatch"
            );
            cache.load(label, |_| Ok(clip))?
        };
        let clip = cache.payload(index).expect("loaded dialogue cache payload");
        let mut sample = cache.sample(index);
        let lifetime = if let Some(sample) = sample {
            anyhow::ensure!(sample < 16, "RTX cached sample slot out of range");
            clip.entry
                .voice
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("RTX remembered voice metadata absent"))?
                .lifetime
        } else {
            let started = self.start_dialogue(clip)?;
            sample = started.sample;
            started.lifetime
        };
        let text = hud.font.wrap(&clip.entry.text, width);
        if clip.entry.voice.is_some() {
            cache.remember_sample(index, sample)?;
        }
        Ok(Poll::Ready(cyrus_game::script::DialogueLine {
            text,
            sample,
            lifetime,
        }))
    }

    /// Lend synchronous, already prepared taunt services to the logical actor.
    /// The supplied clip must include ready audio dependencies before invocation;
    /// this path never yields a script function or repeats its random selection.
    pub fn with_combat_taunt_playback<R>(
        &mut self,
        cache: &mut cyrus_assets::sound::DialogueCache<super::dialogue::DialogueClip>,
        clip: &super::dialogue::DialogueClip,
        hud: Option<&cyrus_assets::hud::HudAsset>,
        update: impl FnOnce(&mut dyn cyrus_game::script::CombatTauntPlayback) -> R,
    ) -> R {
        update(&mut PreparedCombatTaunt {
            sounds: self,
            cache,
            clip,
            hud,
        })
    }

    /// Lend a prepared catalogue to synchronous SOUP combat dispatch. Selection
    /// consumes game RNG inside the call; missing labels fail before cache release.
    pub fn with_combat_taunt_source<R>(
        &mut self,
        cache: &mut cyrus_assets::sound::DialogueCache<super::dialogue::DialogueClip>,
        clips: &[super::dialogue::DialogueClip],
        hud: Option<&cyrus_assets::hud::HudAsset>,
        update: impl FnOnce(&dyn cyrus_game::script::CombatTauntSource) -> R,
    ) -> R {
        let source = PreparedCombatTauntSource {
            state: RefCell::new((self, cache)),
            clips,
            hud,
        };
        update(&source)
    }

    /// Lend a cache-backed source to one game-owned script invocation. The
    /// prepared-entry reader may resolve loaded native or browser assets; it must
    /// return Pending while dependencies load. No borrowed owner escapes.
    pub fn with_dialogue_source<R>(
        &mut self,
        cache: &mut cyrus_assets::sound::DialogueCache<super::dialogue::DialogueClip>,
        hud: &cyrus_assets::hud::HudAsset,
        read: &mut dyn FnMut([u8; 4]) -> anyhow::Result<Poll<super::dialogue::DialogueClip>>,
        update: impl FnOnce(&dyn cyrus_game::script::DialogueSource) -> R,
    ) -> R {
        let source = CachedDialogueSource {
            state: RefCell::new((self, cache, read)),
            hud,
        };
        update(&source)
    }

    /// Run ordinary game stages with a prepared RTX cache and the same shared
    /// channels used by ambient effects. Pending preparation retains the requesting task;
    /// missing reset ownership remains explicit rather than defaulting to false.
    pub fn run_ordinary_with_dialogue(
        &mut self,
        context: cyrus_game::script::OrdinaryContext,
        cache: &mut cyrus_assets::sound::DialogueCache<super::dialogue::DialogueClip>,
        hud: &cyrus_assets::hud::HudAsset,
        read: &mut dyn FnMut([u8; 4]) -> anyhow::Result<Poll<super::dialogue::DialogueClip>>,
        sound_reset: Option<bool>,
    ) {
        let generation = context.generation();
        let source = CachedDialogueSource {
            state: RefCell::new((self, cache, read)),
            hud,
        };
        let ambient = DialogueAmbient {
            source: &source,
            generation,
        };
        cyrus_game::script::run_ordinary(
            context,
            Some(&ambient),
            sound_reset,
            Some(&source),
            &mut |cue| {
                source
                    .state
                    .borrow_mut()
                    .0
                    .frame
                    .emit(&cue, Some(generation));
                Ok(())
            },
        );
    }

    /// Lend shared sample feedback to the game-owned dialogue record update.
    /// The pose reader must resolve the current logical speaker, including its
    /// world generation. The game retains text selection and lifetime ownership;
    /// presentation observes its selected record after this callback returns.
    pub fn with_dialogue_playback<R>(
        &mut self,
        position: &mut dyn FnMut(cyrus_game::script::ScriptActorId) -> anyhow::Result<[i32; 3]>,
        update: impl FnOnce(&mut dyn cyrus_game::script::DialoguePlayback) -> R,
    ) -> R {
        update(&mut SharedDialoguePlayback {
            sounds: self,
            position,
        })
    }

    /// Refresh a remembered dialogue voice from its speaker's signed Q8 pose.
    /// Speech retains centre pan while its volume follows the distance curve.
    /// The caller checks sample status first, as required by the dialogue stage.
    pub fn refresh_dialogue(&mut self, sample: u8, position: [i32; 3]) -> anyhow::Result<()> {
        anyhow::ensure!(sample < 16, "RTX volume: sample slot {sample} out of range");
        let settings = anyhow::Context::context(
            self.frame.settings.as_deref(),
            "RTX volume: sound settings unavailable",
        )?;
        let listener = anyhow::Context::context(
            self.frame.listener.as_deref(),
            "RTX volume: listener unavailable",
        )?;
        let (volume, _) =
            positional_q8(position, settings, listener, 0).map_err(anyhow::Error::msg)?;
        self.frame
            .channels
            .update_slot(sample, volume, settings.master_volume, 64)
            .map_err(|error| anyhow::anyhow!("RTX volume: {error}"))
    }

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

struct SharedDialoguePlayback<'a, 'w, 's> {
    sounds: &'a mut ScriptSounds<'w, 's>,
    position: &'a mut dyn FnMut(cyrus_game::script::ScriptActorId) -> anyhow::Result<[i32; 3]>,
}

impl cyrus_game::script::DialoguePlayback for SharedDialoguePlayback<'_, '_, '_> {
    fn sample_playing(&mut self, sample: u8) -> anyhow::Result<bool> {
        cyrus_assets::sound::DialogueSamples::playing(self.sounds, sample)
    }
    fn stop_sample(&mut self, sample: u8) -> anyhow::Result<()> {
        cyrus_assets::sound::DialogueSamples::stop(self.sounds, sample)
    }
    fn refresh_volume(
        &mut self,
        speaker: cyrus_game::script::ScriptActorId,
        sample: u8,
    ) -> anyhow::Result<()> {
        let position = (self.position)(speaker)?;
        self.sounds.refresh_dialogue(sample, position)
    }
    fn present_line(&mut self, _: usize, _: &str) -> anyhow::Result<()> {
        // DialogueState stores the selected record after this succeeds. The HUD
        // reads that selection; audio does not keep another presentation cursor.
        Ok(())
    }
}

struct PreparedCombatTaunt<'a, 'w, 's> {
    sounds: &'a mut ScriptSounds<'w, 's>,
    cache: &'a mut cyrus_assets::sound::DialogueCache<super::dialogue::DialogueClip>,
    clip: &'a super::dialogue::DialogueClip,
    hud: Option<&'a cyrus_assets::hud::HudAsset>,
}

impl cyrus_game::script::CombatTauntPlayback for PreparedCombatTaunt<'_, '_, '_> {
    fn prepare(
        &mut self,
        label: [u8; 4],
        speech: bool,
        text_width: Option<u32>,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            label == self.clip.entry.label,
            "combat taunt prepared label mismatch"
        );
        if let Some(width) = text_width {
            anyhow::ensure!(
                (150..=i32::MAX as u32).contains(&width),
                "combat taunt text width unsupported"
            );
            anyhow::Context::context(self.hud, "combat taunt font unavailable")?;
        }
        if speech {
            self.sounds.check_dialogue_dependencies(self.clip)?;
        }
        Ok(())
    }

    fn release(&mut self, retained: &mut Option<usize>) -> anyhow::Result<()> {
        self.cache.release_taunt(retained, self.sounds)
    }

    fn load(&mut self, label: [u8; 4]) -> anyhow::Result<usize> {
        self.cache
            .replace_taunt(&mut None, label, |_| Ok(self.clip.clone()), self.sounds)
    }

    fn present(
        &mut self,
        cache_index: usize,
        speech: bool,
        text_width: Option<u32>,
    ) -> anyhow::Result<cyrus_game::script::DialogueLine> {
        let clip = anyhow::Context::context(
            self.cache.payload(cache_index),
            "combat taunt cache payload absent",
        )?;
        let start = if speech {
            self.sounds.start_dialogue(clip)?
        } else {
            super::dialogue::DialogueStart {
                sample: None,
                lifetime: clip.entry.text_lifetime,
            }
        };
        if clip.entry.voice.is_some() {
            self.cache.remember_sample(cache_index, start.sample)?;
        }
        Ok(cyrus_game::script::DialogueLine {
            text: text_width
                .map(|width| {
                    self.hud
                        .expect("checked taunt font")
                        .font
                        .wrap(&self.clip.entry.text, width as i32)
                })
                .unwrap_or_default(),
            sample: start.sample,
            lifetime: start.lifetime,
        })
    }
}

struct PreparedCombatTauntSource<'a, 'w, 's> {
    state: RefCell<(
        &'a mut ScriptSounds<'w, 's>,
        &'a mut cyrus_assets::sound::DialogueCache<super::dialogue::DialogueClip>,
    )>,
    clips: &'a [super::dialogue::DialogueClip],
    hud: Option<&'a cyrus_assets::hud::HudAsset>,
}
impl std::fmt::Debug for PreparedCombatTauntSource<'_, '_, '_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PreparedCombatTauntSource")
    }
}
impl cyrus_game::script::CombatTauntSource for PreparedCombatTauntSource<'_, '_, '_> {
    fn with_playback(
        &self,
        update: &mut dyn FnMut(
            &mut dyn cyrus_game::script::CombatTauntPlayback,
        ) -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        update(&mut CatalogueCombatTaunt {
            source: self,
            clip: None,
        })
    }
}
struct CatalogueCombatTaunt<'a, 'b, 'w, 's> {
    source: &'a PreparedCombatTauntSource<'b, 'w, 's>,
    clip: Option<&'b super::dialogue::DialogueClip>,
}
impl CatalogueCombatTaunt<'_, '_, '_, '_> {
    fn with_prepared<R>(
        &mut self,
        update: impl FnOnce(&mut PreparedCombatTaunt<'_, '_, '_>) -> anyhow::Result<R>,
    ) -> anyhow::Result<R> {
        let clip = anyhow::Context::context(self.clip, "combat taunt catalogue not prepared")?;
        let mut state = self
            .source
            .state
            .try_borrow_mut()
            .map_err(|_| anyhow::anyhow!("combat taunt audio source already borrowed"))?;
        let (sounds, cache) = &mut *state;
        update(&mut PreparedCombatTaunt {
            sounds,
            cache,
            clip,
            hud: self.source.hud,
        })
    }
}
impl cyrus_game::script::CombatTauntPlayback for CatalogueCombatTaunt<'_, '_, '_, '_> {
    fn prepare(&mut self, label: [u8; 4], speech: bool, width: Option<u32>) -> anyhow::Result<()> {
        self.clip = Some(
            self.source
                .clips
                .iter()
                .find(|clip| clip.entry.label == label)
                .ok_or_else(|| {
                    anyhow::anyhow!("combat taunt {:?} missing from prepared catalogue", label)
                })?,
        );
        self.with_prepared(|playback| playback.prepare(label, speech, width))
    }
    fn release(&mut self, retained: &mut Option<usize>) -> anyhow::Result<()> {
        self.with_prepared(|playback| playback.release(retained))
    }
    fn load(&mut self, label: [u8; 4]) -> anyhow::Result<usize> {
        self.with_prepared(|playback| playback.load(label))
    }
    fn present(
        &mut self,
        index: usize,
        speech: bool,
        width: Option<u32>,
    ) -> anyhow::Result<cyrus_game::script::DialogueLine> {
        self.with_prepared(|playback| playback.present(index, speech, width))
    }
}

type DialogueReader<'a> =
    &'a mut dyn FnMut([u8; 4]) -> anyhow::Result<Poll<super::dialogue::DialogueClip>>;

struct CachedDialogueSource<'a, 'w, 's> {
    state: RefCell<(
        &'a mut ScriptSounds<'w, 's>,
        &'a mut cyrus_assets::sound::DialogueCache<super::dialogue::DialogueClip>,
        DialogueReader<'a>,
    )>,
    hud: &'a cyrus_assets::hud::HudAsset,
}

impl std::fmt::Debug for CachedDialogueSource<'_, '_, '_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CachedDialogueSource")
            .finish_non_exhaustive()
    }
}

impl cyrus_game::script::DialogueSource for CachedDialogueSource<'_, '_, '_> {
    fn player_line(
        &self,
        label: [u8; 4],
        _: cyrus_game::script::ScriptActorId,
        text_width: u32,
    ) -> anyhow::Result<Poll<Option<cyrus_game::script::DialogueLine>>> {
        let mut state = self.state.borrow_mut();
        let (sounds, cache, read) = &mut *state;
        sounds
            .cached_dialogue_line(cache, label, &mut **read, self.hud, text_width)
            .map(|line| line.map(Some))
    }
}

struct DialogueAmbient<'a, 'b, 'w, 's> {
    source: &'a CachedDialogueSource<'b, 'w, 's>,
    generation: Option<u64>,
}

impl std::fmt::Debug for DialogueAmbient<'_, '_, '_, '_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DialogueAmbient")
            .field("generation", &self.generation)
            .finish()
    }
}

impl cyrus_world::sound::AmbientSoundDriver for DialogueAmbient<'_, '_, '_, '_> {
    fn update(
        &self,
        request: cyrus_world::sound::AmbientSoundRequest,
    ) -> Result<cyrus_world::sound::AmbientSoundUpdate, String> {
        self.source
            .state
            .borrow_mut()
            .0
            .with_driver(self.generation, |driver, _| driver.update(request))
    }
}

#[derive(Resource, Default)]
pub(super) struct RuntimeDialogue {
    ordinary_completed: bool,
    generation: Option<u64>,
    hud: Option<Handle<cyrus_assets::hud::HudAsset>>,
    cache: cyrus_assets::sound::DialogueCache<super::dialogue::DialogueClip>,
    preparation: super::dialogue::DialoguePreparation,
}

#[derive(SystemParam)]
struct DialogueAssets<'w> {
    server: Option<Res<'w, AssetServer>>,
    clips: Option<Res<'w, Assets<super::dialogue::DialogueClip>>>,
    hud_assets: Option<Res<'w, Assets<cyrus_assets::hud::HudAsset>>>,
    runtime: ResMut<'w, RuntimeDialogue>,
    #[cfg(not(target_arch = "wasm32"))]
    install: Option<Res<'w, cyrus_assets::install::GameInstall>>,
}

fn scoped_ordinary(
    mut context: cyrus_game::script::OrdinaryContext,
    mut sounds: ScriptSounds,
    mut dialogue: DialogueAssets,
) {
    dialogue.runtime.ordinary_completed = false;
    if !context.is_running() {
        return;
    }
    let generation = context.generation();
    if let (Some(server), Some(clips), Some(hud_assets)) = (
        dialogue.server.as_deref(),
        dialogue.clips.as_deref(),
        dialogue.hud_assets.as_deref(),
    ) {
        let RuntimeDialogue {
            ordinary_completed,
            generation: prepared_generation,
            hud,
            cache,
            preparation,
        } = &mut *dialogue.runtime;
        if *prepared_generation != generation {
            preparation.clear();
            *prepared_generation = generation;
        }
        let hud_handle = hud.get_or_insert_with(|| server.load("cache://hud/overlay.hud"));
        if let Some(bevy::asset::LoadState::Failed(error)) = server.get_load_state(hud_handle.id())
        {
            context.report_error(format!("ordinary dialogue font: {error}"));
            return;
        }
        let Some(hud) = hud_assets.get(&*hud_handle) else {
            return;
        };
        let mut read = |label| {
            preparation.read(
                label,
                server,
                clips,
                #[cfg(not(target_arch = "wasm32"))]
                dialogue.install.as_deref(),
            )
        };
        *ordinary_completed = true;
        sounds.run_ordinary_with_dialogue(context, cache, hud, &mut read, None);
    } else {
        // Reset ownership remains with game maintenance; absent ownership is not
        // replaced by an assumed false flag.
        sounds.with_driver(generation, |driver, emit| {
            cyrus_game::script::run_ordinary(context, Some(driver), None, None, emit);
        });
    }
}

fn finish_ordinary(
    mut context: cyrus_game::script::OrdinaryContext,
    mut sounds: ScriptSounds,
    mut dialogue: ResMut<RuntimeDialogue>,
    skip: Option<Res<cyrus_game::player::input::DialogueSkip>>,
) {
    if !std::mem::take(&mut dialogue.ordinary_completed) {
        return;
    }
    let speech = sounds
        .frame
        .settings
        .as_ref()
        .map(|settings| settings.speech);
    let result = context.with_world_services(|world, ticks| -> anyhow::Result<()> {
        let speech =
            speech.ok_or_else(|| anyhow::anyhow!("dialogue sound settings unavailable"))?;
        let skip = skip
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("dialogue key_a input unavailable"))?
            .0;
        let positions = world.dialogue_speaker_positions()?;
        sounds.with_dialogue_playback(
            &mut |speaker| {
                positions
                    .iter()
                    .find(|(id, _)| *id == speaker)
                    .map(|(_, position)| *position)
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "dialogue speaker {} pose unavailable in generation {}",
                            speaker.index,
                            speaker.generation
                        )
                    })
            },
            |playback| world.advance_dialogue(ticks, speech, skip, playback),
        )?;
        dialogue.cache.advance(ticks, &mut sounds)?;
        Ok(())
    });
    if let Some(Err(error)) = result {
        context.report_error(format!("ordinary dialogue/cache update: {error:#}"));
    }
}

impl cyrus_assets::sound::DialogueSamples for ScriptSounds<'_, '_> {
    fn playing(&mut self, sample: u8) -> anyhow::Result<bool> {
        anyhow::ensure!(sample < 16, "dialogue sample {sample}: slot out of range");
        Ok(self.frame.channels.samples.status(Some(sample)) == 4)
    }

    fn stop(&mut self, sample: u8) -> anyhow::Result<()> {
        anyhow::ensure!(sample < 16, "dialogue sample {sample}: slot out of range");
        self.frame
            .channels
            .stop_slot(&mut self.frame.commands, sample);
        Ok(())
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod dialogue_tests {
    use super::*;
    use cyrus_assets::sound::DialogueSamples;

    fn maintenance_world(navigation: bool) -> cyrus_game::script::ScriptWorld {
        use cyrus_assets::{animation::Animations, script::Scripts};
        let mut scripts: Scripts = serde_json::from_value(serde_json::json!({
            "version":12,"world":7,"content_id":"","actor_pool_capacity":32,
            "player_distance_override":false,
            "clock":{"normal_frame_rate":12,"min_frame_rate":6,"max_frame_rate":300,
                "use_smooth_fps":true,"use_smooth_divisor":true},
            "updates":{"task_system":true,"animation_system":true,
                "disable_slaves":false,"disable_master_slaves":false},
            "dialogue":{"menu_start":[35,25],"max_menu_items":300,"max_dialog_lines":20,
                "max_text_width":500,"menu_traverse_delay":4,"print_text":true,
                "use_speech":true,"max_distance":1600,"rtx_filename":"ENGLISH.RTX"},
            "definition":{"functions":[{"name":"NullFunction","param_count":0,
                "kind":"function"}],"flags":[],"references":[],"attributes":[]},
            "actors":{},"unsupported":{},
            "instances":[]
        }))
        .unwrap();
        scripts.instances = (0..2)
            .map(|index| cyrus_assets::script::Instance {
                object_id: index,
                actor: "SCRIPTLESS".into(),
                object_type: 1,
                active: 1,
                model_name: String::new(),
                model_mode: 0,
                map_position: [0; 3],
                angles: [0; 3],
            })
            .collect();
        scripts.content_id = scripts.content_identity().unwrap();
        let mut world = cyrus_game::script::ScriptWorld::prepare(&scripts, 1).unwrap();
        if navigation {
            world
                .prepare_navigation(&Animations {
                    version: 2,
                    world: 7,
                    metadata: serde_json::json!({"script_content_id":scripts.content_id,
                    "walk_node_maps":[{"node_count":1,"walk_nodes":[{}]}]}),
                    sections: Default::default(),
                    models: Default::default(),
                    unsupported: Default::default(),
                })
                .unwrap();
        }
        let actor = world.id(0).unwrap();
        world
            .globals_mut()
            .dialogue_mut()
            .unwrap()
            .begin_line(actor, "Retained text".into(), Some(0), 100)
            .unwrap();
        world
    }

    #[test]
    fn maintenance_cleans_live_owners_only_after_the_threshold() {
        let mut game = maintenance_world(true);
        let first = game.id(0).unwrap();
        let second = game.id(1).unwrap();
        assert!(game.reserve_walk_node(first, 0, 0).unwrap());
        assert!(!game.reserve_walk_node(second, 0, 0).unwrap());
        let mut audio = World::new();
        audio.init_resource::<SoundEffects>();
        let mut channels = Channels::default();
        channels.samples.start(0, None, None).unwrap().unwrap();
        channels.samples.start(1, None, None).unwrap().unwrap();
        audio.insert_resource(channels);
        let mut cache = cyrus_assets::sound::DialogueCache::default();
        let slot = cache
            .load(*b"TEST", |_| {
                Ok(super::super::dialogue::DialogueClip {
                    entry: cyrus_assets::sound::DialogueEntry {
                        version: 1,
                        label: *b"TEST",
                        text: "Cached text".into(),
                        text_lifetime: 100,
                        voice: None,
                    },
                    audio: None,
                })
            })
            .unwrap();
        let mut timer = cyrus_game::script::SoundMaintenance::default();
        let mut state = bevy::ecs::system::SystemState::<ScriptSounds>::new(&mut audio);
        {
            let mut sounds = state.get_mut(&mut audio).unwrap();
            assert!(
                !sounds
                    .maintain_sound(&mut timer, 1400, &mut game, &mut cache)
                    .unwrap()
            );
            assert!(game.globals().dialogue().unwrap().waiting());
            assert!(cache.payload(slot).is_some());
            assert!(sounds.playing(0).unwrap() && sounds.playing(1).unwrap());
            assert!(
                sounds
                    .maintain_sound(&mut timer, 1, &mut game, &mut cache)
                    .unwrap()
            );
            assert!(!sounds.playing(0).unwrap() && !sounds.playing(1).unwrap());
        }
        assert!(!game.globals().dialogue().unwrap().waiting());
        assert!(cache.payload(slot).is_none());
        assert!(game.reserve_walk_node(second, 0, 0).unwrap());
        assert!(timer.reset().unwrap());
    }

    #[test]
    fn maintenance_missing_navigation_retains_partial_failure() {
        let mut game = maintenance_world(false);
        let mut audio = World::new();
        audio.init_resource::<SoundEffects>();
        let mut channels = Channels::default();
        channels.samples.start(0, None, None).unwrap().unwrap();
        audio.insert_resource(channels);
        let mut cache = cyrus_assets::sound::DialogueCache::default();
        let mut timer = cyrus_game::script::SoundMaintenance::default();
        let mut state = bevy::ecs::system::SystemState::<ScriptSounds>::new(&mut audio);
        let mut sounds = state.get_mut(&mut audio).unwrap();
        let error = sounds
            .maintain_sound(&mut timer, 1401, &mut game, &mut cache)
            .unwrap_err()
            .to_string();
        assert!(error.contains("ClearNodeReservations: walk-node navigation not prepared"));
        assert!(!game.globals().dialogue().unwrap().waiting());
        assert!(!sounds.playing(0).unwrap());
        assert!(timer.reset().is_err());
        assert_eq!(
            sounds
                .maintain_sound(&mut timer, 0, &mut game, &mut cache)
                .unwrap_err()
                .to_string(),
            error
        );
    }

    #[derive(Resource, Default)]
    struct Observation {
        before: bool,
        after: bool,
        invalid_status: bool,
        invalid_stop: bool,
    }

    fn stop_dialogue(mut sounds: ScriptSounds, mut observation: ResMut<Observation>) {
        observation.before = sounds.playing(0).unwrap();
        sounds.stop(0).unwrap();
        observation.after = sounds.playing(0).unwrap();
        observation.invalid_status = sounds.playing(16).is_err();
        observation.invalid_stop = sounds.stop(16).is_err();
    }

    #[test]
    fn dialogue_feedback_stops_the_shared_slot_before_deferred_commands() {
        let mut world = World::new();
        world.init_resource::<SoundEffects>();
        world.init_resource::<Observation>();
        let mut channels = Channels::default();
        assert_eq!(
            channels.samples.start(0, None, None).unwrap().unwrap().slot,
            0
        );
        world.insert_resource(channels);
        world.run_system_cached(stop_dialogue).unwrap();
        let observation = world.resource::<Observation>();
        assert!(observation.before);
        assert!(!observation.after);
        assert!(observation.invalid_status && observation.invalid_stop);
        assert_eq!(world.resource::<Channels>().samples.status(Some(0)), 2);
    }
    fn dialogue_playback(mut sounds: ScriptSounds) {
        let speaker = cyrus_game::script::ScriptActorId {
            generation: 7,
            index: 2,
        };
        let mut reads = Vec::new();
        sounds.with_dialogue_playback(
            &mut |actor| {
                reads.push(actor);
                anyhow::ensure!(actor == speaker, "expired dialogue speaker");
                Ok([256, 0, 16_777_215])
            },
            |playback| {
                assert!(playback.sample_playing(0).unwrap());
                assert!(
                    playback
                        .refresh_volume(speaker, 0)
                        .unwrap_err()
                        .to_string()
                        .contains("playback transport unavailable")
                );
                assert!(
                    playback
                        .refresh_volume(
                            cyrus_game::script::ScriptActorId {
                                generation: 6,
                                index: 2,
                            },
                            0
                        )
                        .is_err()
                );
                playback.present_line(0, "selected by game").unwrap();
                playback.stop_sample(0).unwrap();
                assert!(!playback.sample_playing(0).unwrap());
                assert!(playback.stop_sample(16).is_err());
            },
        );
        assert_eq!(reads.len(), 2);
    }

    #[test]
    fn dialogue_playback_resolves_logical_speaker_and_stops_shared_sample() {
        let mut world = World::new();
        world.init_resource::<SoundEffects>();
        world.insert_resource(SoundSettings::parse("[system]\nsound_distance=64\n"));
        world.insert_resource(crate::AudioListener(GlobalTransform::default()));
        let mut channels = Channels::default();
        channels.samples.start(0, None, None).unwrap().unwrap();
        world.insert_resource(channels);
        world.run_system_cached(dialogue_playback).unwrap();
        assert_eq!(world.resource::<Channels>().samples.status(Some(0)), 2);
    }

    #[derive(Resource)]
    struct LoadedDialogue(super::super::dialogue::DialogueClip);
    #[derive(Resource, Default)]
    struct Starts(Vec<anyhow::Result<super::super::dialogue::DialogueStart>>);

    fn start_line(mut sounds: ScriptSounds, clip: Res<LoadedDialogue>, mut starts: ResMut<Starts>) {
        starts.0.push(sounds.start_dialogue(&clip.0));
    }

    fn refresh_line(mut sounds: ScriptSounds) {
        sounds.refresh_dialogue(0, [256, 0, 16_777_215]).unwrap();
        assert!(sounds.refresh_dialogue(16, [0; 3]).is_err());
    }

    #[test]
    fn dialogue_voice_uses_shared_slots_and_text_fallback_without_stealing() {
        use cyrus_assets::sound::{DialogueEntry, DialogueVoice};
        let mut world = World::new();
        world.init_resource::<SoundEffects>();
        world.init_resource::<Channels>();
        world.init_resource::<Starts>();
        world.insert_resource(SoundSettings::parse(""));
        let mut sources = Assets::<AudioSource>::default();
        let audio = sources.add(AudioSource {
            bytes: vec![0; 4].into(),
        });
        world.insert_resource(sources);
        world.init_resource::<Assets<Panned>>();
        world.insert_resource(LoadedDialogue(super::super::dialogue::DialogueClip {
            entry: DialogueEntry {
                version: 1,
                label: *b"TEST",
                text: "Hi".into(),
                text_lifetime: 30,
                voice: Some(DialogueVoice {
                    audio: "dialogue/54455354.wav".into(),
                    base_volume: 113,
                    sample_rate: 11025,
                    channels: 1,
                    bits_per_sample: 16,
                    loop_flag: 0,
                    loop_offset: 0,
                    loop_end: 0,
                    lifetime: 128,
                }),
            },
            audio: Some(audio),
        }));
        world.resource_mut::<Channels>().start = 15;
        world.run_system_cached(start_line).unwrap();
        assert_eq!(
            world.resource::<Starts>().0[0].as_ref().unwrap(),
            &super::super::dialogue::DialogueStart {
                sample: Some(0),
                lifetime: 128
            }
        );
        let channels = world.resource::<Channels>();
        assert_eq!(channels.start, 6);
        assert_eq!(channels.samples.status(Some(0)), 4);
        let channel = channels.playing.values().next().unwrap();
        let scale = channel.gains.as_ref().unwrap().snapshot();
        assert_eq!(
            [scale.apply(0.5, 0), scale.apply(0.5, 1)],
            [113.0 / 256.0; 2]
        );
        assert!(
            world
                .resource_mut::<Channels>()
                .samples
                .stop_matching(
                    Some(Owner::Script {
                        generation: 7,
                        index: 2
                    }),
                    None
                )
                .is_empty()
        );
        assert_eq!(world.resource::<Channels>().samples.status(Some(0)), 4);
        world.resource_mut::<SoundSettings>().sound_distance = Some(20);
        world.insert_resource(crate::AudioListener(GlobalTransform::IDENTITY));
        world.run_system_cached(refresh_line).unwrap();
        let scale = world
            .resource::<Channels>()
            .playing
            .values()
            .next()
            .unwrap()
            .gains
            .as_ref()
            .unwrap()
            .snapshot();
        assert_eq!([scale.apply(0.5, 0), scale.apply(0.5, 1)], [0.5; 2]);
        for _ in 1..16 {
            world
                .resource_mut::<Channels>()
                .samples
                .start(0, None, None)
                .unwrap()
                .unwrap();
        }
        world.run_system_cached(start_line).unwrap();
        assert_eq!(
            world.resource::<Starts>().0[1].as_ref().unwrap(),
            &super::super::dialogue::DialogueStart {
                sample: None,
                lifetime: 30
            }
        );
        assert_eq!(world.resource::<Channels>().playing.len(), 1);
        world.resource_mut::<SoundSettings>().speech = false;
        world.resource_mut::<LoadedDialogue>().0.audio = None;
        world.run_system_cached(start_line).unwrap();
        assert_eq!(
            world.resource::<Starts>().0[2].as_ref().unwrap(),
            &super::super::dialogue::DialogueStart {
                sample: None,
                lifetime: 30
            }
        );
        world.resource_mut::<SoundSettings>().speech = true;
        world.run_system_cached(start_line).unwrap();
        assert!(
            world.resource::<Starts>().0[3]
                .as_ref()
                .unwrap_err()
                .to_string()
                .contains("dependency absent")
        );
        assert_eq!(world.resource::<Channels>().playing.len(), 1);
    }
    fn dialogue_test_hud() -> cyrus_assets::hud::HudAsset {
        serde_json::from_value(serde_json::json!({
            "labels": {}, "candle_mode": 0, "candle": [0,0], "candle_size": [0,0],
            "pickup": [0,0], "prompt": [0,0], "override_ticks": 0,
            "compass": [0,0], "compass_size": [0,0], "world_compass": {},
            "powerup_slots": [[0,0],[0,0]], "powerup_sizes": [],
            "logbook": [0,0], "logbook_size": [0,0], "walk_mode": false,
            "auto_defend": false, "notices": [["",""],["",""]], "inventory": {}, "start_items": [], "items": [],
            "selected": null, "bindings": [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],
            "font": {"start": 0, "line_height": 0, "glyphs": [], "palette": []},
            "text_alpha": [255,255], "player_text_alpha": [255,97]
        }))
        .unwrap()
    }

    fn cached_line(mut sounds: ScriptSounds, clip: Res<LoadedDialogue>) {
        let hud = dialogue_test_hud();
        let mut cache = cyrus_assets::sound::DialogueCache::default();
        sounds.with_dialogue_source(&mut cache, &hud, &mut |_| Ok(Poll::Pending), |source| {
            let player = cyrus_game::script::ScriptActorId {
                generation: 2,
                index: 8,
            };
            assert!(matches!(
                source.player_line(*b"TEST", player, 150).unwrap(),
                Poll::Pending
            ));
        });
        assert!(
            cache.payload(0).is_none(),
            "pending preparation must not occupy a cache slot"
        );
        assert_eq!(
            sounds.frame.channels.samples.status(Some(0)),
            2,
            "pending preparation must not start a voice"
        );
        let mut reads = 0;
        let mut read = |label| {
            reads += 1;
            assert_eq!(label, *b"TEST");
            Ok(Poll::Ready(clip.0.clone()))
        };
        sounds.with_dialogue_source(&mut cache, &hud, &mut read, |source| {
            let player = cyrus_game::script::ScriptActorId {
                generation: 2,
                index: 8,
            };
            for _ in 0..2 {
                let Poll::Ready(Some(line)) = source.player_line(*b"TEST", player, 150).unwrap()
                else {
                    panic!("prepared line must be ready")
                };
                assert_eq!(line.text, "Hi");
                assert_eq!(line.sample, Some(0));
                assert_eq!(line.lifetime, 128);
            }
        });
        assert_eq!(reads, 1);
        assert_eq!(cache.sample(0), Some(0));
        cache.reset(&mut sounds).unwrap();
        assert!(cache.payload(0).is_none());
    }

    fn taunt_cache_restart(mut sounds: ScriptSounds, clip: Res<LoadedDialogue>) {
        let mut cache = cyrus_assets::sound::DialogueCache::default();
        let hud = dialogue_test_hud();
        let (index, first) =
            sounds.with_combat_taunt_playback(&mut cache, &clip.0, Some(&hud), |source| {
                source.prepare(*b"TEST", true, Some(150)).unwrap();
                let index = source.load(*b"TEST").unwrap();
                let line = source.present(index, true, Some(150)).unwrap();
                (index, line)
            });
        assert_eq!(
            (first.text.as_str(), first.sample, first.lifetime),
            ("Hi", Some(0), 128)
        );
        assert_eq!(sounds.frame.channels.samples.status(Some(0)), 4);
        let first_entity = sounds.frame.channels.playing.values().next().unwrap().sound;
        let mut retained = Some(index);
        let mut invalid = clip.0.clone();
        invalid.audio = None;
        sounds.with_combat_taunt_playback(&mut cache, &invalid, Some(&hud), |source| {
            assert!(source.prepare(*b"TEST", true, Some(150)).is_err());
        });
        assert_eq!(cache.sample(index), Some(0));
        assert_eq!(sounds.frame.channels.samples.status(Some(0)), 4);
        let (next, second) =
            sounds.with_combat_taunt_playback(&mut cache, &clip.0, None, |source| {
                source.prepare(*b"TEST", true, None).unwrap();
                source.release(&mut retained).unwrap();
                assert_eq!(retained, None);
                let next = source.load(*b"TEST").unwrap();
                (next, source.present(next, true, None).unwrap())
            });
        assert_eq!(
            (second.text.as_str(), second.sample, second.lifetime),
            ("", Some(0), 128)
        );
        assert_eq!(sounds.frame.channels.samples.status(Some(0)), 4);
        assert_ne!(
            sounds.frame.channels.playing.values().next().unwrap().sound,
            first_entity
        );
        assert_eq!(cache.sample(next), Some(0));
        // A cache hit restarts even without a retained previous actor reference.
        let unvoiced =
            sounds.with_combat_taunt_playback(&mut cache, &clip.0, Some(&hud), |source| {
                source.prepare(*b"TEST", false, Some(150)).unwrap();
                let reused = source.load(*b"TEST").unwrap();
                assert_eq!(reused, next);
                source.present(reused, false, Some(150)).unwrap()
            });
        assert_eq!(
            (unvoiced.text.as_str(), unvoiced.sample, unvoiced.lifetime),
            ("Hi", None, 30)
        );
        assert_eq!(sounds.frame.channels.samples.status(Some(0)), 2);
        assert_eq!(cache.sample(next), None);
    }

    fn catalogue_taunt(mut sounds: ScriptSounds, clip: Res<LoadedDialogue>) {
        let hud = dialogue_test_hud();
        let mut cache = cyrus_assets::sound::DialogueCache::default();
        let mut retained = None;
        sounds.with_combat_taunt_source(
            &mut cache,
            std::slice::from_ref(&clip.0),
            Some(&hud),
            |source| {
                source
                    .with_playback(&mut |playback| {
                        playback.prepare(*b"TEST", true, Some(150))?;
                        let index = playback.load(*b"TEST")?;
                        let line = playback.present(index, true, Some(150))?;
                        assert_eq!((line.text.as_str(), line.sample), ("Hi", Some(0)));
                        retained = Some(index);
                        Ok(())
                    })
                    .unwrap();
            },
        );
        let entity = sounds.frame.channels.playing.values().next().unwrap().sound;
        sounds.with_combat_taunt_source(&mut cache, &[], Some(&hud), |source| {
            assert!(
                source
                    .with_playback(&mut |playback| {
                        playback.prepare(*b"MISS", true, Some(150))?;
                        playback.release(&mut retained)
                    })
                    .is_err()
            );
        });
        assert!(retained.is_some());
        assert_eq!(sounds.frame.channels.samples.status(Some(0)), 4);
        assert_eq!(
            sounds.frame.channels.playing.values().next().unwrap().sound,
            entity
        );
        let mut other = clip.0.clone();
        other.entry.label = *b"NEXT";
        other.entry.text = "Next".into();
        sounds.with_combat_taunt_source(
            &mut cache,
            &[clip.0.clone(), other],
            Some(&hud),
            |source| {
                source
                    .with_playback(&mut |playback| {
                        playback.prepare(*b"NEXT", false, Some(150))?;
                        playback.release(&mut retained)?;
                        let index = playback.load(*b"NEXT")?;
                        let line = playback.present(index, false, Some(150))?;
                        assert_eq!((line.text.as_str(), line.sample), ("Next", None));
                        Ok(())
                    })
                    .unwrap();
            },
        );
    }

    #[test]
    fn combat_catalogue_selects_prepared_label_and_rejects_missing_before_release() {
        let mut world = dialogue_test_world();
        world.run_system_cached(catalogue_taunt).unwrap();
        assert!(world.resource::<Channels>().playing.is_empty());
    }

    fn dialogue_test_world() -> World {
        use cyrus_assets::sound::{DialogueEntry, DialogueVoice};
        let mut world = World::new();
        world.init_resource::<SoundEffects>();
        world.init_resource::<Channels>();
        world.insert_resource(SoundSettings::parse(""));
        let mut sources = Assets::<AudioSource>::default();
        let audio = sources.add(AudioSource {
            bytes: vec![0; 4].into(),
        });
        world.insert_resource(sources);
        world.init_resource::<Assets<Panned>>();
        world.insert_resource(LoadedDialogue(super::super::dialogue::DialogueClip {
            entry: DialogueEntry {
                version: 1,
                label: *b"TEST",
                text: "Hi".into(),
                text_lifetime: 30,
                voice: Some(DialogueVoice {
                    audio: "dialogue/54455354.wav".into(),
                    base_volume: 100,
                    sample_rate: 11025,
                    channels: 1,
                    bits_per_sample: 16,
                    loop_flag: 0,
                    loop_offset: 0,
                    loop_end: 0,
                    lifetime: 128,
                }),
            },
            audio: Some(audio),
        }));
        world
    }

    #[test]
    fn combat_taunt_restarts_cached_voice_and_preflights_before_releasing() {
        let mut world = dialogue_test_world();
        world.run_system_cached(taunt_cache_restart).unwrap();
        assert!(world.resource::<Channels>().playing.is_empty());
    }

    #[test]
    fn dialogue_source_reuses_prepared_entry_and_sample_then_resets_shared_slot() {
        let mut world = dialogue_test_world();
        world.run_system_cached(cached_line).unwrap();
        let channels = world.resource::<Channels>();
        assert_eq!(channels.samples.status(Some(0)), 2);
        assert!(channels.playing.is_empty());
    }
}
