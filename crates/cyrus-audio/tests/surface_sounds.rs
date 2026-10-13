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
    app.world_mut().write_message(SoundCue::Animation {
        actor,
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
        actor,
        sound: AnimationSound::Stop,
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
