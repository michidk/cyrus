//! Installed receiver preparation retains special roles and ordinary authored allocations.
use cyrus_game::script::{ScriptActorSource, ScriptWorld};

#[test]
fn installed_island_flags_advance_through_ordinary_updates() {
    use bevy::{asset::AssetApp, prelude::*, state::app::StatesPlugin};
    use cyrus_assets::animation::Animations;
    use cyrus_assets::script::Scripts;
    use cyrus_game::{
        AppState,
        script::{ScriptActors, ScriptPlugin, ScriptView},
    };
    use cyrus_world::{
        animation::{AnimationRequest, AnimationTick},
        lifecycle::PreparedWorld,
    };
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../redguard");
    if !root.join("RGFX.EXE").exists() {
        eprintln!("skipped: no install (SOUP automatic island flags)");
        return;
    }
    let path = std::fs::read_dir(root.join("maps"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .eq_ignore_ascii_case("ISLAND.RGM")
        })
        .unwrap();
    let map = rgpre::import::rgm::parse_rgm_file(&std::fs::read(path).unwrap()).unwrap();
    let scripts = cyrus_assets::script_export::prepare(&map, &root, 0).unwrap();
    let mut metadata = rgpre::import::rgm::export_rgm_metadata_json(&map, None);
    metadata["script_content_id"] = scripts.content_id.clone().into();
    let data = Animations {
        version: 2,
        world: scripts.world,
        metadata,
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    };
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world.prepare_markers(&data).unwrap();
    world.prepare_navigation(&data).unwrap();
    let index = world
        .actors()
        .iter()
        .position(|actor| actor.name() == "FLAG2")
        .unwrap();
    let placement = world.actors()[index].placement().unwrap();
    let view = [
        placement.map_position[0] as i32,
        placement.map_position[2] as i32,
    ];
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, StatesPlugin, AssetPlugin::default()))
        .init_state::<AppState>()
        .init_asset::<Scripts>()
        .init_asset::<Animations>()
        .add_message::<AnimationTick>()
        .add_plugins(ScriptPlugin);
    let scripts = app
        .world_mut()
        .resource_mut::<Assets<Scripts>>()
        .add(scripts);
    let animations = app
        .world_mut()
        .resource_mut::<Assets<Animations>>()
        .add(data);
    app.insert_resource(PreparedWorld {
        requested_name: "ISLAND".into(),
        world: Some(0),
        scripts,
        animations: animations.clone(),
        player_path: "test.player".into(),
        scene_path: "test.glb".into(),
        environment_path: "test.environment".into(),
    });
    app.insert_resource(AnimationRequest(animations.clone()));
    app.insert_resource(ScriptView {
        source: animations,
        position: [view[0] * 256, 0, view[1] * 256],
    });
    app.world_mut()
        .resource_mut::<NextState<AppState>>()
        .set(AppState::InGame);
    let mut frames = Vec::new();
    for _ in 0..24 {
        app.update();
        let actors = app.world().resource::<ScriptActors>();
        let world = actors.current.as_ref().unwrap();
        assert!(
            world.actors()[index].failure().is_none(),
            "{:?}",
            world.actors()[index].failure()
        );
        frames.push(world.animation_frame(world.id(index).unwrap()).unwrap());
    }
    assert!(
        frames.windows(2).any(|frames| frames[0] != frames[1]),
        "installed FLAG2 remained frozen: {frames:?}"
    );
}

#[test]
fn installed_maps_prepare_receivers_without_using_map_object_ids_as_runtime_ids() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../redguard");
    if !root.join("RGFX.EXE").exists() {
        eprintln!("skipped: no install (SOUP receiver instances)");
        return;
    }
    let mut maps = 0;
    for file in std::fs::read_dir(root.join("maps")).unwrap() {
        let path = file.unwrap().path();
        if !path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("rgm"))
        {
            continue;
        }
        let map = rgpre::import::rgm::parse_rgm_file(&std::fs::read(&path).unwrap()).unwrap();
        let scripts = cyrus_assets::script_export::prepare(&map, &root, 0).unwrap();
        let mut world = ScriptWorld::prepare(&scripts, 1)
            .unwrap_or_else(|error| panic!("{}: {error:#}", path.display()));
        let mut metadata = rgpre::import::rgm::export_rgm_metadata_json(&map, None);
        metadata["script_content_id"] = scripts.content_id.clone().into();
        world
            .prepare_markers(&cyrus_assets::animation::Animations {
                version: 2,
                world: scripts.world,
                metadata,
                sections: Default::default(),
                models: Default::default(),
                unsupported: Default::default(),
            })
            .unwrap_or_else(|error| panic!("{}: {error:#}", path.display()));
        for actor in world.actors() {
            if let Some(error) = actor.unavailable() {
                assert!(
                    !error.contains("authored instance allocation exceeded"),
                    "{}: {error}",
                    path.display()
                );
            }
            if let Some(placement) = actor.placement()
                && matches!(placement.object_type, 2 | 6)
            {
                assert!(actor.runtime_identifier.is_none());
                assert!(actor.runtime().is_none());
            }
        }
        for (name, role, identifier) in [
            ("CYRUS", "player", 1),
            ("GREMLIN", "alternate_player", 1),
            ("CAMERA", "camera", 2),
        ] {
            if let Some(definition) = scripts.actors.get(name) {
                let actor = world
                    .actors()
                    .iter()
                    .find(|actor| match actor.source {
                        ScriptActorSource::Player => role == "player",
                        ScriptActorSource::AlternatePlayer => role == "alternate_player",
                        ScriptActorSource::Camera => role == "camera",
                        _ => false,
                    })
                    .unwrap();
                assert_eq!(actor.runtime_identifier, Some(identifier));
                assert_eq!(actor.name(), name);
                assert!(actor.placement().is_none());
                assert_eq!(
                    actor.runtime().unwrap().variables(),
                    definition.instance_variables[0]
                );
            }
        }
        maps += 1;
    }
    assert!(maps > 0);
    eprintln!("SOUP receiver preparation: {maps} installed maps");
}

#[test]
fn installed_palace_nested_family_reconstructs_authored_poses() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../redguard");
    if !root.join("RGFX.EXE").exists() {
        eprintln!("skipped: no install (SOUP nested family)");
        return;
    }
    let path = std::fs::read_dir(root.join("maps"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .eq_ignore_ascii_case("PALACE.RGM")
        })
        .unwrap();
    let map = rgpre::import::rgm::parse_rgm_file(&std::fs::read(path).unwrap()).unwrap();
    let scripts = cyrus_assets::script_export::prepare(&map, &root, 3).unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world.prepare_spatial().unwrap();
    let ids: Vec<_> = ["PI_PILA", "PI_PLTA", "PI_LVRA"]
        .into_iter()
        .map(|name| {
            world
                .id(world
                    .actors()
                    .iter()
                    .position(|actor| actor.name() == name)
                    .unwrap())
                .unwrap()
        })
        .collect();
    assert_eq!(world.spatial(ids[1]).unwrap().parent, Some(ids[0].index));
    assert_eq!(world.spatial(ids[2]).unwrap().parent, Some(ids[1].index));
    world.refresh_families().unwrap();
    for id in ids {
        let placement = world.actors()[id.index].placement().unwrap();
        let pose = world.spatial(id).unwrap().query_pose().unwrap();
        assert_eq!(
            pose.position,
            placement
                .map_position
                .map(|value| i32::try_from(value * 256).unwrap())
        );
        assert_eq!(
            pose.angles,
            placement.angles.map(|value| (value & 2047) as u16)
        );
    }
}

#[test]
fn installed_palace_lever_shares_player_tasks_waits_sound_and_rotation() {
    use cyrus_assets::{animation::Animations, hud::HudAsset, script::Scripts};
    use cyrus_game::script::{
        ActivationActor, ActivationInputs, PlayerQuery, QueryInputs, ScriptInputs, ScriptSound,
        ScriptStage,
    };
    use cyrus_script::Thread;
    use std::collections::BTreeMap;

    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../redguard");
    if !root.join("RGFX.EXE").exists() {
        eprintln!("skipped: no install (SOUP palace lever sequence)");
        return;
    }
    let install = cyrus_assets::install::GameInstall::locate(Some(&root)).unwrap();
    let output = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!(
        "../../cyrus-config/soup-lever-test-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&output).unwrap();
    let scene = output.join("PALACE.glb");
    cyrus_assets::scene::export_scene(&install, "PALACE", &scene).unwrap();
    cyrus_assets::hud_export::export_hud(&install, &output).unwrap();
    let scripts: Scripts =
        serde_json::from_slice(&std::fs::read(scene.with_extension("scripts")).unwrap()).unwrap();
    let animations: Animations =
        serde_json::from_slice(&std::fs::read(scene.with_extension("animations")).unwrap())
            .unwrap();
    let hud: HudAsset =
        serde_json::from_slice(&std::fs::read(output.join("hud/overlay.hud")).unwrap()).unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let actor_id = |name| {
        world
            .id(world
                .actors()
                .iter()
                .position(|actor| actor.name() == name)
                .unwrap())
            .unwrap()
    };
    let lever = actor_id("PI_LVRA");
    let player = actor_id("CYRUS");
    let flag = scripts
        .definition
        .flags
        .iter()
        .position(|flag| flag.name == "pi_throneA")
        .unwrap();
    assert_eq!(world.flags()[flag], 0);
    world.prepare_spatial().unwrap();
    world.refresh_families().unwrap();
    let pose = world.spatial(lever).unwrap().query_pose().unwrap();
    let query = QueryInputs {
        player: Some(PlayerQuery {
            position: pose.position,
            heading: pose.angles[1],
        }),
        distance_override: Some(false),
        fallback_position: None,
        linked_position: None,
    };
    let initial_pitch = world.spatial(lever).unwrap().local.angles[0];
    // Explicit caller inputs isolate this sequence; they do not establish ordinary eligibility.
    world.bind_animation(player, &animations, 0).unwrap();
    let state = ActivationActor {
        state: 0,
        on_rope: false,
        water_state: 0,
        torch_burning: 0,
        weapon_assigned: false,
        weapon_drawn: false,
        animation: None,
    };
    let mut activation = ActivationInputs {
        player,
        actors: BTreeMap::from([(player.index, state), (lever.index, state)]),
        labels: hud.labels,
        lockout: 0,
        dialogue_open: false,
        lineup_result: 0,
        use_held: true,
        item_system_ready: true,
        shell_execution: false,
        render_disabled: false,
    };
    let mut sounds = Vec::new();
    let mut player_frames = Vec::new();
    let mut saw_player_task = false;
    for _ in 0..64 {
        let inputs = ScriptInputs {
            activation: Some(&activation),
            nominal_ticks: Some(1),
            frame_scale: Some(256),
            player_query: Some(query),
            ..Default::default()
        };
        for (id, stage) in [
            (player, ScriptStage::Animation),
            (player, ScriptStage::Tasks),
            (lever, ScriptStage::Tasks),
            (lever, ScriptStage::Body(Thread::Main)),
        ] {
            world
                .advance(
                    id,
                    stage,
                    inputs,
                    Some(&mut |owner, sound| {
                        sounds.push((owner, sound));
                        Ok(())
                    }),
                    None,
                    100,
                )
                .unwrap();
        }
        saw_player_task |= world.actors()[player.index]
            .runtime()
            .unwrap()
            .active_tasks()
            != 0;
        if let Some(frame) = world.animation_frame(player).unwrap() {
            animations.resolve_frame("CYRUS", frame).unwrap();
            player_frames.push(frame);
        }
        world.refresh_families().unwrap();
        world
            .activation_overlay(&activation, 1, false, hud.override_ticks)
            .unwrap();
        activation.use_held = false;
        if world.flags()[flag] == 1 {
            break;
        }
    }
    assert!(
        saw_player_task,
        "lever did not allocate the player's animation task"
    );
    assert!(
        player_frames
            .windows(2)
            .any(|frames| frames[0] != frames[1])
    );
    assert_eq!(world.flags()[flag], 1);
    assert_eq!(
        world.spatial(lever).unwrap().local.angles[0],
        initial_pitch.wrapping_sub(256) & 2047
    );
    assert_eq!(
        sounds
            .iter()
            .filter(|(_, sound)| matches!(sound, ScriptSound::Flat { .. }))
            .collect::<Vec<_>>(),
        vec![&(
            lever,
            ScriptSound::Flat {
                effect: 37,
                volume: 0,
                rate_offset_hz: 0
            }
        )]
    );
    std::fs::remove_dir_all(output).unwrap();
}

#[test]
fn installed_island_mechanism_moves_its_master_and_refreshes_attached_poses() {
    use cyrus_game::script::{ScriptInputs, ScriptStage};
    use cyrus_script::Thread;
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../redguard");
    if !root.join("RGFX.EXE").exists() {
        eprintln!("skipped: no install (SOUP axis movement)");
        return;
    }
    let path = std::fs::read_dir(root.join("maps"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .eq_ignore_ascii_case("ISLAND.RGM")
        })
        .unwrap();
    let map = rgpre::import::rgm::parse_rgm_file(&std::fs::read(path).unwrap()).unwrap();
    let mut scripts = cyrus_assets::script_export::prepare(&map, &root, 0).unwrap();
    // Supply the scenario's enabling flag without replacing its authored program.
    scripts
        .definition
        .flags
        .iter_mut()
        .find(|flag| flag.name == "dr_boiler_move")
        .unwrap()
        .initial_value = 1;
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world.prepare_spatial().unwrap();
    world.refresh_families().unwrap();
    let id = world
        .id(world
            .actors()
            .iter()
            .position(|actor| actor.name() == "DR_EXMB1")
            .unwrap())
        .unwrap();
    let child = (0..world.actors().len())
        .filter_map(|index| {
            let candidate = world.id(index).unwrap();
            (world.spatial(candidate).ok()?.parent == Some(id.index)).then_some(candidate)
        })
        .next()
        .expect("installed mechanism has an attached child");
    let initial = world.spatial(id).unwrap().local.position;
    let child_before = world.spatial(child).unwrap().effective.unwrap().position;
    let inputs = ScriptInputs {
        frame_scale: Some(256),
        nominal_ticks: Some(1),
        ..Default::default()
    };
    let mut sounds = Vec::new();
    let mut emit = |receiver, sound| {
        sounds.push((receiver, sound));
        Ok(())
    };
    world
        .advance(
            id,
            ScriptStage::Body(Thread::Main),
            inputs,
            Some(&mut emit),
            None,
            100,
        )
        .unwrap();
    assert_eq!(
        world.spatial(id).unwrap().local.position[0],
        initial[0] - 1792
    );
    for _ in 0..18 {
        world
            .advance(id, ScriptStage::Tasks, inputs, Some(&mut emit), None, 100)
            .unwrap();
    }
    assert_eq!(
        world.spatial(id).unwrap().local.position,
        [initial[0] - 32768, initial[1], initial[2]]
    );
    assert_eq!(
        world.actors()[id.index].runtime().unwrap().active_tasks(),
        0
    );
    assert_eq!(
        world.spatial(child).unwrap().effective.unwrap().position,
        child_before
    );
    world.refresh_families().unwrap();
    assert_eq!(
        world.spatial(child).unwrap().effective.unwrap().position,
        [child_before[0] - 32768, child_before[1], child_before[2]]
    );
    assert_eq!(sounds.len(), 1);
    assert_eq!(sounds[0].0, id);
}

#[test]
fn installed_hideout_explicit_hook_reports_its_out_of_range_marker() {
    use cyrus_game::script::{ScriptInputs, ScriptStage};
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../redguard");
    if !root.join("RGFX.EXE").exists() {
        eprintln!("skipped: no install (SOUP node marker hook)");
        return;
    }
    let path = std::fs::read_dir(root.join("maps"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .eq_ignore_ascii_case("HIDEOUT.RGM")
        })
        .unwrap();
    let map = rgpre::import::rgm::parse_rgm_file(&std::fs::read(path).unwrap()).unwrap();
    let scripts = cyrus_assets::script_export::prepare(&map, &root, 0).unwrap();
    let hook_actor = scripts
        .actors
        .iter()
        .find(|(_, actor)| {
            actor
                .native_hook
                .as_ref()
                .is_some_and(|call| call.function == "AtNodeMarker")
        })
        .unwrap()
        .0;
    let mut metadata = rgpre::import::rgm::export_rgm_metadata_json(&map, None);
    metadata["script_content_id"] = scripts.content_id.clone().into();
    let data = cyrus_assets::animation::Animations {
        version: 2,
        world: scripts.world,
        metadata,
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    };
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world.prepare_markers(&data).unwrap();
    world.prepare_spatial().unwrap();
    let index = world
        .actors()
        .iter()
        .position(|actor| actor.name() == hook_actor)
        .unwrap();
    let id = world.id(index).unwrap();
    let before = world.actors()[index]
        .runtime()
        .unwrap()
        .program_counter(cyrus_script::Thread::Main);
    let error = world
        .advance(
            id,
            ScriptStage::OrdinaryHook,
            ScriptInputs {
                frame_scale: Some(256),
                nominal_ticks: Some(0),
                ..Default::default()
            },
            None,
            None,
            20,
        )
        .unwrap_err();
    let error = format!("{error:#}");
    assert!(error.contains("actor MARINE"), "{error}");
    assert!(error.contains("AtNodeMarker"), "{error}");
    assert!(
        error.contains("marker 262144: outside ordinary world table"),
        "{error}"
    );
    assert_eq!(
        world.actors()[index]
            .runtime()
            .unwrap()
            .program_counter(cyrus_script::Thread::Main),
        before
    );
    assert!(world.actors()[index].failure().is_some());
}

#[test]
fn installed_viledog_selects_its_branch_from_the_shared_random_draw() {
    use cyrus_game::{random::EngineRandom, script::ScriptStage};
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../redguard");
    if !root.join("RGFX.EXE").exists() {
        eprintln!("skipped: no install (SOUP random branch)");
        return;
    }
    let map =
        rgpre::import::rgm::parse_rgm_file(&std::fs::read(root.join("maps/VILE.RGM")).unwrap())
            .unwrap();
    let scripts = cyrus_assets::script_export::prepare(&map, &root, 14).unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let index = world
        .actors()
        .iter()
        .position(|actor| actor.name() == "VILEDOG")
        .unwrap();
    let mut random = EngineRandom::default();
    world
        .advance(
            world.id(index).unwrap(),
            ScriptStage::Body(cyrus_script::Thread::Main),
            Default::default(),
            None,
            Some(&mut random),
            100,
        )
        .unwrap();
    assert_eq!(
        world.actors()[index].runtime().unwrap().variables(),
        [4, 0, 1, 6]
    );
    assert_eq!(random.state(), 1103527590);
    assert!(world.actors()[index].failure().is_none());
}

#[test]
fn installed_palace_actor_is_selected_at_its_authored_world_position() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../redguard");
    if !root.join("RGFX.EXE").exists() {
        eprintln!("skipped: no install (SOUP ordinary actor coordinates)");
        return;
    }
    let map =
        rgpre::import::rgm::parse_rgm_file(&std::fs::read(root.join("maps/PALACE.RGM")).unwrap())
            .unwrap();
    let scripts = cyrus_assets::script_export::prepare(&map, &root, 3).unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let id = world.id(0).unwrap();
    let placement = world.actors()[0].placement().unwrap();
    assert_eq!(placement.actor, "GUARDPL1");
    assert_eq!(placement.active, 0);
    let view = [placement.map_position[0], placement.map_position[2]]
        .map(|coordinate| i32::try_from(coordinate).unwrap());
    assert!(
        view.iter()
            .all(|coordinate| (9216..56319).contains(coordinate))
    );
    assert!(world.ordinary_actors(view).unwrap().contains(&id));
    assert!(!world.ordinary_actors([4608, 4608]).unwrap().contains(&id));
}

#[test]
fn installed_viledog_wander_hook_consumes_random_without_walk_maps() {
    use cyrus_game::{
        random::EngineRandom,
        script::{ScriptInputs, ScriptStage},
    };
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../redguard");
    if !root.join("RGFX.EXE").exists() {
        eprintln!("skipped: no install (SOUP Wander hook)");
        return;
    }
    let map =
        rgpre::import::rgm::parse_rgm_file(&std::fs::read(root.join("maps/VILE.RGM")).unwrap())
            .unwrap();
    let scripts = cyrus_assets::script_export::prepare(&map, &root, 14).unwrap();
    let mut metadata = rgpre::import::rgm::export_rgm_metadata_json(&map, None);
    metadata["script_content_id"] = scripts.content_id.clone().into();
    let data = cyrus_assets::animation::Animations {
        version: 2,
        world: scripts.world,
        metadata,
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    };
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world.prepare_navigation(&data).unwrap();
    world.prepare_spatial().unwrap();
    let index = world
        .actors()
        .iter()
        .position(|actor| actor.name() == "VILEDOG")
        .unwrap();
    let id = world.id(index).unwrap();
    let before = world.spatial(id).unwrap().local.position;
    let mut random = EngineRandom::default();
    world
        .advance(
            id,
            ScriptStage::NativeHook,
            ScriptInputs {
                frame_scale: Some(256),
                nominal_ticks: Some(0),
                ..Default::default()
            },
            None,
            Some(&mut random),
            100,
        )
        .unwrap();
    assert_eq!(random.state(), 2524885223);
    assert_eq!(world.actors()[index].attributes()[94], 1);
    assert_eq!(world.spatial(id).unwrap().local.position, before);
    assert!(world.actors()[index].failure().is_none());
}
