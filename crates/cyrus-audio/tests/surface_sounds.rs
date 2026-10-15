//! Headless playback contract: remapped PCM, source ownership and invalid-state rejection.
use bevy::{
    audio::{AudioPlayer, AudioSource, Decodable, Source},
    prelude::*,
};
use cyrus_assets::{
    audio::pcm_wav,
    sound::{SoundSettings, SurfaceSounds},
};
use cyrus_audio::{
    AudioListener,
    sound::{Effect, Panned, PlayingSound, SoundEffects, SoundPlugin},
};
use cyrus_game::script::{ScriptActorId, ScriptBinding};
use cyrus_world::{
    animation::AnimationSound,
    sound::{ScriptSound, SoundCue, SoundSurface},
};
use rgpre::import::sfx::AudioType;

fn fixture() -> (App, Entity) {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default(), SoundPlugin))
        .init_asset::<AudioSource>()
        .init_asset::<Panned>()
        .insert_resource(SoundSettings::parse("[system]\nsound_distance=64"))
        .insert_resource(AudioListener(GlobalTransform::default()))
        .insert_resource(
            SurfaceSounds::parse("[surfaces]\n[wood]\n200=1\n201=600\n[unknown]\n0=1").unwrap(),
        );
    let effects = [11025, 22050]
        .into_iter()
        .map(|rate| {
            let audio = app
                .world_mut()
                .resource_mut::<Assets<AudioSource>>()
                .add(AudioSource {
                    bytes: pcm_wav(AudioType::Mono8, rate, &[192; 16]).unwrap().into(),
                });
            Some(Effect {
                bits_per_sample: 8,
                audio,
                looping: true,
                base_volume: 64,
                sample_rate: rate,
            })
        })
        .collect();
    app.insert_resource(SoundEffects(effects));
    let actor = app
        .world_mut()
        .spawn((
            GlobalTransform::from_translation(Vec3::Z),
            SoundSurface(7),
            ScriptBinding {
                actor: ScriptActorId {
                    generation: 17,
                    index: 4,
                },
                source: Handle::default(),
            },
        ))
        .id();
    (app, actor)
}

fn play(app: &mut App, actor: Entity, effect: i16) {
    let surface = app.world().get::<SoundSurface>(actor).map(|value| value.0);
    app.world_mut().write_message(SoundCue::Animation {
        position: None,
        actor,
        surface,
        sound: AnimationSound::Play {
            effect,
            rate_offset_hz: 0,
        },
    });
    app.update();
}

fn playing(app: &mut App) -> usize {
    app.world_mut()
        .query_filtered::<Entity, With<PlayingSound>>()
        .iter(app.world())
        .count()
}

#[test]
fn remapped_audio_uses_target_pcm_but_cleanup_matches_authored_source() {
    let (mut app, actor) = fixture();
    play(&mut app, actor, 200);
    assert_eq!(
        playing(&mut app),
        1,
        "source 200 is outside the bank, but its target 1 is playable"
    );
    let handle = app
        .world_mut()
        .query::<&AudioPlayer<Panned>>()
        .single(app.world())
        .unwrap()
        .0
        .clone();
    assert_eq!(
        app.world()
            .resource::<Assets<Panned>>()
            .get(&handle)
            .unwrap()
            .decoder()
            .sample_rate()
            .get(),
        22050
    );
    for (effect, expected) in [(1, 1), (200, 0)] {
        app.world_mut().write_message(SoundCue::Script {
            generation: 17,
            index: 4,
            sound: ScriptSound::StopEffect {
                effect,
                owned: true,
            },
        });
        app.update();
        assert_eq!(playing(&mut app), expected);
    }
    play(&mut app, actor, 200);
    app.world_mut().write_message(SoundCue::Animation {
        position: None,
        actor,
        sound: AnimationSound::Stop,
        surface: None,
    });
    app.update();
    assert_eq!(playing(&mut app), 0);
}

#[test]
fn invalid_sources_targets_classes_and_unknown_ground_do_not_start_audio() {
    let (mut app, actor) = fixture();
    for source in [-1, 256, 201] {
        play(&mut app, actor, source);
        assert_eq!(playing(&mut app), 0);
    }
    app.world_mut().entity_mut(actor).insert(SoundSurface(12));
    play(&mut app, actor, 0);
    assert_eq!(playing(&mut app), 0);
    app.world_mut().entity_mut(actor).remove::<SoundSurface>();
    play(&mut app, actor, 0);
    assert_eq!(playing(&mut app), 0);
    app.world_mut().entity_mut(actor).insert(SoundSurface(7));
    app.world_mut().remove_resource::<SurfaceSounds>();
    play(&mut app, actor, 200);
    assert_eq!(playing(&mut app), 0);
    app.world_mut().entity_mut(actor).insert(SoundSurface(0));
    play(&mut app, actor, 0);
    assert_eq!(playing(&mut app), 1, "known zero needs no table");
}

#[test]
fn known_zero_and_script_flat_sounds_bypass_configured_remaps() {
    let (mut app, actor) = fixture();
    app.world_mut().entity_mut(actor).insert(SoundSurface(0));
    play(&mut app, actor, 0);
    let handle = app
        .world_mut()
        .query::<&AudioPlayer<Panned>>()
        .single(app.world())
        .unwrap()
        .0
        .clone();
    assert_eq!(
        app.world()
            .resource::<Assets<Panned>>()
            .get(&handle)
            .unwrap()
            .decoder()
            .sample_rate()
            .get(),
        11025
    );
    app.world_mut().write_message(SoundCue::Script {
        generation: 17,
        index: 4,
        sound: ScriptSound::Flat {
            effect: 200,
            volume: 0,
            rate_offset_hz: 0,
        },
    });
    app.update();
    assert_eq!(
        playing(&mut app),
        1,
        "script source 200 must not become target 1"
    );
}

#[test]
fn queued_animation_keeps_emission_surface_and_cleanup_needs_no_surface() {
    let (mut app, actor) = fixture();
    app.world_mut().write_message(SoundCue::Animation {
        position: None,
        actor,
        sound: AnimationSound::Play {
            effect: 200,
            rate_offset_hz: 0,
        },
        surface: Some(7),
    });
    app.world_mut().entity_mut(actor).remove::<SoundSurface>();
    app.update();
    assert_eq!(
        playing(&mut app),
        1,
        "later missing ground cannot discard a captured wood cue"
    );
    app.world_mut().write_message(SoundCue::Animation {
        position: None,
        actor,
        sound: AnimationSound::Forget,
        surface: None,
    });
    app.world_mut().write_message(SoundCue::Animation {
        position: None,
        actor,
        sound: AnimationSound::Stop,
        surface: None,
    });
    app.update();
    assert_eq!(
        playing(&mut app),
        1,
        "Forget clears the animation handle without stopping its channel"
    );
    app.world_mut().entity_mut(actor).insert(SoundSurface(7));
    app.world_mut().write_message(SoundCue::Animation {
        position: None,
        actor,
        sound: AnimationSound::Play {
            effect: 200,
            rate_offset_hz: 0,
        },
        surface: None,
    });
    app.update();
    assert_eq!(
        playing(&mut app),
        1,
        "later known ground must not fill an unknown cue snapshot"
    );
    app.world_mut().write_message(SoundCue::Animation {
        position: None,
        actor,
        sound: AnimationSound::Play {
            effect: 200,
            rate_offset_hz: 0,
        },
        surface: Some(7),
    });
    app.world_mut().entity_mut(actor).insert(SoundSurface(0));
    app.update();
    assert_eq!(
        playing(&mut app),
        2,
        "later zero must not replace a captured wood class"
    );
    app.world_mut().write_message(SoundCue::Animation {
        position: None,
        actor,
        sound: AnimationSound::Stop,
        surface: None,
    });
    app.update();
    assert_eq!(playing(&mut app), 1, "Stop works with no snapshot");
}

#[test]
fn animation_end_stops_the_current_occupant_of_its_reused_sample_slot() {
    let (mut app, actor) = fixture();
    app.world_mut().write_message(SoundCue::Animation {
        position: None,
        actor,
        sound: AnimationSound::Play {
            effect: 200,
            rate_offset_hz: 0,
        },
        surface: Some(7),
    });
    app.update();
    let finished = app
        .world_mut()
        .query_filtered::<Entity, With<PlayingSound>>()
        .single(app.world())
        .unwrap();
    app.world_mut().despawn(finished);
    app.world_mut().write_message(SoundCue::Flat {
        effect: 0,
        volume: 0,
        rate_offset_hz: 0,
    });
    app.update();
    assert_eq!(playing(&mut app), 1);
    app.world_mut().write_message(SoundCue::Animation {
        position: None,
        actor,
        sound: AnimationSound::Stop,
        surface: None,
    });
    app.update();
    assert_eq!(
        playing(&mut app),
        0,
        "the remembered slot names its current occupant"
    );
}

#[test]
fn all_sample_stop_preserves_the_animation_handle_until_end_animation() {
    let (mut app, actor) = fixture();
    app.world_mut().write_message(SoundCue::Animation {
        position: None,
        actor,
        sound: AnimationSound::Play {
            effect: 200,
            rate_offset_hz: 0,
        },
        surface: Some(7),
    });
    app.update();
    app.world_mut()
        .write_message(cyrus_audio::sound::SoundRequest::StopAll);
    app.update();
    assert_eq!(playing(&mut app), 0);
    app.world_mut().write_message(SoundCue::Flat {
        effect: 0,
        volume: 0,
        rate_offset_hz: 0,
    });
    app.update();
    assert_eq!(playing(&mut app), 1);
    app.world_mut().write_message(SoundCue::Animation {
        position: None,
        actor,
        sound: AnimationSound::Stop,
        surface: None,
    });
    app.update();
    assert_eq!(playing(&mut app), 0);
}

#[test]
fn synchronous_ambient_and_immediate_commands_share_reusable_slots_and_live_gain_control() {
    use cyrus_audio::sound::ScriptSounds;
    use cyrus_world::sound::AmbientSoundRequest;

    fn interleave(mut sounds: ScriptSounds) {
        sounds.with_driver(Some(17), |driver, emit| {
            let request = AmbientSoundRequest {
                generation: 17,
                actor_index: 7,
                channel: None,
                effect: 0,
                volume: 0,
                rate_offset_hz: 0,
                position: [0, 0, 16_777_215],
                object_update_excluded: false,
            };
            let first = driver.update(request).unwrap();
            assert_eq!(first.channel, Some(0));
            assert!(first.active);
            emit(SoundCue::Script {
                generation: 17,
                index: 8,
                sound: ScriptSound::Flat {
                    effect: 1,
                    volume: 0,
                    rate_offset_hz: 0,
                },
            })
            .unwrap();
            emit(SoundCue::Script {
                generation: 17,
                index: 7,
                sound: ScriptSound::StopOwned,
            })
            .unwrap();
            emit(SoundCue::Script {
                generation: 17,
                index: 8,
                sound: ScriptSound::Flat {
                    effect: 1,
                    volume: 0,
                    rate_offset_hz: 0,
                },
            })
            .unwrap();
            // The remembered slot now holds the immediate FlatSound. Ambient
            // updates that current sample instead of restarting its old effect.
            let updated = driver
                .update(AmbientSoundRequest {
                    channel: first.channel,
                    volume: -64,
                    ..request
                })
                .unwrap();
            assert_eq!(updated.channel, Some(0));
            assert!(updated.active);
            assert!(
                driver
                    .update(AmbientSoundRequest {
                        generation: 18,
                        ..request
                    })
                    .is_err()
            );
            let ended = driver
                .update(AmbientSoundRequest {
                    channel: Some(1),
                    object_update_excluded: true,
                    ..request
                })
                .unwrap();
            assert_eq!(ended.channel, Some(1));
            assert!(!ended.active);
        });
    }
    let (mut app, _) = fixture();
    app.world_mut().run_system_cached(interleave).unwrap();
    assert_eq!(playing(&mut app), 1);
    let player = app
        .world_mut()
        .query::<&AudioPlayer<Panned>>()
        .single(app.world())
        .unwrap();
    let audio = app
        .world()
        .resource::<Assets<Panned>>()
        .get(&player.0)
        .unwrap();
    let mut decoder = audio.decoder();
    assert_eq!(decoder.sample_rate().get(), 22050);
    assert_eq!(decoder.next(), Some(0.25));
    assert_eq!(decoder.next(), Some(0.25));
}

#[test]
fn ordinary_and_animation_audio_drains_do_not_replay_the_same_cue() {
    fn early_audio(world: &mut World) {
        let execute = world
            .resource::<cyrus_game::script::OrdinarySoundExecutor>()
            .0;
        execute(world);
    }
    let (mut app, _) = fixture();
    app.init_resource::<cyrus_game::script::ScriptActors>()
        .init_resource::<cyrus_game::random::EngineRandom>()
        .init_asset::<cyrus_assets::animation::Animations>()
        .add_systems(Update, early_audio.before(cyrus_audio::PlayCues));
    app.world_mut().write_message(SoundCue::Flat {
        effect: 0,
        volume: 0,
        rate_offset_hz: 0,
    });
    app.update();
    assert_eq!(playing(&mut app), 1);
    app.update();
    assert_eq!(playing(&mut app), 1);
}

#[test]
fn animation_uses_emission_position_even_after_the_scene_transform_changes() {
    let (mut app, actor) = fixture();
    app.world_mut()
        .entity_mut(actor)
        .insert(GlobalTransform::from_translation(Vec3::splat(1_000_000.0)));
    app.world_mut().write_message(SoundCue::Animation {
        actor,
        sound: AnimationSound::Play {
            effect: 0,
            rate_offset_hz: 0,
        },
        surface: Some(0),
        position: Some([0, 0, 16_777_215]),
    });
    app.update();
    assert_eq!(playing(&mut app), 1);
    let player = app
        .world_mut()
        .query::<&AudioPlayer<Panned>>()
        .single(app.world())
        .unwrap();
    let audio = app
        .world()
        .resource::<Assets<Panned>>()
        .get(&player.0)
        .unwrap();
    let mut decoder = audio.decoder();
    assert_eq!(decoder.next(), Some(0.5));
    assert_eq!(decoder.next(), Some(0.5));
}

#[test]
fn animation_checkpoint_restores_remembered_slot_without_stopping_new_sound() {
    use cyrus_world::sound::AnimationSoundState;
    let (mut app, actor) = fixture();
    play(&mut app, actor, 200);
    let first = app
        .world_mut()
        .query_filtered::<Entity, With<PlayingSound>>()
        .single(app.world())
        .unwrap();
    app.world_mut().write_message(SoundCue::Script {
        generation: 17,
        index: 4,
        sound: ScriptSound::AnimationState(AnimationSoundState::Capture),
    });
    app.update();
    play(&mut app, actor, 200);
    assert_eq!(playing(&mut app), 2);
    app.world_mut().write_message(SoundCue::Script {
        generation: 17,
        index: 4,
        sound: ScriptSound::AnimationState(AnimationSoundState::Restore),
    });
    app.update();
    assert_eq!(
        playing(&mut app),
        2,
        "restoring the receiver changes only its remembered handle"
    );
    app.world_mut().write_message(SoundCue::Animation {
        actor,
        position: None,
        surface: None,
        sound: AnimationSound::Stop,
    });
    app.update();
    assert_eq!(playing(&mut app), 1);
    assert!(
        app.world().get_entity(first).is_err(),
        "EndAnimation stops the restored earlier slot"
    );
}

#[test]
fn animation_checkpoint_can_restore_absent_slot_and_current_replacement() {
    use cyrus_world::sound::AnimationSoundState;
    let (mut app, actor) = fixture();
    app.world_mut().write_message(SoundCue::Script {
        generation: 17,
        index: 4,
        sound: ScriptSound::AnimationState(AnimationSoundState::Capture),
    });
    app.update();
    play(&mut app, actor, 200);
    app.world_mut().write_message(SoundCue::Script {
        generation: 17,
        index: 4,
        sound: ScriptSound::AnimationState(AnimationSoundState::Restore),
    });
    app.world_mut().write_message(SoundCue::Animation {
        actor,
        position: None,
        surface: None,
        sound: AnimationSound::Stop,
    });
    app.update();
    assert_eq!(
        playing(&mut app),
        1,
        "a captured absent handle is restored as absent"
    );
    let (mut app, actor) = fixture();
    play(&mut app, actor, 200);
    app.world_mut().write_message(SoundCue::Script {
        generation: 17,
        index: 4,
        sound: ScriptSound::AnimationState(AnimationSoundState::Capture),
    });
    app.update();
    let remembered = app
        .world_mut()
        .query_filtered::<Entity, With<PlayingSound>>()
        .single(app.world())
        .unwrap();
    app.world_mut().despawn(remembered);
    app.world_mut().write_message(SoundCue::Flat {
        effect: 0,
        volume: 0,
        rate_offset_hz: 0,
    });
    app.update();
    assert_eq!(playing(&mut app), 1);
    app.world_mut().write_message(SoundCue::Script {
        generation: 17,
        index: 4,
        sound: ScriptSound::AnimationState(AnimationSoundState::Restore),
    });
    app.world_mut().write_message(SoundCue::Animation {
        actor,
        position: None,
        surface: None,
        sound: AnimationSound::Stop,
    });
    app.update();
    assert_eq!(
        playing(&mut app),
        0,
        "a reusable remembered slot stops its replacement occupant"
    );
}

#[test]
fn remapped_target_retains_its_pcm_width_through_production_playback() {
    let (mut app, actor) = fixture();
    *app.world_mut().get_mut::<GlobalTransform>(actor).unwrap() = GlobalTransform::IDENTITY;
    app.world_mut()
        .resource_mut::<SoundSettings>()
        .master_volume = 255;
    let pcm = [0, 64].repeat(4);
    let audio = app
        .world_mut()
        .resource_mut::<Assets<AudioSource>>()
        .add(AudioSource {
            bytes: pcm_wav(AudioType::Mono16, 22050, &pcm).unwrap().into(),
        });
    let mut effects = app.world_mut().resource_mut::<SoundEffects>();
    let target = effects.0[1].as_mut().unwrap();
    target.audio = audio;
    target.bits_per_sample = 16;
    play(&mut app, actor, 200);
    let handle = app
        .world_mut()
        .query::<&AudioPlayer<Panned>>()
        .single(app.world())
        .unwrap()
        .0
        .clone();
    let audio = app
        .world()
        .resource::<Assets<Panned>>()
        .get(&handle)
        .unwrap();
    assert_eq!(
        audio.decoder().take(8).collect::<Vec<_>>(),
        vec![16128.0 / 32768.0; 8]
    );
}

#[test]
fn separately_initialized_player_role_uses_player_samples_without_becoming_controlled() {
    for (player_source, expected) in [(false, 10), (true, 11)] {
        let (mut app, actor) = fixture();
        for _ in 0..10 {
            let other = app
                .world_mut()
                .spawn((GlobalTransform::from_translation(Vec3::Z), SoundSurface(7)))
                .id();
            play(&mut app, other, 200);
        }
        assert_eq!(playing(&mut app), 10);
        if player_source {
            app.world_mut()
                .entity_mut(actor)
                .insert(cyrus_world::sound::PlayerAnimationSource);
        }
        assert!(
            app.world()
                .get::<cyrus_game::player::Player>(actor)
                .is_none()
        );
        play(&mut app, actor, 200);
        assert_eq!(playing(&mut app), expected);
    }
}

#[test]
fn production_loop_rechecks_gains_and_finite_cues_complete() {
    use bevy::audio::{PlaybackMode, PlaybackSettings};
    use cyrus_audio::sound::PannedGains;
    for looping in [true, false] {
        let (mut app, actor) = fixture();
        app.world_mut().resource_mut::<SoundEffects>().0[1]
            .as_mut()
            .unwrap()
            .looping = looping;
        play(&mut app, actor, 200);
        let (player, settings, gains) = app
            .world_mut()
            .query::<(&AudioPlayer<Panned>, &PlaybackSettings, &PannedGains)>()
            .single(app.world())
            .unwrap();
        let source = app
            .world()
            .resource::<Assets<Panned>>()
            .get(&player.0)
            .unwrap();
        let gains = gains.clone();
        let mut decoder: Box<dyn Iterator<Item = f32>> = match settings.mode {
            PlaybackMode::Loop => Box::new(source.decoder().repeat_infinite()),
            _ => Box::new(source.decoder()),
        };
        assert!(decoder.by_ref().take(32).all(|sample| sample > 0.0));
        if looping {
            gains.set(0, 256, 64);
            assert_eq!(decoder.take(64).collect::<Vec<_>>(), [0.0; 64]);
        } else {
            assert_eq!(decoder.next(), None);
        }
    }
}

#[test]
fn malformed_transport_starts_do_not_exhaust_the_shared_sample_pool() {
    let (mut app, actor) = fixture();
    app.world_mut()
        .entity_mut(actor)
        .insert(cyrus_world::sound::PlayerAnimationSource);
    let handle = app.world().resource::<SoundEffects>().0[1]
        .as_ref()
        .unwrap()
        .audio
        .clone();
    let source = app
        .world()
        .resource::<Assets<AudioSource>>()
        .get(&handle)
        .unwrap()
        .clone();
    app.world_mut()
        .resource_mut::<Assets<AudioSource>>()
        .get_mut(&handle)
        .unwrap()
        .bytes = vec![0; 4].into();
    for _ in 0..20 {
        play(&mut app, actor, 200);
        assert_eq!(playing(&mut app), 0);
    }
    *app.world_mut()
        .resource_mut::<Assets<AudioSource>>()
        .get_mut(&handle)
        .unwrap() = source;
    for count in 1..=16 {
        play(&mut app, actor, 200);
        assert_eq!(playing(&mut app), count);
    }
}

#[test]
fn installed_loop_pcm_updates_gains_and_reconstructs_through_the_headless_host() {
    use cyrus_assets::{audio::sfx_path, install::GameInstall};
    use cyrus_audio::sound::{PannedGains, PannedSnapshot, PcmWidth};
    let root = std::env::var_os("REDGUARD_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../redguard"));
    let Ok(install) = GameInstall::locate(Some(&root)) else {
        eprintln!("skipped: no install (looping transport headless host)");
        return;
    };
    let bank =
        rgpre::import::sfx::parse_sfx_file(&std::fs::read(sfx_path(&install)).unwrap()).unwrap();
    let effect = bank
        .effects
        .iter()
        .find(|effect| effect.loop_flag != 0 && !effect.pcm_data.is_empty())
        .unwrap();
    let width: PcmWidth = effect.audio_type.bits_per_sample().try_into().unwrap();
    let frames = effect.pcm_data.len()
        / usize::from(effect.audio_type.bits_per_sample() / 8)
        / usize::from(effect.audio_type.channels());
    let (mut app, actor) = fixture();
    let audio = app
        .world_mut()
        .resource_mut::<Assets<AudioSource>>()
        .add(AudioSource {
            bytes: pcm_wav(effect.audio_type, effect.sample_rate, &effect.pcm_data)
                .unwrap()
                .into(),
        });
    let mut effects = app.world_mut().resource_mut::<SoundEffects>();
    let target = effects.0[1].as_mut().unwrap();
    target.audio = audio;
    target.bits_per_sample = effect.audio_type.bits_per_sample();
    target.sample_rate = effect.sample_rate;
    target.looping = true;
    play(&mut app, actor, 200);
    let (player, gains) = app
        .world_mut()
        .query::<(&AudioPlayer<Panned>, &PannedGains)>()
        .single(app.world())
        .unwrap();
    let transport = app
        .world()
        .resource::<Assets<Panned>>()
        .get(&player.0)
        .unwrap();
    let mut decoder = transport.decoder();
    let first_loop = decoder.by_ref().take(frames * 2).collect::<Vec<_>>();
    assert_eq!(first_loop.len(), frames * 2);
    assert!(first_loop.iter().any(|sample| *sample != 0.0));
    gains.set(0, 256, 64);
    assert!(
        decoder
            .by_ref()
            .take(frames * 2)
            .all(|sample| sample == 0.0)
    );
    gains.set(127, 256, 32);
    decoder.next();
    let snapshot: PannedSnapshot =
        serde_json::from_slice(&serde_json::to_vec(&transport.snapshot().unwrap()).unwrap())
            .unwrap();
    let replacement = Panned::from_snapshot(transport.source().clone(), width, snapshot).unwrap();
    let mut restored = replacement.decoder();
    assert_eq!(
        decoder.by_ref().take(1024).collect::<Vec<_>>(),
        restored.by_ref().take(1024).collect::<Vec<_>>()
    );
}
