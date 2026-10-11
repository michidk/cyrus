//! Script-request cues share the ordinary actor's sound handler and tick input.
use cyrus_assets::animation::Animations;
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
fn group_and_scripted_playback_start_and_stop_sound_in_the_pose_update() {
    use bevy::{audio::AudioSource, prelude::*};
    use cyrus_assets::audio::pcm_wav;
    use cyrus_assets::install::GameInstall;
    use cyrus_assets::sound::SoundSettings;
    use cyrus_render::camera::ViewCamera;
    use cyrus_render::sound::Effect;
    use cyrus_render::sound::Panned;
    use cyrus_render::sound::PlayingSound;
    use cyrus_render::sound::SoundEffects;
    use cyrus_render::sound::SoundPlugin;
    use cyrus_world::animation::AnimationPlayback;
    use cyrus_world::animation::AnimationPlugin;
    use cyrus_world::animation::AnimationPose;
    use cyrus_world::animation::AnimationRequest;
    use cyrus_world::animation::AnimationTick;
    use cyrus_world::sound::SoundSurface;
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
            cyrus_render::animation::AnimationRenderPlugin,
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
