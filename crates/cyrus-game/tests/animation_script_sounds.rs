//! Script-request cues share the ordinary actor's sound handler and tick input.
use cyrus_game::animation::{AnimationSound, Animations};
use serde_json::json;

fn asset() -> Animations {
    Animations {
        version: 2,
        world: 17,
        metadata: json!({
            "actors":[{"script_name":"ACTOR","animation_groups":[
                {"group_index":0,"anim_type":0,"anim_id":1,"commands":[
                    {"opcode":0,"frame_reference":9},{"opcode":2,"value":0}]},
                {"group_index":7,"anim_type":0,"anim_id":1,"commands":[
                    {"opcode":4,"sound_param":-17,"volume_shift":-1},
                    {"opcode":0,"frame_reference":1},{"opcode":0,"frame_reference":2},
                    {"opcode":1}]}
            ]}],
            "animation_scripts":{"ACTOR":{"version":1,"entry":0,"steps":{
                "0":{"next":14,"operation":{"type":"task","function":"PushAnimation","arguments":[7,1]}},
                "14":{"next":19,"operation":{"type":"end","target":0}}
            }}}
        }),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    }
}

#[test]
fn script_requests_dispatch_authored_cues_before_the_immediate_pose() {
    let data = asset();
    let mut actor = data.actor_animation("ACTOR", 0).unwrap();
    let mut script = data.animation_script("ACTOR").unwrap();
    actor.request(0, 0, 1).unwrap();
    let mut events = Vec::new();
    let mut sounds = |event| {
        events.push(event);
        Ok(())
    };
    assert_eq!(
        script
            .advance_with_sounds(&mut actor, 0, &mut sounds)
            .unwrap(),
        Some(1)
    );
    assert_eq!(
        script
            .advance_with_sounds(&mut actor, 0, &mut sounds)
            .unwrap(),
        None
    );
    assert_eq!(actor.advance_with_sounds(0, &mut sounds).unwrap(), Some(1));
    assert_eq!(actor.advance_with_sounds(1, &mut sounds).unwrap(), Some(1));
    assert_eq!(actor.advance_with_sounds(1, &mut sounds).unwrap(), Some(2));
    assert_eq!(actor.advance_with_sounds(1, &mut sounds).unwrap(), Some(9));
    assert_eq!(
        script
            .advance_with_sounds(&mut actor, 1, &mut sounds)
            .unwrap(),
        None
    );
    assert_eq!(script.program_counter(), 0);
    assert_eq!(
        script
            .advance_with_sounds(&mut actor, 0, &mut sounds)
            .unwrap(),
        Some(1)
    );
    assert_eq!(
        events,
        [
            AnimationSound::Play {
                effect: -17,
                rate_offset_hz: -64
            },
            AnimationSound::Stop,
            AnimationSound::Play {
                effect: -17,
                rate_offset_hz: -64
            },
        ]
    );
}

#[test]
fn script_sound_failures_keep_context_and_do_not_retry_the_cue() {
    let data = asset();
    let mut actor = data.actor_animation("ACTOR", 0).unwrap();
    let mut script = data.animation_script("ACTOR").unwrap();
    let mut calls = 0;
    let mut sounds = |_| {
        calls += 1;
        anyhow::bail!("actor surface unavailable")
    };
    let error = script
        .advance_with_sounds(&mut actor, 0, &mut sounds)
        .unwrap_err()
        .to_string();
    assert!(error.contains("world 17 actor ACTOR script instruction 0"));
    assert!(error.contains("actor surface unavailable"));
    assert_eq!(
        script
            .advance_with_sounds(&mut actor, 1, &mut sounds)
            .unwrap_err()
            .to_string(),
        error
    );
    assert_eq!(calls, 1);
}

#[test]
fn forced_replacement_forgets_the_prior_sound_before_its_immediate_cue() {
    let data = asset();
    let mut actor = data.actor_animation("ACTOR", 0).unwrap();
    let mut events = Vec::new();
    let mut sounds = |event| {
        events.push(event);
        Ok(())
    };
    assert_eq!(
        actor
            .force_request_with_sounds(7, 0, 0, &mut sounds)
            .unwrap(),
        Some(1)
    );
    assert_eq!(
        actor
            .force_request_with_sounds(7, 0, 0, &mut sounds)
            .unwrap(),
        None
    );
    assert_eq!(
        actor
            .force_request_with_sounds(0, 0, 0, &mut sounds)
            .unwrap(),
        Some(9)
    );
    assert_eq!(
        actor
            .force_request_with_sounds(7, 0, 0, &mut sounds)
            .unwrap(),
        Some(1)
    );
    assert_eq!(
        events,
        [
            AnimationSound::Play {
                effect: -17,
                rate_offset_hz: -64
            },
            AnimationSound::Forget,
            AnimationSound::Play {
                effect: -17,
                rate_offset_hz: -64
            },
        ]
    );
}

#[test]
fn group_and_scripted_playback_start_and_stop_sound_in_the_pose_update() {
    use bevy::{audio::AudioSource, prelude::*};
    use cyrus_game::{
        animation::{
            AnimationPlayback, AnimationPlugin, AnimationPose, AnimationRequest, AnimationTick,
        },
        install::GameInstall,
        player::ViewCamera,
        sound::{
            Effect, Panned, PlayingSound, SoundEffects, SoundPlugin, SoundSettings, SoundSurface,
            pcm_wav,
        },
    };
    use rgpre::import::sfx::AudioType;

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../redguard");
    let Ok(install) = GameInstall::locate(Some(&root)) else {
        eprintln!("skipped: no install (animation runtime sound)");
        return;
    };
    for scripted in [false, true] {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            AnimationPlugin,
            SoundPlugin,
        ))
        .init_asset::<AudioSource>()
        .init_asset::<Panned>()
        .insert_resource(install.clone());
        app.update();
        let audio = app
            .world_mut()
            .resource_mut::<Assets<AudioSource>>()
            .add(AudioSource {
                bytes: pcm_wav(AudioType::Mono8, 11025, &[128; 64]).unwrap().into(),
            });
        app.insert_resource(SoundSettings::parse("[system]\nsound_distance=64\n"));
        app.insert_resource(SoundEffects(vec![Some(Effect {
            audio,
            looping: true,
            base_volume: 64,
            sample_rate: 11025,
        })]));
        app.world_mut()
            .spawn((ViewCamera, GlobalTransform::default()));
        let mut data = asset();
        data.metadata["actors"][0]["animation_groups"][0]["commands"][0]["frame_reference"] =
            json!(0);
        data.metadata["actors"][0]["animation_groups"][1]["commands"][0]["sound_param"] = json!(0);
        data.metadata["actors"][0]["raan_entries"] =
            json!([{"file_path":"test.3dc","frame_count":3}]);
        data.models.insert(
            "TEST".into(),
            serde_json::from_value(json!({
                "base_positions":[[0,0,0]], "base_normals":[], "faces":[],
                "encoding":"animated_i32", "positions":[[[0,0,0]],[[0,0,1]],[[0,0,2]]],
                "normals":[[],[],[]]
            }))
            .unwrap(),
        );
        let source = app
            .world_mut()
            .resource_mut::<Assets<Animations>>()
            .add(data);
        let data = app
            .world()
            .resource::<Assets<Animations>>()
            .get(&source)
            .unwrap();
        let playback = if scripted {
            AnimationPlayback::new_scripted(source.clone(), data, "ACTOR", 0).unwrap()
        } else {
            AnimationPlayback::new(source.clone(), data, "ACTOR", 7, 1).unwrap()
        };
        app.world_mut().resource_mut::<AnimationRequest>().0 = source;
        let actor = app
            .world_mut()
            .spawn((
                playback,
                GlobalTransform::from_translation(Vec3::Z),
                SoundSurface(0),
            ))
            .id();
        for (ticks, frame, sounds) in [
            (0, 1, 1),
            (0, 1, 1),
            (1, 1, 1),
            (1, 2, 1),
            (1, if scripted { 0 } else { 2 }, 0),
        ] {
            app.world_mut().write_message(AnimationTick { ticks });
            app.update();
            assert_eq!(
                app.world().get::<AnimationPose>(actor).unwrap().frame,
                frame
            );
            let mut playing = app
                .world_mut()
                .query_filtered::<Entity, With<PlayingSound>>();
            assert_eq!(
                playing.iter(app.world()).count(),
                sounds,
                "scripted={scripted} tick={ticks}"
            );
        }
        assert_eq!(
            app.world().get::<AnimationPlayback>(actor).is_some(),
            scripted
        );
        if scripted {
            app.world_mut().write_message(AnimationTick { ticks: 0 });
            app.update();
            assert_eq!(app.world().get::<AnimationPose>(actor).unwrap().frame, 1);
            let mut playing = app
                .world_mut()
                .query_filtered::<Entity, With<PlayingSound>>();
            assert_eq!(playing.iter(app.world()).count(), 1);
        } else {
            let source = app.world().resource::<AnimationRequest>().0.clone();
            let data = app
                .world()
                .resource::<Assets<Animations>>()
                .get(&source)
                .unwrap();
            let playback = AnimationPlayback::new(source.clone(), data, "ACTOR", 7, 1).unwrap();
            app.world_mut().entity_mut(actor).insert(playback);
            for ticks in [0, 1, 1, 1] {
                app.world_mut().write_message(AnimationTick { ticks });
            }
            app.update();
            assert_eq!(app.world().get::<AnimationPose>(actor).unwrap().frame, 2);
            let mut playing = app
                .world_mut()
                .query_filtered::<Entity, With<PlayingSound>>();
            assert_eq!(
                playing.iter(app.world()).count(),
                0,
                "same-update End stops the queued start"
            );
        }
    }
}
