use bevy::audio::{AudioSource, Decodable, Source};
use cyrus_assets::audio::load_speech;
use cyrus_assets::audio::pcm_wav;
use cyrus_assets::audio::read_settings;
use cyrus_assets::audio::sfx_path;
use cyrus_assets::install::GameInstall;
use cyrus_assets::music::cue_path;
use cyrus_assets::music::read_track;
use cyrus_assets::music::world_track;
use cyrus_assets::sound::SoundSettings;
use cyrus_assets::sound::slider_volume;
use cyrus_audio::music::redbook_gain;
use cyrus_audio::sound::distance_volume;
use cyrus_audio::sound::heading;
use cyrus_audio::sound::output_gain;
use cyrus_audio::sound::pan;
use cyrus_audio::sound::pan_gains;
use rgpre::import::sfx::parse_sfx_file;
use std::path::Path;

#[test]
fn playing_decoder_applies_gain_updates_without_restarting_or_splitting_a_frame() {
    use cyrus_audio::sound::{Panned, PannedGains};
    use rgpre::import::sfx::AudioType;

    for (audio_type, pcm) in [
        (AudioType::Mono8, vec![192; 4]),
        (AudioType::Stereo8, vec![192; 8]),
        (AudioType::Mono16, [0, 64].repeat(4)),
        (AudioType::Stereo16, [0, 64].repeat(8)),
    ] {
        let gains = PannedGains::new(
            audio_type.bits_per_sample().try_into().unwrap(),
            127,
            256,
            0,
        );
        let source = Panned::new(
            AudioSource {
                bytes: pcm_wav(audio_type, 11025, &pcm).unwrap().into(),
            },
            gains.clone(),
        )
        .unwrap();
        let mut decoder = source.decoder();
        assert!(decoder.next().unwrap() > 0.0);
        gains.set(127, 256, 127);
        assert_eq!(decoder.next(), Some(0.0));
        assert_eq!(decoder.next(), Some(0.0));
        assert!(decoder.next().unwrap() > 0.0);
        gains.set(0, 256, 64);
        assert_eq!(decoder.next(), Some(0.0));
        assert_eq!(decoder.next(), Some(0.0));
        gains.set(127, 256, 0);
        assert!(decoder.next().unwrap() > 0.0);
        assert_eq!(decoder.next(), Some(0.0));
        assert_eq!(decoder.next(), None);
    }
}

/// Decodes `source` with Bevy's own decoder: (sample rate, channels, sample count).
fn decode(source: AudioSource) -> (u32, u16, usize) {
    let decoder = source.decoder();
    let (rate, channels) = (decoder.sample_rate().get(), decoder.channels().get());
    (rate, channels, decoder.count())
}

fn install(check: &str) -> Option<GameInstall> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../redguard");
    let install = GameInstall::locate(Some(&root)).ok();
    if install.is_none() {
        eprintln!("skipped: no install ({check})");
    }
    install
}

#[test]
fn installed_effects_decode_at_their_stored_format() {
    let Some(install) = install("sound effect") else {
        return;
    };
    let file = parse_sfx_file(&std::fs::read(sfx_path(&install)).unwrap()).unwrap();
    assert_eq!(
        file.effects.len(),
        118,
        "MAIN.SFX effect count per preservation"
    );
    for (index, effect) in file.effects.iter().enumerate() {
        let wav = pcm_wav(effect.audio_type, effect.sample_rate, &effect.pcm_data)
            .unwrap_or_else(|e| panic!("effect {index}: {e}"));
        let bytes_per_sample = usize::from(effect.audio_type.bits_per_sample() / 8);
        assert_eq!(
            decode(AudioSource { bytes: wav.into() }),
            (
                effect.sample_rate,
                effect.audio_type.channels(),
                effect.pcm_data.len() / bytes_per_sample
            ),
            "effect {index}"
        );
    }
    let looping = file.effects.iter().filter(|e| e.loop_flag != 0).count();
    assert!(looping > 0 && looping < file.effects.len());
}

#[test]
fn installed_voice_clips_all_load() {
    let Some(install) = install("speech") else {
        return;
    };
    let settings = read_settings(&install);
    assert!(settings.effects && settings.music && settings.speech);
    assert_eq!(settings.redbook_volume, 200);
    assert_eq!(
        (settings.master_volume, settings.sound_distance),
        (255, Some(64))
    );
    let clips = load_speech(&install.root.join(&settings.rtx_filename)).unwrap();
    assert_eq!(clips.len(), 3933, "RTX voice clip count per preservation");
    let (rate, channels, samples) = decode(AudioSource {
        bytes: clips.values().next().unwrap().audio.clone().into(),
    });
    assert!(rate > 0 && channels > 0 && samples > 0);
}

#[test]
fn installed_world_music_reads_red_book_tracks() {
    let Some(install) = install("music") else {
        return;
    };
    let Some(cue) = cue_path(&install) else {
        eprintln!("skipped: no install (game.ins beside install)");
        return;
    };
    assert_eq!(world_track(&install, "1").unwrap(), 2);
    let (rate, channels, samples) = decode(AudioSource {
        bytes: read_track(&cue, 2).unwrap().into(),
    });
    assert_eq!((rate, channels), (44_100, 2));
    assert!(samples > 44_100 * 2 * 60, "track 2 lasts over a minute");
    assert!(read_track(&cue, 1).is_err(), "track 1 is the data track");
    // The GOG image's track 2 starts with two sync-marked CD-ROM data sectors.
    let mut track = read_track(&cue, 2).unwrap();
    assert_eq!(
        cyrus_assets::disc::mute_leading_data_sectors(&mut track[44..]),
        4728
    );
    let mut later = read_track(&cue, 3).unwrap();
    assert_eq!(
        cyrus_assets::disc::mute_leading_data_sectors(&mut later[44..]),
        0
    );
}

#[test]
fn distance_volume_matches_the_original_curve() {
    // Shipped sound_distance 64: radius 2304, divisor 3640 (preservation docs/engine/sound.md).
    for (distance, volume) in [
        (0.0, 127),
        (18.0, 127),
        (19.0, 126),
        (500.0, 100),
        (1152.0, 64),
        (2286.0, 1),
        (2287.0, 0),
        (2305.0, 0),
    ] {
        assert_eq!(
            distance_volume(64, distance, 0),
            Ok(volume),
            "distance {distance}"
        );
    }
    assert_eq!(distance_volume(64, 0.0, -40), Ok(87));
    assert_eq!(distance_volume(64, 1000.0, 100), Ok(127));
    assert_eq!(distance_volume(64, 2400.0, 100), Ok(0), "beyond the radius");
    assert_eq!(distance_volume(64, 0.0, i32::MAX), Ok(0));
    assert_eq!(distance_volume(64, 0.0, i32::MAX - 127), Ok(127));
    assert_eq!(distance_volume(64, 0.0, i32::MIN), Ok(0));
    assert!(
        distance_volume(3, 0.0, 0).is_err(),
        "the original divides by zero"
    );
    // Miles mixer: (v + 1) / 128 after v = volume × master >> 8.
    assert_eq!(
        output_gain(cyrus_audio::sound::PcmWidth::Eight, 127, 256),
        1.0
    );
    assert_eq!(
        output_gain(cyrus_audio::sound::PcmWidth::Eight, 127, 128),
        0.5,
        "v 63"
    );
    assert_eq!(
        output_gain(cyrus_audio::sound::PcmWidth::Eight, 64, 255),
        0.5,
        "v 63"
    );
    assert_eq!(
        output_gain(cyrus_audio::sound::PcmWidth::Eight, 1, 255),
        0.0,
        "v 0 is silent"
    );
    // Sound page slider to engine volume (preservation docs/engine/menu.md).
    let volumes: Vec<i32> = (0..=20).map(slider_volume).collect();
    assert_eq!(
        volumes,
        [
            0, 12, 25, 38, 51, 64, 76, 89, 102, 115, 128, 140, 153, 166, 179, 192, 204, 217, 230,
            243, 256
        ]
    );
    assert_eq!(redbook_gain(slider_volume(9)), Some(230.0 / 255.0));
    assert_eq!(
        redbook_gain(slider_volume(10)),
        None,
        "128 is ignored by the drive"
    );
}

#[test]
fn pan_follows_the_original_heading_and_miles_pan_law() {
    // Emitter offsets (dx, dz) with the listener facing +Z (preservation docs/engine/sound.md).
    for ((dx, dz), expected) in [
        ((0, 100), 64),
        ((100, 100), 96),
        ((100, 0), 64),
        ((100, -1), 127),
        ((0, -100), 64),
        ((-100, -100), 32),
        ((-100, 0), 64),
        ((-100, 100), 32),
    ] {
        assert_eq!(pan(heading(dx, dz), 0), expected, "({dx}, {dz})");
    }
    assert_eq!(heading(100, -1), 515);
    assert_eq!(pan(heading(100, 100), 512), 32, "listener turned a quarter");
    // Miles pan table: centre keeps both channels full; extremes silence one.
    assert_eq!(pan_gains(64), [1.0, 1.0]);
    assert_eq!(pan_gains(127), [0.0, 1.0]);
    assert_eq!(pan_gains(0), [1.0, 0.0]);
    assert_eq!(pan_gains(96), [62.0 / 128.0, 1.0]);
    assert_eq!(pan_gains(32), [1.0, 64.0 / 128.0]);
}

#[test]
fn system_ini_switches_default_on() {
    let defaults = SoundSettings::parse("");
    assert!(defaults.effects && defaults.music && defaults.speech);
    assert_eq!(defaults.rtx_filename, "ENGLISH.RTX");
    let off = SoundSettings::parse(
        "[system]\nsound=0\nredbook=off\nrtx_filename=FRENCH.RTX\n[dialog]\ndialog_use_speech=0\n[other]\nsound=1\n",
    );
    assert!(!off.effects && !off.music && !off.speech);
    assert_eq!(off.rtx_filename, "FRENCH.RTX");
}

#[test]
fn flat_cues_fill_sixteen_channels_with_clamped_rate_and_base_volume() {
    use bevy::audio::{PlaybackSettings, Volume};
    use bevy::prelude::*;
    use cyrus_audio::sound::Effect;
    use cyrus_audio::sound::PlayingSound;
    use cyrus_audio::sound::SoundEffects;
    use cyrus_audio::sound::SoundPlugin;
    use cyrus_world::sound::SoundCue;
    let Some(install) = install("sound channel") else {
        return;
    };
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, SoundPlugin))
        .insert_resource(install);
    app.update();
    let master = app.world().resource::<SoundSettings>().master_volume;
    app.insert_resource(SoundEffects(vec![Some(Effect {
        bits_per_sample: 8,
        audio: Handle::default(),
        looping: true,
        base_volume: 64,
        sample_rate: 22050,
    })]));
    for _ in 0..17 {
        app.world_mut().write_message(SoundCue::Flat {
            effect: 0,
            volume: -4,
            rate_offset_hz: -8 * 64,
        });
    }
    app.world_mut().write_message(SoundCue::Flat {
        effect: 1,
        volume: 0,
        rate_offset_hz: 0,
    });
    app.update();
    let mut playing = app
        .world_mut()
        .query_filtered::<&PlaybackSettings, With<PlayingSound>>();
    let settings: Vec<_> = playing.iter(app.world()).collect();
    assert_eq!(
        settings.len(),
        16,
        "16 channels; the 17th start and effect 1 are dropped"
    );
    assert_eq!(settings[0].speed, 21538.0 / 22050.0);
    assert_eq!(
        settings[0].volume,
        Volume::Linear(output_gain(cyrus_audio::sound::PcmWidth::Eight, 60, master))
    );
}

#[test]
fn animation_cues_need_a_known_surface_and_stop_their_last_sound() {
    use bevy::audio::AudioSource;
    use bevy::prelude::*;
    use cyrus_audio::AudioListener;
    use cyrus_audio::sound::Effect;
    use cyrus_audio::sound::Panned;
    use cyrus_audio::sound::PlayingSound;
    use cyrus_audio::sound::SoundEffects;
    use cyrus_audio::sound::SoundPlugin;
    use cyrus_world::animation::AnimationSound;
    use cyrus_world::sound::SoundCue;
    use cyrus_world::sound::SoundSurface;
    use rgpre::import::sfx::AudioType;
    let Some(install) = install("animation sound cue") else {
        return;
    };
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default(), SoundPlugin))
        .init_asset::<AudioSource>()
        .init_asset::<Panned>()
        .insert_resource(install);
    app.update();
    let audio = app
        .world_mut()
        .resource_mut::<Assets<AudioSource>>()
        .add(AudioSource {
            bytes: pcm_wav(AudioType::Mono8, 11025, &[128; 64]).unwrap().into(),
        });
    app.insert_resource(SoundEffects(vec![Some(Effect {
        bits_per_sample: 8,
        audio,
        looping: true,
        base_volume: 64,
        sample_rate: 11025,
    })]));
    app.world_mut()
        .insert_resource(AudioListener(GlobalTransform::default()));
    let known = app
        .world_mut()
        .spawn((GlobalTransform::from_translation(Vec3::Z), SoundSurface(0)))
        .id();
    let unknown = app
        .world_mut()
        .spawn(GlobalTransform::from_translation(Vec3::Z))
        .id();
    for actor in [known, unknown] {
        app.world_mut().write_message(SoundCue::Animation {
            position: None,
            actor,
            surface: (actor == known).then_some(0),
            sound: AnimationSound::Play {
                effect: 0,
                rate_offset_hz: 0,
            },
        });
    }
    app.update();
    let mut playing = app
        .world_mut()
        .query_filtered::<Entity, With<PlayingSound>>();
    assert_eq!(
        playing.iter(app.world()).count(),
        1,
        "only the known surface plays"
    );
    app.world_mut().write_message(SoundCue::Animation {
        position: None,
        actor: known,
        surface: Some(0),
        sound: AnimationSound::Stop,
    });
    app.update();
    assert_eq!(
        playing.iter(app.world()).count(),
        0,
        "Stop ends the actor's last sound"
    );
    app.world_mut()
        .entity_mut(known)
        .insert(cyrus_game::script::ScriptBinding {
            actor: cyrus_game::script::ScriptActorId {
                generation: 17,
                index: 4,
            },
            source: Handle::default(),
        });
    app.world_mut().write_message(SoundCue::Animation {
        position: None,
        actor: known,
        surface: Some(0),
        sound: AnimationSound::Play {
            effect: 0,
            rate_offset_hz: 0,
        },
    });
    app.update();
    assert_eq!(playing.iter(app.world()).count(), 1);
    for (index, expected) in [(3, 1), (4, 0)] {
        app.world_mut().write_message(SoundCue::Script {
            generation: 17,
            index,
            sound: cyrus_world::sound::ScriptSound::StopEffect {
                effect: 0,
                owned: true,
            },
        });
        app.update();
        assert_eq!(
            playing.iter(app.world()).count(),
            expected,
            "script cleanup matches the bound animation's logical receiver"
        );
    }
}

#[test]
fn script_sound_cleanup_matches_owner_effect_and_world_generation() {
    use bevy::prelude::*;
    use cyrus_audio::sound::{Effect, PlayingSound, SoundEffects, SoundPlugin};
    use cyrus_world::sound::{ScriptSound, SoundCue};
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, SoundPlugin));
    app.insert_resource(SoundSettings::parse(""));
    app.insert_resource(SoundEffects(
        (0..2)
            .map(|_| {
                Some(Effect {
                    bits_per_sample: 8,
                    audio: Handle::default(),
                    looping: true,
                    base_volume: 64,
                    sample_rate: 22050,
                })
            })
            .collect(),
    ));
    let send = |app: &mut App, index, sound| {
        app.world_mut().write_message(SoundCue::Script {
            generation: 17,
            index,
            sound,
        });
    };
    let count = |app: &mut App| {
        app.world_mut()
            .query_filtered::<Entity, With<PlayingSound>>()
            .iter(app.world())
            .count()
    };
    for (actor, effect) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
        send(
            &mut app,
            actor,
            ScriptSound::Flat {
                effect,
                volume: 0,
                rate_offset_hz: 0,
            },
        );
    }
    app.update();
    assert_eq!(count(&mut app), 4);
    send(
        &mut app,
        0,
        ScriptSound::StopEffect {
            effect: 0,
            owned: true,
        },
    );
    app.update();
    assert_eq!(count(&mut app), 3);
    send(&mut app, 0, ScriptSound::StopOwned);
    app.update();
    assert_eq!(count(&mut app), 2);
    send(
        &mut app,
        0,
        ScriptSound::StopEffect {
            effect: 1,
            owned: false,
        },
    );
    app.update();
    assert_eq!(count(&mut app), 1);
    send(&mut app, 1, ScriptSound::StopOwned);
    app.update();
    assert_eq!(count(&mut app), 0);
    // Global cleanup crosses script ownership and unowned presentation samples.
    for index in [0, 1] {
        send(
            &mut app,
            index,
            ScriptSound::Flat {
                effect: 0,
                volume: 0,
                rate_offset_hz: 0,
            },
        );
    }
    app.world_mut().write_message(SoundCue::Flat {
        effect: 1,
        volume: 0,
        rate_offset_hz: 0,
    });
    app.update();
    assert_eq!(count(&mut app), 3);
    send(&mut app, 0, ScriptSound::StopAll);
    // A later start in the same invocation survives the earlier global stop.
    send(
        &mut app,
        1,
        ScriptSound::Flat {
            effect: 1,
            volume: 0,
            rate_offset_hz: 0,
        },
    );
    app.update();
    assert_eq!(count(&mut app), 1);
    send(&mut app, 0, ScriptSound::StopAll);
    app.update();
    assert_eq!(count(&mut app), 0);
    // A start and its cleanup in one update cannot leave a deferred sound alive.
    send(
        &mut app,
        0,
        ScriptSound::Flat {
            effect: 0,
            volume: 0,
            rate_offset_hz: 0,
        },
    );
    send(&mut app, 0, ScriptSound::StopOwned);
    app.update();
    assert_eq!(count(&mut app), 0);
    send(
        &mut app,
        0,
        ScriptSound::Flat {
            effect: 0,
            volume: i32::MAX,
            rate_offset_hz: i32::MAX,
        },
    );
    app.update();
    assert_eq!(count(&mut app), 1);
    let settings = app
        .world_mut()
        .query_filtered::<&bevy::audio::PlaybackSettings, With<PlayingSound>>()
        .single(app.world())
        .unwrap();
    assert_eq!(settings.volume, bevy::audio::Volume::Linear(0.0));
    assert_eq!(settings.speed, 512.0 / 22050.0);
    app.init_resource::<cyrus_game::script::ScriptActors>();
    send(
        &mut app,
        0,
        ScriptSound::Flat {
            effect: 0,
            volume: 0,
            rate_offset_hz: 0,
        },
    );
    app.update();
    assert_eq!(
        count(&mut app),
        0,
        "world removal stops owned channels and rejects stale starts"
    );
}

#[test]
fn headless_sound_preparation_waits_for_host_readiness() {
    use bevy::prelude::*;
    use cyrus_audio::{AudioReadiness, SoundPlugin};
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, SoundPlugin))
        .insert_resource(GameInstall {
            root: std::env::temp_dir().join("cyrus-absent-install"),
        })
        .insert_resource(AudioReadiness { prepare: false });
    app.update();
    assert!(!app.world().contains_resource::<SoundSettings>());
    app.world_mut().resource_mut::<AudioReadiness>().prepare = true;
    app.update();
    assert!(app.world().contains_resource::<SoundSettings>());
    assert!(
        app.world()
            .resource::<cyrus_audio::sound::SoundEffects>()
            .0
            .is_empty()
    );
}

#[test]
fn positional_script_starts_include_silent_channels_and_owned_cleanup() {
    use bevy::prelude::*;
    use cyrus_audio::{
        AudioListener,
        sound::{Effect, Panned, PlayingSound, SoundEffects, SoundPlugin},
    };
    use cyrus_world::sound::{ScriptSound, SoundCue};
    use rgpre::import::sfx::AudioType;
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default(), SoundPlugin))
        .init_asset::<AudioSource>()
        .init_asset::<Panned>();
    app.insert_resource(SoundSettings::parse("[system]\nsound_distance=64\n"));
    app.insert_resource(AudioListener(GlobalTransform::default()));
    let audio = app
        .world_mut()
        .resource_mut::<Assets<AudioSource>>()
        .add(AudioSource {
            bytes: pcm_wav(AudioType::Mono8, 11025, &[192; 8]).unwrap().into(),
        });
    app.insert_resource(SoundEffects(vec![Some(Effect {
        bits_per_sample: 8,
        audio,
        looping: true,
        base_volume: 64,
        sample_rate: 11025,
    })]));
    for (index, volume) in [(0, 0), (1, i32::MAX)] {
        app.world_mut().write_message(SoundCue::Script {
            generation: 17,
            index,
            sound: ScriptSound::Positional {
                effect: 0,
                volume,
                rate_offset_hz: 0,
                position: [0, 0, 16_777_215],
                start_index: None,
            },
        });
    }
    app.update();
    assert_eq!(
        app.world_mut()
            .query_filtered::<Entity, With<PlayingSound>>()
            .iter(app.world())
            .count(),
        2
    );
    app.world_mut().write_message(SoundCue::Script {
        generation: 17,
        index: 0,
        sound: ScriptSound::StopOwned,
    });
    app.update();
    assert_eq!(
        app.world_mut()
            .query_filtered::<Entity, With<PlayingSound>>()
            .iter(app.world())
            .count(),
        1
    );
    let handle = app
        .world_mut()
        .query::<&bevy::audio::AudioPlayer<Panned>>()
        .single(app.world())
        .unwrap()
        .0
        .clone();
    let source = app
        .world()
        .resource::<Assets<Panned>>()
        .get(&handle)
        .unwrap();
    let samples: Vec<_> = source.decoder().take(48).collect();
    assert_eq!(samples, vec![0.0; 48]);
}

#[test]
fn pcm_width_selects_original_gain_and_signed_rounding_before_stereo_output() {
    use cyrus_audio::sound::{Panned, PannedGains};
    use rgpre::import::sfx::AudioType;

    for (kind, samples, expected) in [
        (
            AudioType::Mono8,
            vec![192, 64, 129, 127],
            vec![14464, -14464, 226, -226],
        ),
        (
            AudioType::Stereo8,
            vec![192, 192, 64, 64, 129, 129, 127, 127],
            vec![14464, -14464, 226, -226],
        ),
        (
            AudioType::Mono16,
            [16384i16, -16384, 257, -257]
                .into_iter()
                .flat_map(i16::to_le_bytes)
                .collect(),
            vec![14336, -14336, 224, -225],
        ),
        (
            AudioType::Stereo16,
            [16384i16, 16384, -16384, -16384, 257, 257, -257, -257]
                .into_iter()
                .flat_map(i16::to_le_bytes)
                .collect(),
            vec![14336, -14336, 224, -225],
        ),
    ] {
        let source = Panned::new(
            AudioSource {
                bytes: pcm_wav(kind, 22050, &samples).unwrap().into(),
            },
            PannedGains::new(kind.bits_per_sample().try_into().unwrap(), 112, 256, 64),
        )
        .unwrap();
        let actual: Vec<_> = source
            .decoder()
            .map(|sample| (sample * 32768.0) as i32)
            .collect();
        let expected: Vec<_> = expected
            .into_iter()
            .flat_map(|sample| [sample, sample])
            .collect();
        assert_eq!(actual, expected, "{kind:?}");
    }
}

#[test]
fn scalar_pan_rounds_the_coefficient_before_multiplying_each_sample() {
    use cyrus_audio::sound::{Panned, PannedGains, PcmWidth};
    use rgpre::import::sfx::AudioType;
    let pcm: Vec<_> = [16384i16, -257]
        .into_iter()
        .flat_map(i16::to_le_bytes)
        .collect();
    let source = Panned::new(
        AudioSource {
            bytes: pcm_wav(AudioType::Mono16, 22050, &pcm).unwrap().into(),
        },
        PannedGains::new(PcmWidth::Sixteen, 112, 256, 32),
    )
    .unwrap();
    // Original pan-table values 128 and64 produce integer coefficients112 and56.
    assert_eq!(
        source.decoder().collect::<Vec<_>>(),
        vec![
            14336.0 / 32768.0,
            7168.0 / 32768.0,
            -225.0 / 32768.0,
            -113.0 / 32768.0
        ]
    );
    assert_eq!(output_gain(PcmWidth::Sixteen, 127, 256), 1.0);
    assert_eq!(output_gain(PcmWidth::Sixteen, 112, 256), 112.0 / 128.0);
    assert_eq!(output_gain(PcmWidth::Eight, 112, 256), 113.0 / 128.0);
    assert!(PcmWidth::try_from(24).is_err());
}
