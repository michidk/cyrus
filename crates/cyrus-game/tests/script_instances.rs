//! Installed receiver preparation retains special roles and ordinary authored allocations.
use cyrus_game::script::{ScriptActorSource, ScriptWorld};

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
        let world = ScriptWorld::prepare(&scripts, 1)
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
