//! Script binding and audio command contracts.
use super::*;

#[test]
fn speech_command_admits_uppercase_installed_labels() {
    let mut app = app();
    app.init_resource::<cyrus_audio::sound::Speech>()
        .add_message::<cyrus_audio::sound::SoundRequest>();
    app.world_mut()
        .resource_mut::<cyrus_audio::sound::Speech>()
        .0
        .insert(
            "cisw".into(),
            cyrus_assets::sound::VoiceClip {
                audio: Handle::default(),
                bits_per_sample: 8,
                base_volume: 127,
                seconds: 2.5,
                text: "description".into(),
            },
        );
    let result = run_commands(
        &mut app,
        vec![Command::PlaySpeech {
            label: "CISW".into(),
        }],
    );
    assert_eq!(result[0]["label"], "CISW");
    let requests = app
        .world_mut()
        .resource_mut::<Messages<cyrus_audio::sound::SoundRequest>>()
        .drain()
        .collect::<Vec<_>>();
    assert!(
        matches!(requests.as_slice(), [cyrus_audio::sound::SoundRequest::Speech(label)] if label == "CISW")
    );
}

#[test]
fn capture_readiness_requires_the_current_world_to_finish_spawning() {
    let mut app = script_app();
    app.insert_state(cyrus_game::AppState::InGame);
    let state = run_commands(&mut app, vec![Command::State]).remove(0);
    assert_eq!(state["state"], "InGame");
    assert_eq!(state["scene_ready"], false);
    // The engine generator's state is read-only diagnostics for replay determinism checks.
    assert_eq!(state["random_state"], 1);

    let current = app
        .world()
        .resource::<cyrus_world::lifecycle::PreparedWorld>()
        .animations
        .clone();
    app.insert_resource(cyrus_render::ReadyScene(current));
    let state = run_commands(&mut app, vec![Command::State]).remove(0);
    assert_eq!(state["scene_ready"], true);

    app.world_mut()
        .resource_mut::<cyrus_world::lifecycle::PreparedWorld>()
        .animations = default();
    let state = run_commands(&mut app, vec![Command::State]).remove(0);
    assert_eq!(state["scene_ready"], false);
}

#[test]
fn cheat_command_supplies_player_distance_state_to_script_steps() {
    use cyrus_game::cheats::{CheatStore, Cheats};
    struct Memory;
    impl CheatStore for Memory {
        fn load(&self) -> anyhow::Result<cyrus_assets::cheats::CheatFile> {
            Ok(cyrus_assets::cheats::empty())
        }
        fn save(&self, _: &cyrus_assets::cheats::CheatFile) -> anyhow::Result<()> {
            Ok(())
        }
    }
    let mut app = app();
    let dialogue = json!({"menu_start":[35,25],"max_menu_items":300,"max_dialog_lines":20,"max_text_width":500,"menu_traverse_delay":4,"print_text":true,"use_speech":true,"max_distance":1600,"rtx_filename":"ENGLISH.RTX"});
    let mut script_json = json!({
        "version":30,"dialogue":dialogue,"player_distance_override":false,"updates":{"fall_death_ticks":144,"task_system":true,"animation_system":true,"disable_slaves":false,"disable_master_slaves":false,"dummy_model_manager":false},
        "clock":{"normal_frame_rate":12,"min_frame_rate":6,"max_frame_rate":300,"use_smooth_fps":true,"use_smooth_divisor":true},
        "actor_pool_capacity":10,"content_id":"","world":17,
        "definition":{"functions":[
            {"name":"NullFunction","param_count":0,"kind":"function"},
            {"name":"PlayerDistance","param_count":0,"kind":"function"}
        ],"flags":[],"references":[],"attributes":[]},
        "actors":{"ACTOR":{"header_index":0,"code_origin":0,"instances":1,
            "namespace":"ACTOR","instance_variables":[[0]],"location_offsets":[],"attributes":vec![0u8; 256],"native_hook":null,
            "program":{"version":4,"entry":0,"variables":[0],"steps":{
                "0":{"next":1,"operation":{"type":"assign","destination":{"type":"local","index":0},
                    "formula":{"first":{"type":"call","call":{"function_id":1,"kind":"function","target":{"type":"me"},"function":"PlayerDistance","arguments":[]}},"rest":[]}}},
                "1":{"next":2,"operation":{"type":"end","target":0}}
            }}
        }},
        "instances":[{"object_id":0,"actor":"ACTOR","object_type":0,"active":0,"model_name":"","model_mode":0,"light_intensity":0,"light_radius":0,"light_rgb":[0,0,0],"map_position":[0,0,0],"angles":[0,0,0]}],"unsupported":{}
    });
    script_json["weapons"] = json!({});
    script_json["combat_attacks"] = json!([]);
    script_json["combat_clank_effects"] = json!([null, null]);
    script_json["combat_defenses"] = json!([]);
    let mut scripts: cyrus_assets::script::Scripts = serde_json::from_value(script_json).unwrap();
    scripts.content_id = scripts.content_identity().unwrap();
    let mut actors = cyrus_script::script::ScriptActors::default();
    actors.current = Some(cyrus_script::script::ScriptWorld::prepare(&scripts, 1).unwrap());
    app.insert_resource(actors)
        .insert_resource(Cheats::new(Memory));
    let step = |stage| Command::StepScript {
        generation: 1,
        index: 0,
        stage,
        frame_scale: Some(256),
        nominal_ticks: Some(0),
        distance_override: None,
        fallback_position: None,
    };
    let responses = run_commands(
        &mut app,
        vec![
            Command::Cheat {
                name: "neversaydie".into(),
                value: Some(7),
            },
            step(cyrus_control::ScriptStage::PrepareSpatial),
            step(cyrus_control::ScriptStage::Main),
            Command::State,
        ],
    );
    assert_eq!(responses[0]["effect"], "player_distance");
    assert_eq!(responses[2]["actors"][0]["locals"], json!([9_999_999]));
    assert_eq!(responses[3]["cheats"]["ready"], true);
    assert_eq!(responses[3]["cheats"]["entries"][10]["value"], 7);
}

#[test]
fn node_less_script_sound_is_captured_without_poisoning_the_actor() {
    let mut app = script_app();
    let responses = run_commands(&mut app, bind_and_step(None));
    assert_eq!(
        responses[1]["captured_sounds"],
        json!([{
            "actor":{"generation":1,"index":0},
            "sound":{"type":"play","effect":-17,"rate_offset_hz":-64}
        }])
    );
    let response = run_commands(
        &mut app,
        vec![Command::StepScript {
            generation: 1,
            index: 0,
            stage: cyrus_control::ScriptStage::Tasks,
            frame_scale: Some(256),
            nominal_ticks: Some(0),
            distance_override: None,
            fallback_position: None,
        }],
    );
    assert_eq!(response[0]["captured_sounds"], json!([]));
    let response = run_commands(
        &mut app,
        vec![Command::StepScript {
            generation: 1,
            index: 0,
            stage: cyrus_control::ScriptStage::Animation,
            frame_scale: Some(256),
            nominal_ticks: Some(0),
            distance_override: None,
            fallback_position: None,
        }],
    );
    assert_eq!(
        response[0]["captured_sounds"],
        json!([{
            "actor":{"generation":1,"index":0},
            "sound":{"type":"check_sample"}
        }])
    );
    let actors = app.world().resource::<cyrus_script::script::ScriptActors>();
    assert!(
        actors.current.as_ref().unwrap().actors()[0]
            .failure()
            .is_none()
    );
}

#[test]
fn script_binding_routes_sound_in_the_same_command_drain() {
    use cyrus_world::{animation::AnimationSound, sound::SoundCue};
    let mut app = script_app();
    app.add_message::<SoundCue>();
    let source = app
        .world()
        .resource::<cyrus_world::lifecycle::PreparedWorld>()
        .animations
        .clone();
    let entity = app
        .world_mut()
        .spawn((
            Name::new("actor-node"),
            cyrus_script::script::ScriptBinding {
                actor: cyrus_script::script::ScriptActorId {
                    generation: 0,
                    index: 0,
                },
                source,
            },
            cyrus_world::animation::AnimationPose {
                model: "OLD".into(),
                frame: 9,
            },
        ))
        .id();
    let responses = run_commands(&mut app, bind_and_step(Some("actor-node")));
    assert_eq!(responses[1]["captured_sounds"], json!([]));
    assert!(
        app.world()
            .get::<cyrus_world::animation::AnimationPose>(entity)
            .is_none()
    );
    let cues: Vec<_> = app
        .world_mut()
        .resource_mut::<Messages<SoundCue>>()
        .drain()
        .collect();
    assert_eq!(cues.len(), 1);
    assert!(matches!(cues[0], SoundCue::Animation {
            actor,
            sound: AnimationSound::Play { effect: -17, rate_offset_hz: -64 }, position: None, surface: Some(Surface::UNKNOWN)
        } if actor == entity));
    assert_eq!(
        app.world()
            .get::<cyrus_script::script::ScriptBinding>(entity)
            .unwrap()
            .actor
            .index,
        0
    );
}

#[test]
fn animation_sound_uses_unnamed_player_bindings() {
    use cyrus_world::{animation::AnimationSound, sound::SoundCue};
    let mut app = script_app();
    app.add_message::<SoundCue>();
    let entity = app.world_mut().spawn(Name::new("player")).id();
    let mut commands = bind_and_step(Some("player"));
    let step = commands.pop().unwrap();
    run_commands(&mut app, commands);
    app.world_mut().entity_mut(entity).remove::<Name>();
    let responses = run_commands(&mut app, vec![step]);
    assert_eq!(responses[0]["captured_sounds"], json!([]));
    let cues: Vec<_> = app
        .world_mut()
        .resource_mut::<Messages<SoundCue>>()
        .drain()
        .collect();
    assert_eq!(cues.len(), 1);
    assert!(matches!(cues[0], SoundCue::Animation {
            actor, sound: AnimationSound::Play { effect: -17, rate_offset_hz: -64 }, position: None, surface: Some(Surface::UNKNOWN)
        } if actor == entity));
}
