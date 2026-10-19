//! Presentation observes explicit logical stages without advancing scripts itself.
use bevy::{asset::AssetApp, prelude::*};
use cyrus_assets::{animation::Animations, script::Scripts};
use cyrus_game::script::{
    ScriptActors, ScriptBinding, ScriptPlugin, ScriptScene, ScriptStage, Thread,
};
use cyrus_world::{
    animation::{
        AnimationPlayback, AnimationPlugin, AnimationPose, AnimationRequest, AnimationTick,
    },
    lifecycle::PreparedWorld,
    sound::{SoundCue, SoundSurface},
};
use serde_json::json;

#[test]
fn scene_binding_follows_actor_stage_and_expires_on_same_world_reload() {
    let instances = json!([{"object_id":0,"actor":"ACTOR","object_type":1,"active":0,"model_name":"test","model_mode":2,"light_intensity":0,"light_radius":0,"light_rgb":[0,0,0],"map_position":[0,0,0],"angles":[0,0,0]}, {"object_id":1,"actor":"ACTOR","object_type":2,"active":0,"model_name":"test","model_mode":2,"light_intensity":0,"light_radius":0,"light_rgb":[0,0,0],"map_position":[0,0,0],"angles":[0,0,0]}]);
    let mut script_json = json!({
        "version":18,"combat_attacks":[],"dialogue":{"menu_start":[35,25],"max_menu_items":300,"max_dialog_lines":20,"max_text_width":500,"menu_traverse_delay":4,"print_text":true,"use_speech":true,"max_distance":1600,"rtx_filename":"ENGLISH.RTX"},"player_distance_override":false,"updates":{"task_system":true,"animation_system":true,"disable_slaves":false,"disable_master_slaves":false},"clock":{"normal_frame_rate":12,"min_frame_rate":6,"max_frame_rate":300,"use_smooth_fps":true,"use_smooth_divisor":true},"actor_pool_capacity":10,"content_id":"","world":17,
        "definition":{"functions":[
            {"name":"NullFunction","param_count":0,"kind":"function"},
            {"name":"PushAnimation","param_count":2,"kind":"task"}
        ],"flags":[],"references":[],"attributes":[]},
        "actors":{"ACTOR":{"header_index":0,"code_origin":0,"instances":2,
            "namespace":"ACTOR","instance_variables":[[],[]],"location_offsets":[],"attributes":{},"native_hook":null,
            "program":{"version":4,"entry":0,"steps":{
                "0":{"next":1,"operation":{"type":"call","call":{
                    "function_id":1,"kind":"task","target":{"type":"me"},
                    "function":"PushAnimation","arguments":[{"type":"integer","value":7},{"type":"integer","value":1}]
                }}},
                "1":{"next":2,"operation":{"type":"end","target":0}}
            }}
        }},
        "instances": instances,
        "unsupported":{}
    });
    script_json["combat_defenses"] = json!([]);
    let mut scripts: Scripts = serde_json::from_value(script_json).unwrap();
    scripts.content_id = scripts.content_identity().unwrap();
    let data: Animations = serde_json::from_value(json!({
        "version":2,"world":17,"sections":{},"unsupported":{},
        "models":{"TEST":{"base_positions":[[0,0,0]],"base_normals":[],"faces":[],"encoding":"animated_i32",
            "positions":[[[0,0,0]],[[0,1,0]]],"normals":[[],[]]}},
        "metadata":{"markers": [], "walk_node_maps": [], "script_content_id":scripts.content_id,
        "mpob_objects":[{"script_name":"ACTOR","object_type":1,"is_static":2,"model_name":"test"},{"script_name":"ACTOR","object_type":2,"is_static":2,"model_name":"test"}],"actors":[{
            "script_name":"ACTOR",
            "animation_groups":[
                {"group_index":9,"anim_type":0,"anim_id":1,"commands":[{"opcode":4,"sound_param":200,"volume_shift":0},{"opcode":1}]},
                {"group_index":1,"anim_type":0,"anim_id":1,"commands":[{"opcode":0,"frame_reference":0},{"opcode":2,"value":0}]},
                {"group_index":7,"anim_type":0,"anim_id":1,"commands":[{"opcode":0,"frame_reference":1},{"opcode":2,"value":0},{"opcode":1}]}
            ]
        }]}
    })).unwrap();
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_asset::<Scripts>()
        .init_asset::<Animations>()
        .add_message::<AnimationTick>()
        .add_message::<SoundCue>()
        .add_plugins((AnimationPlugin, ScriptPlugin));
    let script_handle = app
        .world_mut()
        .resource_mut::<Assets<Scripts>>()
        .add(scripts);
    let animation_handle = app
        .world_mut()
        .resource_mut::<Assets<Animations>>()
        .add(data);
    let prepared = || PreparedWorld {
        requested_name: "TEST".into(),
        world: Some(17),
        scripts: script_handle.clone(),
        animations: animation_handle.clone(),
        player_path: "test.player".into(),
        scene_path: "test.glb".into(),
        environment_path: "test.environment".into(),
    };
    app.world_mut()
        .resource_mut::<ScriptActors>()
        .begin_new_game();
    app.insert_resource(prepared());
    app.insert_resource(AnimationRequest(animation_handle.clone()));
    app.update();
    assert_eq!(
        app.world()
            .resource::<ScriptActors>()
            .current
            .as_ref()
            .unwrap()
            .globals()
            .lockout(),
        Some(0)
    );
    let id = app
        .world()
        .resource::<ScriptActors>()
        .current
        .as_ref()
        .unwrap()
        .id(0)
        .unwrap();
    app.world_mut()
        .resource_scope(|world, mut actors: Mut<ScriptActors>| {
            let data = world
                .resource::<Assets<Animations>>()
                .get(&animation_handle)
                .unwrap();
            actors
                .current
                .as_mut()
                .unwrap()
                .bind_animation(id, data, 1)
                .unwrap();
        });
    app.world_mut()
        .resource_mut::<ScriptActors>()
        .current
        .as_mut()
        .unwrap()
        .set_actor_surface(id, 7)
        .unwrap();
    let root = app
        .world_mut()
        .spawn(ScriptScene::new(animation_handle.clone()))
        .id();
    let unrelated = app.world_mut().spawn(Name::new("B_000_ACTOR")).id();
    let wrong_actor = app
        .world_mut()
        .spawn((Name::new("B_000_OTHER"), ChildOf(root)))
        .id();
    let node = app
        .world_mut()
        .spawn((
            Name::new("B_000_ACTOR"),
            ChildOf(root),
            AnimationPose {
                model: "PREVIOUS".into(),
                frame: 42,
            },
        ))
        .id();
    let unknown = app
        .world_mut()
        .spawn((Name::new("B_001_ACTOR"), ChildOf(root), SoundSurface(11)))
        .id();
    let playback = AnimationPlayback::new(
        animation_handle.clone(),
        app.world()
            .resource::<Assets<Animations>>()
            .get(&animation_handle)
            .unwrap(),
        "ACTOR",
        9,
        1,
    )
    .unwrap();
    app.world_mut().entity_mut(node).insert(playback);
    app.world_mut().write_message(AnimationTick { ticks: 0 });
    app.update();
    assert!(app.world().get::<ScriptBinding>(unknown).is_some());
    assert!(
        app.world().get::<SoundSurface>(unknown).is_none(),
        "unsupported object state must clear a stale component"
    );
    let cues: Vec<_> = app
        .world_mut()
        .resource_mut::<Messages<SoundCue>>()
        .drain()
        .collect();
    assert!(cues.iter().any(|cue| matches!(cue, SoundCue::Animation {
        actor, position: None, surface: Some(7), sound: cyrus_world::animation::AnimationSound::Play { effect: 200, .. }
    } if *actor == node)), "binding and surface projection must precede playback in the same update");
    assert_eq!(app.world().get::<ScriptBinding>(node).unwrap().actor, id);
    assert_eq!(
        app.world().get::<SoundSurface>(node),
        Some(&SoundSurface(7))
    );
    assert!(app.world().get::<ScriptBinding>(unrelated).is_none());
    assert!(app.world().get::<ScriptBinding>(wrong_actor).is_none());
    assert!(app.world().get::<AnimationPose>(node).is_none());
    app.world_mut()
        .resource_mut::<ScriptActors>()
        .current
        .as_mut()
        .unwrap()
        .advance(
            id,
            ScriptStage::Body(Thread::Main),
            cyrus_game::script::ScriptInputs {
                nominal_ticks: Some(0),
                frame_scale: Some(256),
                ..Default::default()
            },
            None,
            None,
            10,
        )
        .unwrap();
    app.update();
    let pose = app.world().get::<AnimationPose>(node).unwrap();
    assert_eq!(pose.model, "TEST");
    assert_eq!(pose.frame, 1);
    app.update();
    let actors = app.world().resource::<ScriptActors>();
    let runtime = actors.current.as_ref().unwrap().actors()[0]
        .runtime()
        .unwrap();
    assert_eq!(runtime.program_counter(Thread::Main), Some(1));
    assert_eq!(runtime.active_tasks(), 1);
    assert_eq!(app.world().get::<AnimationPose>(node).unwrap().frame, 1);

    app.world_mut()
        .resource_mut::<ScriptActors>()
        .current
        .as_mut()
        .unwrap()
        .set_actor_surface(id, 8)
        .unwrap();
    app.update();
    assert_eq!(
        app.world().get::<SoundSurface>(node),
        Some(&SoundSurface(8))
    );

    let model = app
        .world_mut()
        .resource_mut::<Assets<Animations>>()
        .get_mut(&animation_handle)
        .unwrap()
        .models
        .remove("TEST")
        .unwrap();
    app.update();
    assert!(app.world().get::<ScriptBinding>(node).is_none());
    assert!(app.world().get::<SoundSurface>(node).is_none());
    app.world_mut()
        .resource_mut::<Assets<Animations>>()
        .get_mut(&animation_handle)
        .unwrap()
        .models
        .insert("TEST".into(), model);
    app.update();
    assert!(app.world().get::<ScriptBinding>(node).is_none());
    assert!(app.world().get::<SoundSurface>(node).is_none());
    assert!(app.world().get::<AnimationPose>(node).is_none());

    app.world_mut()
        .resource_mut::<ScriptActors>()
        .current
        .as_mut()
        .unwrap()
        .globals_mut()
        .remember_choice("Remembered topic".into());
    app.insert_resource(prepared());
    app.update();
    let registry = app.world().resource::<ScriptActors>();
    assert!(registry.has_pending_revisit());
    assert_ne!(
        registry.current.as_ref().unwrap().id(0).unwrap().generation,
        id.generation
    );
    assert!(app.world().get::<ScriptBinding>(node).is_none());
    assert!(app.world().get::<SoundSurface>(node).is_none());
    assert!(app.world().get::<AnimationPose>(node).is_none());
    app.world_mut()
        .resource_mut::<ScriptActors>()
        .begin_new_game();
    app.insert_resource(prepared());
    app.update();
    assert_eq!(
        app.world()
            .resource::<ScriptActors>()
            .current
            .as_ref()
            .unwrap()
            .globals()
            .highlighted_choice(),
        Some("")
    );
    assert!(app.world().get::<ScriptBinding>(node).is_none());
    assert!(app.world().get::<SoundSurface>(node).is_none());
    assert!(app.world().get::<AnimationPose>(node).is_none());
    let fresh = app
        .world()
        .resource::<ScriptActors>()
        .current
        .as_ref()
        .unwrap()
        .id(0)
        .unwrap();
    assert_ne!(fresh.generation, id.generation);
    let late_old_node = app
        .world_mut()
        .spawn((Name::new("B_000_ACTOR"), ChildOf(root)))
        .id();
    app.update();
    assert!(app.world().get::<ScriptBinding>(node).is_none());
    assert!(app.world().get::<SoundSurface>(node).is_none());
    assert!(app.world().get::<ScriptBinding>(late_old_node).is_none());
    let runtime = app
        .world()
        .resource::<ScriptActors>()
        .current
        .as_ref()
        .unwrap()
        .actors()[0]
        .runtime()
        .unwrap();
    assert_eq!(runtime.program_counter(Thread::Main), Some(0));
    assert_eq!(runtime.active_tasks(), 0);
    app.world_mut()
        .resource_mut::<ScriptActors>()
        .begin_new_game();
    app.insert_resource(prepared());
    app.update();
    let registry = app.world().resource::<ScriptActors>();
    let globals = registry.current.as_ref().unwrap().globals();
    assert_eq!(globals.lockout(), Some(0));
    assert_eq!(globals.highlighted_choice(), Some(""));
}
