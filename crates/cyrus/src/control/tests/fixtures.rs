//! Shared native control app construction and bounded capture completion.
use super::*;

pub(super) fn app() -> App {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        bevy::state::app::StatesPlugin,
        bevy::input::InputPlugin,
    ))
    .init_state::<cyrus_game::AppState>()
    .init_resource::<Assets<cyrus_assets::animation::Animations>>()
    .init_resource::<Fidelity>()
    .init_resource::<cyrus_game::random::EngineRandom>()
    .init_resource::<cyrus_game::cheats::Cheats>()
    .add_plugins(cyrus_script::console::GameConsolePlugin)
    .init_resource::<CurrentScene>()
    .init_resource::<bevy::diagnostic::FrameCount>()
    .init_resource::<cyrus_script::player::input::PlayerInput>()
    .init_resource::<cyrus_render::camera::DetachedView>()
    .insert_resource(SceneRequest(String::new()))
    .insert_resource(ControlAddr("127.0.0.1:0".into()))
    .add_plugins(ControlPlugin);
    app
}

pub(super) fn script_app() -> App {
    script_app_with_player(false)
}

pub(super) fn script_app_with_player(player: bool) -> App {
    use cyrus_assets::{animation::Animations, script::Scripts};
    use cyrus_script::script::{ScriptActors, ScriptWorld};
    let mut app = app();
    let dialogue = json!({"menu_start":[35,25],"max_menu_items":300,"max_dialog_lines":20,"max_text_width":500,"menu_traverse_delay":4,"print_text":true,"use_speech":true,"max_distance":1600,"rtx_filename":"ENGLISH.RTX"});
    let mut script_json = json!({
        "version":30,"dialogue":dialogue,"player_distance_override":false,"updates":{"fall_death_ticks":144,"task_system":true,"animation_system":true,"disable_slaves":false,"disable_master_slaves":false,"dummy_model_manager":false},"clock":{"normal_frame_rate":12,"min_frame_rate":6,"max_frame_rate":300,"use_smooth_fps":true,"use_smooth_divisor":true},"actor_pool_capacity":10,"content_id":"","world":17,
        "definition":{"functions":[
            {"name":"NullFunction","param_count":0,"kind":"function"},
            {"name":"PushAnimation","param_count":2,"kind":"task"}
        ],"flags":[],"references":[],"attributes":[]},
        "actors":{"ACTOR":{"header_index":0,"code_origin":0,"instances":1,
            "namespace":"ACTOR","instance_variables":[[]],"location_offsets":[],"attributes":vec![0u8; 256],"native_hook":null,
            "program":{"version":4,"entry":0,"steps":{
                "0":{"next":1,"operation":{"type":"call","call":{
                    "function_id":1,"kind":"task","target":{"type":"me"},
                    "function":"PushAnimation","arguments":[{"type":"integer","value":7},{"type":"integer","value":1}]
                }}},
                "1":{"next":2,"operation":{"type":"end","target":0}}
            }}
        }},
        "instances":[{"object_id":0,"actor":"ACTOR","object_type":0,"active":0,"model_name":"","model_mode":0,"light_intensity":0,"light_radius":0,"light_rgb":[0,0,0],"map_position":[0,0,0],"angles":[0,0,0]}],
        "unsupported":{}
    });
    script_json["weapons"] = json!({});
    script_json["combat_attacks"] = json!([]);
    script_json["combat_clank_effects"] = json!([null, null]);
    script_json["combat_defenses"] = json!([]);
    let mut scripts: Scripts = serde_json::from_value(script_json).unwrap();
    if player {
        let mut actor = scripts.actors["ACTOR"].clone();
        actor.namespace = None;
        scripts.actors.insert("CYRUS".into(), actor);
    }
    scripts.content_id = scripts.content_identity().unwrap();
    let mut actors = ScriptActors::default();
    actors.current = Some(ScriptWorld::prepare(&scripts, 1).unwrap());
    let data: Animations = serde_json::from_value(json!({
            "version":2,"world":17,"sections":{},"models":{},"unsupported":{},
            "metadata":{"script_content_id":scripts.content_id,"actors":[{
                "script_name":"ACTOR","animation_groups":[
                    {"group_index":1,"anim_type":0,"anim_id":1,"commands":[{"opcode":0,"frame_reference":0},{"opcode":2,"value":0}]},
                    {"group_index":7,"anim_type":0,"anim_id":1,"commands":[
                        {"opcode":4,"sound_param":-17,"volume_shift":-1},
                        {"opcode":0,"frame_reference":1},{"opcode":2,"value":0},{"opcode":1}]}
                ]
            }]}
        })).unwrap();
    let animations = app
        .world_mut()
        .resource_mut::<Assets<Animations>>()
        .add(data);
    app.insert_resource(actors)
        .insert_resource(cyrus_world::lifecycle::PreparedWorld {
            requested_name: "TEST".into(),
            world: Some(17),
            animations,
            scripts: default(),
            player_path: "test.player".into(),
            scene_path: "test.glb".into(),
            environment_path: "test.environment".into(),
        });
    app
}

pub(super) fn frame_app() -> App {
    use cyrus_script::{clock::SimulationClock, script::ScriptActors};
    let mut app = script_app_with_player(true);
    app.world_mut()
        .spawn((Player, Transform::default(), GlobalTransform::default()));
    app.insert_resource(State::new(cyrus_game::AppState::InGame));
    let animations = app
        .world()
        .resource::<cyrus_world::lifecycle::PreparedWorld>()
        .animations
        .clone();
    app.insert_resource(cyrus_render::ReadyScene(animations));
    app.world_mut()
        .spawn((Window::default(), bevy::window::PrimaryWindow));
    {
        let mut actors = app.world_mut().resource_mut::<ScriptActors>();
        let world = actors.current.as_mut().unwrap();
        world.prepare_spatial().unwrap();
        world
            .initialize_player_spatial(world.selected_player().unwrap(), [0; 3], [0; 3])
            .unwrap();
    }
    let mut clock = SimulationClock::new(cyrus_assets::script::ClockSettings {
        normal_frame_rate: 12,
        min_frame_rate: 6,
        max_frame_rate: 300,
        use_smooth_fps: false,
        use_smooth_divisor: false,
    })
    .unwrap();
    clock.advance(0);
    app.insert_resource(clock);
    app
}

pub(super) fn request_frame(
    app: &mut App,
    path: &std::path::Path,
    pcm_path: Option<String>,
) -> std::sync::mpsc::Receiver<Response> {
    request_frame_at(app, path, pcm_path, None)
}

pub(super) fn request_frame_at(
    app: &mut App,
    path: &std::path::Path,
    pcm_path: Option<String>,
    boundary: Option<cyrus_control::FrameBoundary>,
) -> std::sync::mpsc::Receiver<Response> {
    let (tx, rx) = channel();
    let (reply, results) = channel();
    app.insert_resource(Inbox(Mutex::new(rx)));
    tx.send(Pending {
        request: Request {
            id: 42,
            cmd: Command::CaptureGameplayFrame {
                path: path.display().to_string(),
                pcm_path,
                boundary,
            },
        },
        reply,
    })
    .unwrap();
    app.update();
    results
}

pub(super) fn run_commands(app: &mut App, commands: Vec<Command>) -> Vec<serde_json::Value> {
    let (tx, rx) = channel();
    let (reply, results) = channel();
    app.insert_resource(Inbox(Mutex::new(rx)));
    let count = commands.len();
    for (id, cmd) in commands.into_iter().enumerate() {
        tx.send(Pending {
            request: Request { id: id as u64, cmd },
            reply: reply.clone(),
        })
        .unwrap();
    }
    app.update();
    (0..count)
        .map(|_| {
            let response = results.try_recv().unwrap();
            assert!(response.ok, "{:?}", response.error);
            response.result.unwrap()
        })
        .collect()
}

pub(super) fn bind_and_step(node: Option<&str>) -> Vec<Command> {
    vec![
        Command::BindScriptAnimation {
            generation: 1,
            index: 0,
            default_group: 1,
            node: node.map(str::to_owned),
        },
        Command::StepScript {
            generation: 1,
            index: 0,
            stage: cyrus_control::ScriptStage::Main,
            frame_scale: None,
            nominal_ticks: Some(0),
            distance_override: None,
            fallback_position: None,
        },
    ]
}
#[derive(Resource)]
pub(super) struct TraceOrdinary(pub(super) u32);
pub(super) fn trace_app() -> App {
    use cyrus_script::{
        clock::SimulationClock,
        script::{OrdinaryInputs, ScriptActors, ScriptInputs},
    };
    let mut app = script_app_with_player(true);
    {
        let mut actors = app.world_mut().resource_mut::<ScriptActors>();
        let world = actors.current.as_mut().unwrap();
        world.prepare_spatial().unwrap();
        let player = world.selected_player().unwrap();
        world
            .initialize_player_spatial(player, [0; 3], [0; 3])
            .unwrap();
    }
    let mut clock = SimulationClock::new(cyrus_assets::script::ClockSettings {
        normal_frame_rate: 12,
        min_frame_rate: 6,
        max_frame_rate: 300,
        use_smooth_fps: false,
        use_smooth_divisor: false,
    })
    .unwrap();
    clock.advance(0);
    app.insert_resource(clock);
    app.insert_resource(TraceOrdinary(1));
    app.add_systems(
        Update,
        |run: Res<TraceOrdinary>,
         mut actors: ResMut<ScriptActors>,
         mut clock: ResMut<SimulationClock>,
         animations: Res<Assets<cyrus_assets::animation::Animations>>,
         prepared: Res<cyrus_world::lifecycle::PreparedWorld>,
         mut random: ResMut<cyrus_game::random::EngineRandom>| {
            for _ in 0..run.0 {
                let world = actors.current.as_mut().unwrap();
                clock.advance((world.ordinary_updates() as u32 + 1) * 3);
                world
                    .update_ordinary(
                        animations.get(&prepared.animations).unwrap(),
                        [0; 2],
                        OrdinaryInputs {
                            scripts: ScriptInputs {
                                nominal_ticks: Some(clock.step().nominal_ticks),
                                frame_scale: Some(clock.step().frame_scale),
                                ..default()
                            },
                            scene: None,
                        },
                        &mut random,
                        &mut |_, _| Ok(()),
                        100,
                    )
                    .unwrap();
                world
                    .place_selected_player(
                        [
                            (10 + world.ordinary_updates() as i32) * 256,
                            20 * 256,
                            30 * 256,
                        ],
                        0,
                    )
                    .unwrap();
            }
        },
    );
    app
}
pub(super) fn finish_capture(
    app: &mut App,
    entity: Entity,
    size: u32,
    results: &std::sync::mpsc::Receiver<Response>,
) -> Response {
    use bevy::asset::RenderAssetUsages;
    use bevy::render::{
        render_resource::{Extent3d, TextureDimension, TextureFormat},
        view::screenshot::ScreenshotCaptured,
    };
    let image = Image::new_fill(
        Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[12, 34, 56, 255],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::MAIN_WORLD,
    );
    app.world_mut()
        .trigger(ScreenshotCaptured { entity, image });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        app.update();
        if let Ok(response) = results.try_recv() {
            return response;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "capture did not finish"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

pub(super) fn assert_trace_rows(rows: &[serde_json::Value]) {
    for (index, row) in rows.iter().enumerate() {
        assert_eq!(row["after_updates"], index);
        assert_eq!(row["update"], index);
        assert_eq!(
            row["player"]["spatial"]["local"]["position"],
            json!([(10 + index) * 256, 20 * 256, 30 * 256])
        );
        assert!(row["player"]["animation"].is_null() || row["player"]["animation"].is_object());
        assert_eq!(row["player"]["control"]["falling_state"], 0);
        assert_eq!(row["normal_frame_rate"], 12);
        if index > 0 {
            assert!(row["elapsed_scale"].as_u64() > rows[index - 1]["elapsed_scale"].as_u64());
            assert_eq!(
                row["clock_step"],
                json!({
                    "frame_scale": 256, "nominal_ticks": 1, "fine_ticks": 8
                })
            );
        } else {
            assert!(row["clock_step"].is_null());
        }
    }
}
