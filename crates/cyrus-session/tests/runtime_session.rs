//! Public composed runtime construction and late-rejection contracts.
#[path = "support/render_staging.rs"]
mod render_staging;
use anyhow::Result;
use bevy::{
    audio::{AudioPlayer, AudioSource, Decodable, Volume},
    prelude::*,
};
use cyrus_assets::{animation::Animations, script::*};
use cyrus_audio::sound::{Panned, RestoredDialogueAssets, SoundPlugin, dialogue::DialogueClip};
use cyrus_game::{
    clock::SimulationClock,
    player::{
        Player,
        data::{PlayerScene, PlayerSceneRequest},
        input::PlayerToggles,
        placement::{MarkerEntry, PlayerEntrySupport, PlayerPose},
        runtime::PlayerRuntime,
    },
    preferences::{PreferenceKey, PreferenceStore, Preferences},
    random::EngineRandom,
    script::{
        LogicalSessionRestoreInputs, ScriptActors, ScriptBinding, ScriptHud, ScriptWorld,
        WorldSnapshotSources,
    },
};
use cyrus_script::program::*;
use cyrus_session::{PlayerReplacement, RuntimeSessionSnapshot};
use serde_json::json;
use std::collections::BTreeMap;

fn scripts(operations: Vec<Operation>) -> Scripts {
    let actor = |name: &str, operations: Vec<Operation>| Actor {
        header_index: 0,
        code_origin: 0,
        instances: 1,
        namespace: Some(name.into()),
        instance_variables: vec![vec![]],
        location_offsets: vec![],
        attributes: BTreeMap::new(),
        combat_taunts: vec![],
        native_hook: None,
        program: Program {
            version: 4,
            entry: 0,
            variables: vec![],
            strings: vec!["OTHER".into()],
            steps: operations
                .into_iter()
                .enumerate()
                .map(|(index, operation)| {
                    (
                        index as u32,
                        Step {
                            next: index as u32 + 1,
                            operation,
                        },
                    )
                })
                .collect(),
        },
    };
    let mut result = Scripts {
        version: 22,
        weapons: Default::default(),
        camera: Default::default(),
        combat_attacks: vec![],
        combat_defenses: vec![],
        in_combat_threshold: None,
        non_engaged_spacing: None,
        combat_clank_effects: [None; 2],
        node_timer: None,
        dialogue: cyrus_assets::script::DialogueSettings {
            menu_start: [35, 25],
            max_menu_items: 300,
            max_dialog_lines: 20,
            max_text_width: 500,
            menu_traverse_delay: 4,
            print_text: true,
            use_speech: true,
            max_distance: 1600,
            rtx_filename: "ENGLISH.RTX".into(),
        },
        player_distance_override: false,
        updates: cyrus_assets::script::UpdateSettings {
            dummy_model_manager: false,
            task_system: true,
            animation_system: true,
            disable_slaves: false,
            disable_master_slaves: false,
        },
        clock: cyrus_assets::script::ClockSettings {
            normal_frame_rate: 12,
            min_frame_rate: 6,
            max_frame_rate: 300,
            use_smooth_fps: true,
            use_smooth_divisor: true,
        },
        actor_pool_capacity: 10,
        content_id: String::new(),
        world: 17,
        definition: Definition {
            functions: [
                ("NullFunction", 0, CallKind::Function),
                ("PushAnimation", 2, CallKind::Task),
                ("Wait", 1, CallKind::Task),
            ]
            .into_iter()
            .map(|(name, param_count, kind)| Function {
                name: name.into(),
                param_count,
                kind,
            })
            .collect(),
            flags: vec![Flag {
                name: "Ready".into(),
                flag_type: "NUMBER".into(),
                initial_value: 0,
            }],
            references: vec![],
            attributes: vec![],
        },
        actors: BTreeMap::from([
            ("ACTOR".into(), actor("ACTOR", operations)),
            (
                "OTHER".into(),
                actor("OTHER", vec![Operation::End { target: 0 }]),
            ),
        ]),
        instances: ["ACTOR", "OTHER"]
            .into_iter()
            .enumerate()
            .map(|(index, name)| Instance {
                object_id: index as u32,
                actor: name.into(),
                object_type: 0,
                active: 0,
                model_name: String::new(),
                model_mode: 0,
                light_intensity: 0,
                light_radius: 0,
                light_rgb: [0; 3],
                map_position: [0; 3],
                angles: [0; 3],
            })
            .collect(),
        unsupported: BTreeMap::new(),
    };
    result.actors.get_mut("OTHER").unwrap().code_origin = 100;
    result.content_id = result.content_identity().unwrap();
    result
}
fn hud_asset() -> cyrus_assets::hud::HudAsset {
    cyrus_assets::hud::HudAsset {
        labels: [
            ("TEST".into(), "A new journal entry.".into()),
            ("EMPT".into(), String::new()),
        ]
        .into(),
        images: vec![],
        candle_mode: 0,
        candle: [0; 2],
        candle_size: [0; 2],
        pickup: [0; 2],
        prompt: [0; 2],
        override_ticks: 0,
        compass: [0; 2],
        compass_size: [0; 2],
        world_compass: BTreeMap::new(),
        powerup_slots: [[0; 2]; 2],
        powerup_sizes: vec![[1; 2]; 3],
        logbook: [0; 2],
        logbook_size: [0; 2],
        walk_mode: false,
        auto_defend: false,
        notices: Default::default(),
        inventory: BTreeMap::new(),
        start_items: vec![],
        items: vec![],
        selected: None,
        bindings: [0; 15],
        font: Default::default(),
        text_alpha: [0; 2],
        player_text_alpha: [0; 2],
    }
}

struct Store;
impl PreferenceStore for Store {
    fn load(&self, _: PreferenceKey) -> Result<Option<String>> {
        Ok(None)
    }
    fn save(&self, _: PreferenceKey, _: &str) -> Result<()> {
        panic!("replacement must not persist")
    }
}

struct CheatStore;
impl cyrus_game::cheats::CheatStore for CheatStore {
    fn load(&self) -> Result<cyrus_assets::cheats::CheatFile> {
        Ok(cyrus_assets::cheats::empty())
    }
    fn save(&self, _: &cyrus_assets::cheats::CheatFile) -> Result<()> {
        panic!("session replacement must not write cheat storage")
    }
}

#[test]
fn runtime_session_prepares_all_owners_without_changing_live_resources() -> Result<()> {
    for from_menu in [false, true] {
        exercise_runtime_session(false, from_menu, false)?;
        exercise_runtime_session(true, from_menu, false)?;
    }
    Ok(())
}

#[test]
fn asynchronous_load_waits_for_assets_and_preserves_live_state_on_rejection() -> Result<()> {
    for from_menu in [false, true] {
        exercise_runtime_session(false, from_menu, true)?;
        exercise_runtime_session(true, from_menu, true)?;
    }
    Ok(())
}

fn exercise_runtime_session(with_camera: bool, from_menu: bool, via_load: bool) -> Result<()> {
    let mut scripts = scripts(vec![Operation::End { target: 0 }]);
    let mut player_script = scripts.actors["ACTOR"].clone();
    player_script.code_origin = 200;
    player_script.namespace = None;
    scripts.actors.insert("CYRUS".into(), player_script);
    scripts.content_id = scripts.content_identity()?;
    let animations: Animations = serde_json::from_value(json!({
        "version":2,"world":17,"metadata":{"script_content_id":scripts.content_id,"actors":[]},
        "sections":{},"models":{"TEST":{
            "base_positions":[[0.0,0.0,0.0]],"base_normals":[],"faces":[],
            "encoding":"animated_i16","positions":[[[0.0,0.0,0.0]],[[1.0,2.0,3.0]]],
            "normals":[[],[]]
        }},"unsupported":{}
    }))?;
    let scene_bytes = serde_json::to_vec(&json!({
        "version":2,"world":17,"settings":{"standing_height":-2,"post_collide_height":50,
        "slide_range":-60000,"turn_speed":12,"turn_max_speed":48,"poly_push_units":16},
        "definition":{},"collision":{"placements":[],"models":{},"unsupported":{}}
    }))?;
    let scene = PlayerScene::decode(&scene_bytes)?;
    let collision = cyrus_assets::collision::CollisionScene::default();
    let sources = BTreeMap::from([(
        17,
        WorldSnapshotSources {
            player: Some(&scene),
            scripts: &scripts,
            animations: &animations,
            collision: &collision,
        },
    )]);
    let original_source_bytes = serde_json::to_vec(&(
        &scripts,
        &animations,
        &collision,
        scene.source_identity.as_deref(),
    ))?;
    assert_eq!(
        sources[&17].identity()?,
        blake3::hash(&original_source_bytes).to_hex().to_string(),
        "buffered source hashing must preserve existing document identities"
    );
    let hud_asset = hud_asset();
    let preferences = Preferences::new(Store);
    let hud = ScriptHud::default();
    hud.configure(&hud_asset, &preferences);
    let mut registry = ScriptActors::default();
    let mut current = ScriptWorld::prepare(&scripts, 7)?;
    current.assign_global_flag(0, 33, true)?;
    let player_actor = current.id(2)?;
    registry.current = Some(current);
    let mut app = App::new();
    app.insert_resource(cyrus_game::fidelity::Fidelity {
        water: cyrus_game::fidelity::Mode::Modern,
        ..Default::default()
    });
    app.add_plugins((
        MinimalPlugins,
        bevy::state::app::StatesPlugin,
        AssetPlugin::default(),
        SoundPlugin,
    ))
    .insert_state(cyrus_game::AppState::InGame)
    .init_asset::<Panned>()
    .init_asset::<AudioSource>()
    .init_asset::<PlayerScene>()
    .init_asset::<Animations>()
    .init_asset::<cyrus_assets::script::Scripts>()
    .insert_resource(State::new(cyrus_game::AppState::InGame))
    .insert_resource(registry)
    .insert_resource(hud)
    .insert_resource(SimulationClock::new(scripts.clock)?)
    .insert_resource(EngineRandom::from_state(1234))
    .insert_resource(PlayerToggles {
        walk_mode: true,
        auto_defend: false,
    })
    .insert_resource(cyrus_audio::music::Music::default())
    .insert_resource(PlayerRuntime::default())
    .insert_resource(MarkerEntry::default());
    let scene_handle = app
        .world_mut()
        .resource_mut::<Assets<PlayerScene>>()
        .add(PlayerScene::decode(&scene_bytes)?);
    let animation_handle =
        app.world_mut()
            .resource_mut::<Assets<Animations>>()
            .add(serde_json::from_value::<Animations>(serde_json::to_value(
                &animations,
            )?)?);
    let script_handle = app
        .world_mut()
        .resource_mut::<Assets<cyrus_assets::script::Scripts>>()
        .add(serde_json::from_value::<cyrus_assets::script::Scripts>(
            serde_json::to_value(&scripts)?,
        )?);
    let prepared = || cyrus_world::lifecycle::PreparedWorld {
        requested_name: "TEST".into(),
        world: Some(17),
        animations: animation_handle.clone(),
        scripts: script_handle.clone(),
        player_path: "test.player".into(),
        scene_path: "test.glb".into(),
        environment_path: "test.environment".into(),
    };
    app.insert_resource(prepared())
        .insert_resource(cyrus_world::lifecycle::CurrentScene {
            name: "TEST".into(),
            world: Some(17),
        })
        .insert_resource(cyrus_world::lifecycle::SceneRequest("test".into()));
    app.insert_resource(PlayerSceneRequest(scene_handle.clone()))
        .insert_resource(cyrus_world::animation::AnimationRequest(
            animation_handle.clone(),
        ));
    let player = app
        .world_mut()
        .spawn((
            Player,
            PlayerPose {
                position: [123, -456, 789],
                heading: 12,
            },
            PlayerEntrySupport::InspectionOnly("retained".into()),
            Transform::from_xyz(1.0, 2.0, 3.0),
            ScriptBinding {
                actor: player_actor,
                source: animation_handle.clone(),
            },
        ))
        .id();
    let map_node = app
        .world_mut()
        .spawn((
            Name::new("B_000_ACTOR"),
            Transform::from_xyz(12.0, -34.0, 56.0),
            Visibility::Hidden,
            cyrus_world::animation::AnimationPose {
                model: "TEST".into(),
                frame: 1,
            },
            cyrus_world::sound::SoundSurface(7),
            ScriptBinding {
                actor: cyrus_game::script::ScriptActorId {
                    generation: player_actor.generation,
                    index: 0,
                },
                source: animation_handle.clone(),
            },
        ))
        .id();
    use cyrus_audio::music::{Music, MusicPlayer, MusicTransport};
    app.init_asset::<MusicTransport>();
    let pcm: Vec<u8> = [100_i16, -200, 300, -400, 500, -600, 700, -800]
        .into_iter()
        .flat_map(i16::to_le_bytes)
        .collect();
    let music_source = AudioSource {
        bytes: cyrus_assets::audio::pcm_wav(rgpre::import::sfx::AudioType::Stereo16, 22050, &pcm)
            .map_err(anyhow::Error::msg)?
            .into(),
    };
    let transport = MusicTransport::new(music_source.clone())?;
    let settings = transport
        .playback_settings()
        .with_volume(Volume::Linear(1.0))
        .paused();
    let mut original_music = transport.decoder();
    for _ in 0..3 {
        original_music.next().unwrap();
    }
    let music_handle = app
        .world_mut()
        .resource_mut::<Assets<MusicTransport>>()
        .add(transport);
    app.world_mut()
        .spawn((MusicPlayer, AudioPlayer(music_handle), settings));
    app.world_mut().resource_mut::<Music>().track = Some(2);
    app.insert_resource(cyrus_audio::AudioListener(GlobalTransform::IDENTITY));
    app.insert_resource(cyrus_assets::sound::SoundSettings::parse(
        "[system]\nsound_distance=64",
    ));
    let effect_source = app
        .world_mut()
        .resource_mut::<Assets<AudioSource>>()
        .add(music_source.clone());
    app.insert_resource(cyrus_audio::sound::SoundEffects(vec![Some(
        cyrus_audio::sound::Effect {
            bits_per_sample: 16,
            audio: effect_source,
            looping: true,
            base_volume: 100,
            sample_rate: 22050,
        },
    )]));
    app.world_mut()
        .write_message(cyrus_audio::sound::SoundRequest::Play {
            effect: 0,
            position: None,
        });
    app.update();
    let (old_effect, effect_handle) = app
        .world_mut()
        .query::<(Entity, &AudioPlayer<Panned>)>()
        .single(app.world())
        .map(|(entity, player)| (entity, player.0.clone()))?;
    let mut original_effect = app
        .world()
        .resource::<Assets<Panned>>()
        .get(&effect_handle)
        .unwrap()
        .decoder();
    for _ in 0..3 {
        original_effect.next().unwrap();
    }
    app.world_mut()
        .get_mut::<PlaybackSettings>(old_effect)
        .unwrap()
        .paused = true;
    let old_camera = if with_camera {
        let entity = app.world_mut().spawn(Camera3d::default()).id();
        let snapshot: cyrus_render::camera::CameraSnapshot = serde_json::from_value(json!({
            "version":1,"pose":{"translation":[8.0,9.0,10.0],"rotation":[0.0,0.0,0.0,1.0],"scale":[1.0,1.0,1.0]},
            "projection":[1.0,1.5,0.1,1000.0],"near_clip_plane":[0.0,0.0,1.0,0.0],
            "flycam":false,"speed":150.0,"frame_scene":false,"initial":null
        }))?;
        let mut queue = bevy::ecs::world::CommandQueue::default();
        snapshot
            .restore()?
            .queue(&mut Commands::new(&mut queue, app.world()), entity);
        queue.apply(app.world_mut());
        Some(entity)
    } else {
        None
    };
    let mut explicit_data: Animations = serde_json::from_value(serde_json::to_value(&animations)?)?;
    explicit_data.metadata["actors"] = json!([{
        "script_name":"ACTOR", "raan_entries":[{"file_path":"test.3dc","frame_count":2}],
        "animation_groups":[{"group_index":1,"anim_type":0,"anim_id":1,
            "commands":[{"opcode":0,"frame_reference":0},{"opcode":2,"value":0}]}]
    }]);
    explicit_data.metadata["animation_scripts"] = json!({"ACTOR":{
        "version":4,"entry":0,"steps":{"0":{"next":1,"operation":{"type":"end","target":0}}}
    }});
    let explicit = app
        .world_mut()
        .spawn(cyrus_game::script::ScriptedPlayback::new(
            animation_handle.clone(),
            &explicit_data,
            "ACTOR",
            1,
        )?)
        .id();
    let error = RuntimeSessionSnapshot::capture(app.world(), &sources, &hud_asset)
        .err()
        .expect("unowned explicit playback must reject capture");
    assert!(error.to_string().contains("explicit scripted playback"));
    app.world_mut().despawn(explicit);
    let mut cheats = cyrus_game::cheats::Cheats::new(CheatStore);
    cheats.execute("neversaydie", Some(42))?;
    app.insert_resource(cheats);
    let saved = RuntimeSessionSnapshot::capture(app.world(), &sources, &hud_asset)?;
    let shared_sources = BTreeMap::from([(
        17,
        WorldSnapshotSources {
            player: Some(&scene),
            scripts: app
                .world()
                .resource::<Assets<cyrus_assets::script::Scripts>>()
                .get(&script_handle)
                .unwrap(),
            animations: app
                .world()
                .resource::<Assets<Animations>>()
                .get(&animation_handle)
                .unwrap(),
            collision: &collision,
        },
    )]);
    assert_eq!(
        RuntimeSessionSnapshot::capture(app.world(), &shared_sources, &hud_asset)?
            .encode_document()?,
        saved.encode_document()?,
        "shared and separately decoded sources must produce identical documents"
    );
    app.world_mut()
        .resource_mut::<Assets<Animations>>()
        .get_mut(&animation_handle)
        .unwrap()
        .models
        .get_mut("TEST")
        .unwrap()
        .positions[1][0][0] += 1.0;
    assert!(RuntimeSessionSnapshot::capture(app.world(), &sources, &hud_asset).is_err());
    app.world_mut()
        .resource_mut::<Assets<Animations>>()
        .get_mut(&animation_handle)
        .unwrap()
        .models
        .get_mut("TEST")
        .unwrap()
        .positions[1][0][0] -= 1.0;
    let effect_sources = BTreeMap::from([(
        saved
            .sample_playback()
            .sources()
            .next()
            .unwrap()
            .source_identity
            .to_owned(),
        music_source.clone(),
    )]);
    render_staging::check_loaded_staging(&saved, &prepared())?;
    let bytes = saved.encode_document()?;
    assert!(std::str::from_utf8(&bytes).is_ok());
    let saved = RuntimeSessionSnapshot::decode_document(&bytes)?;
    assert_eq!(saved.encode_document()?, bytes);
    for length in [0, 15, 79, bytes.len() - 1] {
        assert!(RuntimeSessionSnapshot::decode_document(&bytes[..length]).is_err());
    }
    let mut corrupt = bytes.clone();
    *corrupt.last_mut().unwrap() ^= 1;
    assert!(RuntimeSessionSnapshot::decode_document(&corrupt).is_err());
    let mut unsupported = bytes.clone();
    unsupported[14] = b'2';
    assert!(RuntimeSessionSnapshot::decode_document(&unsupported).is_err());
    let mut appended = bytes.clone();
    appended.push(b' ');
    assert!(RuntimeSessionSnapshot::decode_document(&appended).is_err());
    assert_eq!(saved.world_ids().collect::<Vec<_>>(), [17]);
    assert!(saved.dialogue().required_labels().is_empty());
    assert_eq!(saved.music().track(), Some(2));
    let abandoned = cyrus_session::SessionEntityStage::prepare(&saved, None, app.world_mut())?;
    let abandoned_entities: Vec<_> = abandoned
        .bindings()
        .iter()
        .map(|(_, entity)| *entity)
        .collect();
    abandoned.discard(app.world_mut());
    assert!(
        abandoned_entities
            .iter()
            .all(|entity| app.world().get_entity(*entity).is_err())
    );
    assert!(app.world().get::<Player>(player).is_some());
    let stage = cyrus_session::SessionEntityStage::prepare(&saved, None, app.world_mut())?;
    let replacement = stage.player();
    let replacement_node = stage
        .bindings()
        .iter()
        .find(|(saved, _)| *saved == map_node.to_bits())
        .unwrap()
        .1;
    assert_eq!(
        app.world().get::<Visibility>(replacement),
        Some(&Visibility::Hidden)
    );
    assert!(app.world().get::<Player>(replacement).is_none());
    let camera_target = with_camera.then(|| {
        app.world_mut()
            .spawn((
                Camera3d::default(),
                Camera {
                    is_active: false,
                    ..Default::default()
                },
            ))
            .id()
    });
    let finish =
        |saved: RuntimeSessionSnapshot, scene: Handle<PlayerScene>, hud_images| -> Result<_> {
            let retained_sources = cyrus_session::SessionWorldAssets::from_loaded(
                &saved,
                BTreeMap::from([(17, (prepared(), scene.clone()))]),
            )?;
            let plan = saved.plan(LogicalSessionRestoreInputs {
                sources: &sources,
                last_generation: 20,
                hud: &hud_asset,
                preferences: &preferences,
                bios_tick: 100,
            })?;
            assert_eq!(plan.saved_player(), player.to_bits());
            let clips = BTreeMap::new();
            let requests = BTreeMap::new();
            plan.finish(
                PlayerReplacement {
                    entity: replacement,
                    scene,
                    animation: animation_handle.clone(),
                },
                cyrus_session::SessionServices {
                    sources: retained_sources,
                    scene: None,
                    camera_entity: camera_target,
                    entities: stage.bindings(),
                    source_audio: &effect_sources,
                    dialogue: RestoredDialogueAssets {
                        server: app.world().resource::<AssetServer>(),
                        audio_sources: app.world().resource::<Assets<AudioSource>>(),
                        loaded: app.world().resource::<Assets<DialogueClip>>(),
                        clips: &clips,
                        requests: &requests,
                        hud: None,
                    },
                    music_source: Some(music_source.clone()),
                    hud_asset: &hud_asset,
                    hud_images,
                    sky_images: None,
                    render_playback: None,
                    player_model_root: None,
                    actor_model_roots: &BTreeMap::new(),
                    environments: &BTreeMap::new(),
                },
                app.world(),
            )
        };
    let mut malformed = serde_json::to_value(&saved)?;
    malformed["dialogue"]["generation"] = 999.into();
    assert!(
        finish(
            serde_json::from_value(malformed)?,
            scene_handle.clone(),
            None
        )
        .is_err()
    );
    assert!(finish(saved.clone(), Handle::default(), None).is_err());
    let mut wrong_music = serde_json::to_value(&saved)?;
    wrong_music["music"]["playback"]["transport"]["source_identity"] = "wrong source".into();
    assert!(
        finish(
            serde_json::from_value(wrong_music)?,
            scene_handle.clone(),
            None
        )
        .is_err()
    );
    assert_eq!(saved.entity_bindings().len(), 2);
    let mut missing_bindings = serde_json::to_value(&saved)?;
    missing_bindings["entity_bindings"] = json!([]);
    assert!(
        finish(
            serde_json::from_value(missing_bindings)?,
            scene_handle.clone(),
            None
        )
        .is_err()
    );
    let mut wrong_actor_name = serde_json::to_value(&saved)?;
    wrong_actor_name["entity_bindings"][0]["name"] = "WRONG".into();
    assert!(
        finish(
            serde_json::from_value(wrong_actor_name)?,
            scene_handle.clone(),
            None
        )
        .is_err()
    );
    let mut wrong_frame = serde_json::to_value(&saved)?;
    let displayed = wrong_frame["entity_bindings"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|entry| entry["role"] == "map")
        .unwrap();
    displayed["displayed"] = json!(["TEST", 2]);
    let error = finish(
        serde_json::from_value(wrong_frame)?,
        scene_handle.clone(),
        None,
    )
    .err()
    .expect("missing displayed frame must reject");
    assert!(error.to_string().contains("frame 2 absent"));
    assert!(!saved.has_hud_presentation());
    let unexpected_images = cyrus_render::hud::HudPresentationAssets::default();
    assert!(
        finish(
            saved.clone(),
            scene_handle.clone(),
            Some(&unexpected_images)
        )
        .is_err()
    );
    let committed = finish(saved.clone(), scene_handle.clone(), None)?;
    let rejected = finish(saved.clone(), scene_handle.clone(), None)?;
    let candidate = finish(saved.clone(), scene_handle.clone(), None)?;
    candidate.validate_publication(app.world())?;
    app.insert_resource(cyrus_world::lifecycle::PreparationProgress {
        done: 1,
        total: 1,
        label: "prepared".into(),
    });
    candidate.validate_publication(app.world())?;
    RuntimeSessionSnapshot::capture(app.world(), &sources, &hud_asset)?;
    app.insert_resource(cyrus_world::lifecycle::WorldPreparationPending);
    assert!(candidate.validate_publication(app.world()).is_err());
    assert!(RuntimeSessionSnapshot::capture(app.world(), &sources, &hud_asset).is_err());
    app.world_mut()
        .remove_resource::<cyrus_world::lifecycle::WorldPreparationPending>();

    app.world_mut()
        .resource_mut::<Assets<cyrus_assets::script::Scripts>>()
        .get_mut(&script_handle)
        .unwrap()
        .clock
        .normal_frame_rate += 1;
    assert!(candidate.validate_publication(app.world()).is_err());
    assert!(RuntimeSessionSnapshot::capture(app.world(), &sources, &hud_asset).is_err());
    assert!(app.world().get::<Player>(player).is_some());
    app.world_mut()
        .resource_mut::<Assets<cyrus_assets::script::Scripts>>()
        .get_mut(&script_handle)
        .unwrap()
        .clock
        .normal_frame_rate -= 1;
    candidate.validate_publication(app.world())?;

    let mut reader = cyrus_game::logbook::LogbookScreen::default();
    reader.waiting_for_release = true;
    app.insert_resource(reader);
    assert!(
        candidate
            .validate_publication(app.world())
            .unwrap_err()
            .to_string()
            .contains("closing-key release")
    );
    assert!(RuntimeSessionSnapshot::capture(app.world(), &sources, &hud_asset).is_err());
    app.world_mut()
        .remove_resource::<cyrus_game::logbook::LogbookScreen>();
    app.add_plugins(cyrus_session::SessionBoundaryPlugin);
    let capture_sources = |world: &World| {
        cyrus_session::SessionWorldAssets::for_capture(
            world.resource::<ScriptActors>(),
            BTreeMap::from([(17, prepared())]),
            world.resource::<AssetServer>(),
        )
    };
    let pending_sources = capture_sources(app.world())?;
    app.world_mut()
        .resource_mut::<cyrus_session::SessionBoundary>()
        .capture(pending_sources, Handle::default())?;
    let duplicate_sources = capture_sources(app.world())?;
    assert!(
        app.world_mut()
            .resource_mut::<cyrus_session::SessionBoundary>()
            .capture(duplicate_sources, Handle::default())
            .is_err()
    );
    app.world_mut().run_schedule(Last);
    let retained_sources = app
        .world()
        .resource::<cyrus_session::SessionSourceCatalog>()
        .for_capture(app.world())?;
    assert_eq!(retained_sources.current().0.scripts, script_handle);
    assert_eq!(retained_sources.current().1, &scene_handle);
    assert!(retained_sources.sources(app.world())?.is_some());
    assert!(
        app.world_mut()
            .resource_mut::<cyrus_session::SessionSourceCatalog>()
            .retain(99, prepared(), scene_handle.clone())
            .is_err()
    );
    let unread_sources = capture_sources(app.world())?;
    assert!(
        app.world_mut()
            .resource_mut::<cyrus_session::SessionBoundary>()
            .capture(unread_sources, Handle::default())
            .is_err()
    );
    let capture_error = app
        .world_mut()
        .resource_mut::<cyrus_session::SessionBoundary>()
        .take_result()
        .unwrap()
        .err()
        .expect("unprepared capture must reject");
    assert!(capture_error.to_string().contains("session"));
    assert!(app.world().get::<Player>(player).is_some());
    app.world_mut()
        .entity_mut(replacement_node)
        .insert(ScriptBinding {
            actor: player_actor,
            source: animation_handle.clone(),
        });
    assert!(
        candidate
            .validate_publication(app.world())
            .unwrap_err()
            .to_string()
            .contains("already has a live owner")
    );
    app.world_mut()
        .resource_mut::<cyrus_session::SessionBoundary>()
        .commit(rejected)?;
    app.world_mut().run_schedule(Last);
    let rejection = app
        .world_mut()
        .resource_mut::<cyrus_session::SessionBoundary>()
        .take_result()
        .unwrap()
        .err()
        .expect("stale owner must reject at execution");
    assert!(rejection.to_string().contains("already has a live owner"));
    assert!(app.world().get::<Player>(player).is_some());
    app.world_mut()
        .entity_mut(replacement_node)
        .remove::<ScriptBinding>();
    let original = app
        .world_mut()
        .resource_mut::<ScriptActors>()
        .current
        .take();
    app.world_mut().resource_mut::<ScriptActors>().current =
        Some(ScriptWorld::prepare(&scripts, 100)?);
    assert!(
        candidate
            .validate_publication(app.world())
            .unwrap_err()
            .to_string()
            .contains("actor allocation advanced")
    );
    app.world_mut().resource_mut::<ScriptActors>().current = original;
    candidate.validate_publication(app.world())?;
    assert_eq!(
        RuntimeSessionSnapshot::capture(app.world_mut(), &sources, &hud_asset)?
            .encode_document()?,
        bytes
    );
    assert!(app.world().get::<PlayerPose>(replacement).is_none());

    app.world_mut()
        .resource_mut::<cyrus_world::lifecycle::SceneRequest>()
        .0 = "OTHER".into();
    assert!(RuntimeSessionSnapshot::capture(app.world(), &sources, &hud_asset).is_err());
    app.world_mut()
        .resource_mut::<cyrus_world::lifecycle::SceneRequest>()
        .0 = "test".into();
    let extra = app.world_mut().spawn(Player).id();
    assert!(RuntimeSessionSnapshot::capture(app.world(), &sources, &hud_asset).is_err());
    app.world_mut().despawn(extra);
    assert!(app.world().get::<Transform>(replacement_node).is_none());
    app.world_mut()
        .resource_mut::<cyrus_game::fidelity::Fidelity>()
        .water = cyrus_game::fidelity::Mode::Faithful;
    app.world_mut()
        .resource_mut::<cyrus_game::cheats::Cheats>()
        .execute("neversaydie", Some(0))?;
    if from_menu {
        app.world_mut()
            .insert_resource(State::new(cyrus_game::AppState::Menu));
        app.world_mut()
            .resource_mut::<NextState<cyrus_game::AppState>>()
            .set(cyrus_game::AppState::Loading);
        assert!(committed.validate_publication(app.world()).is_err());
        *app.world_mut()
            .resource_mut::<NextState<cyrus_game::AppState>>() = NextState::Unchanged;
    }
    app.world_mut()
        .resource_mut::<cyrus_session::SessionBoundary>()
        .commit(committed)?;
    assert!(app.world().get::<Transform>(replacement_node).is_none());
    app.world_mut().run_schedule(Last);
    assert!(matches!(
        app.world_mut()
            .resource_mut::<cyrus_session::SessionBoundary>()
            .take_result()
            .unwrap()?,
        cyrus_session::SessionBoundaryResult::Committed
    ));
    let restored_sources = app
        .world()
        .resource::<cyrus_session::SessionSourceCatalog>()
        .for_capture(app.world())?;
    assert_eq!(restored_sources.current().1, &scene_handle);
    assert!(restored_sources.sources(app.world())?.is_some());
    assert!(app.world().get_entity(old_effect).is_err());
    let (resumed_handle, resumed_settings) = app
        .world_mut()
        .query::<(&AudioPlayer<Panned>, &PlaybackSettings)>()
        .single(app.world())?;
    assert!(resumed_settings.paused);
    let mut resumed_effect = app
        .world()
        .resource::<Assets<Panned>>()
        .get(&resumed_handle.0)
        .unwrap()
        .decoder();
    for _ in 0..29 {
        assert_eq!(resumed_effect.next(), original_effect.next());
    }

    assert_eq!(
        app.world()
            .resource::<cyrus_game::cheats::Cheats>()
            .state()?
            .value("neversaydie"),
        Some(42)
    );
    if from_menu {
        assert!(
            app.world()
                .contains_resource::<cyrus_game::RestoredSessionEntry>()
        );
        app.world_mut()
            .run_schedule(bevy::state::state::StateTransition);
        assert_eq!(
            app.world().resource::<State<cyrus_game::AppState>>().get(),
            &cyrus_game::AppState::InGame
        );
    }
    let (music_handle, settings) = app
        .world_mut()
        .query_filtered::<(
            &AudioPlayer<cyrus_audio::music::MusicTransport>,
            &PlaybackSettings,
        ), With<cyrus_audio::music::MusicPlayer>>()
        .single(app.world())?;
    assert!(settings.paused);
    let mut resumed_music = app
        .world()
        .resource::<Assets<cyrus_audio::music::MusicTransport>>()
        .get(&music_handle.0)
        .unwrap()
        .decoder();
    for _ in 0..29 {
        assert_eq!(resumed_music.next(), original_music.next());
    }
    assert!(
        app.world()
            .get::<ScriptBinding>(replacement)
            .unwrap()
            .actor
            .generation
            > 20
    );

    assert_eq!(
        app.world()
            .resource::<cyrus_game::fidelity::Fidelity>()
            .water,
        cyrus_game::fidelity::Mode::Modern
    );
    assert!(app.world().get_entity(player).is_err());
    assert!(app.world().get_entity(map_node).is_err());
    if let Some(old) = old_camera {
        assert!(app.world().get_entity(old).is_err());
    }
    if let Some(camera) = camera_target {
        assert_eq!(
            app.world_mut()
                .query_filtered::<Entity, With<cyrus_render::camera::ViewCamera>>()
                .iter(app.world())
                .collect::<Vec<_>>(),
            [camera]
        );
        assert_eq!(
            app.world().get::<Transform>(camera).unwrap().translation,
            Vec3::new(8.0, 9.0, 10.0)
        );
    }
    assert!(app.world().get::<Player>(replacement).is_some());
    assert_eq!(
        app.world()
            .resource::<ScriptActors>()
            .current
            .as_ref()
            .unwrap()
            .flags(),
        [33]
    );
    assert_eq!(app.world().resource::<EngineRandom>().state(), 1234);
    let pose = app
        .world()
        .get::<cyrus_world::animation::AnimationPose>(replacement_node)
        .unwrap();
    assert_eq!(pose.model, "TEST");
    assert_eq!(pose.frame, 1);

    assert_eq!(
        app.world()
            .get::<Transform>(replacement_node)
            .unwrap()
            .translation,
        Vec3::new(12.0, -34.0, 56.0)
    );
    assert_eq!(
        *app.world().get::<Visibility>(replacement_node).unwrap(),
        Visibility::Hidden
    );
    assert_eq!(
        app.world()
            .get::<cyrus_world::sound::SoundSurface>(replacement_node)
            .unwrap()
            .0,
        7
    );

    let generation = app
        .world()
        .resource::<ScriptActors>()
        .current
        .as_ref()
        .unwrap()
        .generation();
    let saved_pose = app.world().get::<PlayerPose>(replacement).unwrap();
    let pose = (saved_pose.position, saved_pose.heading);
    let transform = *app.world().get::<Transform>(replacement).unwrap();
    let source = app.world().resource::<PlayerSceneRequest>().0.clone();
    if via_load {
        app.init_asset::<cyrus_assets::hud::HudAsset>();
        app.insert_resource(preferences);
        let hud_handle = app
            .world()
            .resource::<Assets<cyrus_assets::hud::HudAsset>>()
            .reserve_handle();
        let audio_handle = app
            .world_mut()
            .resource_mut::<Assets<AudioSource>>()
            .add(music_source.clone());
        let load_assets = cyrus_session::SessionLoadAssets {
            sources: cyrus_session::SessionWorldAssets::from_loaded(
                &saved,
                BTreeMap::from([(17, (prepared(), scene_handle.clone()))]),
            )?,
            hud: hud_handle.clone(),
            hud_images: None,
            samples: effect_sources
                .keys()
                .map(|identity| (identity.clone(), audio_handle.clone()))
                .collect(),
            dialogue: BTreeMap::new(),
            music: Some(audio_handle),
        };
        app.world_mut()
            .resource_mut::<cyrus_session::SessionBoundary>()
            .load(saved.clone(), load_assets.clone())?;
        app.world_mut().run_schedule(Last);
        assert!(
            app.world_mut()
                .resource_mut::<cyrus_session::SessionBoundary>()
                .take_result()
                .is_none()
        );
        assert!(app.world().get::<Player>(replacement).is_some());
        assert!(
            app.world_mut()
                .resource_mut::<cyrus_session::SessionBoundary>()
                .load(saved.clone(), load_assets.clone())
                .is_err()
        );
        app.world_mut()
            .resource_mut::<Assets<cyrus_assets::hud::HudAsset>>()
            .insert(
                &hud_handle,
                serde_json::from_value(serde_json::to_value(&hud_asset)?)?,
            )?;
        app.world_mut().run_schedule(Last);
        assert!(matches!(
            app.world_mut()
                .resource_mut::<cyrus_session::SessionBoundary>()
                .take_result()
                .unwrap()?,
            cyrus_session::SessionBoundaryResult::Committed
        ));
        assert!(app.world().get_entity(replacement).is_err());
        let loaded_player = app
            .world_mut()
            .query_filtered::<Entity, With<Player>>()
            .single(app.world())?;
        assert_eq!(
            app.world()
                .get::<PlayerPose>(loaded_player)
                .unwrap()
                .position,
            pose.0
        );
        let live_count = app.world_mut().query::<Entity>().iter(app.world()).count();
        let mut malformed = serde_json::to_value(&saved)?;
        malformed["dialogue"]["generation"] = 999.into();
        app.world_mut()
            .resource_mut::<cyrus_session::SessionBoundary>()
            .load(serde_json::from_value(malformed)?, load_assets)?;
        app.world_mut().run_schedule(Last);
        assert!(
            app.world_mut()
                .resource_mut::<cyrus_session::SessionBoundary>()
                .take_result()
                .unwrap()
                .is_err()
        );
        assert!(app.world().get::<Player>(loaded_player).is_some());
        assert_eq!(
            app.world_mut().query::<Entity>().iter(app.world()).count(),
            live_count
        );
        return Ok(());
    }

    let script_assets = app
        .world_mut()
        .remove_resource::<Assets<Scripts>>()
        .unwrap();
    let player_assets = app
        .world_mut()
        .remove_resource::<Assets<PlayerScene>>()
        .unwrap();
    app.init_resource::<cyrus_game::fidelity::Fidelity>()
        .init_resource::<ButtonInput<KeyCode>>()
        .add_message::<cyrus_world::animation::AnimationTick>()
        .add_plugins((
            cyrus_world::lifecycle::WorldPlugin,
            cyrus_game::player::PlayerPlugin,
            cyrus_game::script::ScriptPlugin,
        ));
    // Late plugin registration replaces asset stores; retain the fixture's handles and sources.
    let server = app.world().resource::<AssetServer>();
    server.register_asset(&script_assets);
    server.register_asset(&player_assets);
    app.insert_resource(script_assets)
        .insert_resource(player_assets);
    app.update();
    app.update();
    let current = app
        .world()
        .resource::<ScriptActors>()
        .current
        .as_ref()
        .unwrap();
    assert_eq!(current.generation(), generation);
    assert_eq!(current.flags(), [33]);
    assert_eq!(app.world().resource::<PlayerSceneRequest>().0, source);
    let retained_pose = app.world().get::<PlayerPose>(replacement).unwrap();
    assert_eq!((retained_pose.position, retained_pose.heading), pose);
    assert_eq!(app.world().get::<Transform>(replacement), Some(&transform));
    assert!(app.world().get::<ScriptBinding>(replacement).is_some());
    assert!(app.world().get::<ScriptBinding>(replacement_node).is_some());
    assert_eq!(
        app.world_mut()
            .query_filtered::<Entity, With<Player>>()
            .iter(app.world())
            .collect::<Vec<_>>(),
        [replacement]
    );

    stage.discard(app.world_mut());
    assert!(app.world().get::<Player>(replacement).is_some());
    assert!(app.world().get::<ScriptBinding>(replacement_node).is_some());
    assert_eq!(
        app.world_mut()
            .query::<&AudioPlayer<Panned>>()
            .iter(app.world())
            .count(),
        1
    );

    if !with_camera && let Some(path) = std::env::var_os("CYRUS_SESSION_FIXTURE") {
        std::fs::write(path, &bytes)?;
    }
    Ok(())
}
