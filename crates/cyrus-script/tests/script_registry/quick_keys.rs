use super::*;

#[test]
fn quick_health_key_drinks_the_installed_health_potion_once() {
    use cyrus_script::script::{QuickKeys, ScriptInputs};
    let (_, mut world, _, _) = player_walking_fixture_with(|_| {});
    let player = world
        .id(world
            .actors()
            .iter()
            .position(|actor| {
                matches!(
                    actor.source,
                    cyrus_script::script::ScriptActorSource::Player
                )
            })
            .unwrap())
        .unwrap();
    let hud = cyrus_script::script::ScriptHud::default();
    hud.configure(
        &inventory_asset(&[(4, 1)]),
        &cyrus_game::preferences::Preferences::default(),
    );
    world
        .set_actor_attribute((player).index, (Attribute::Health).index(), 40)
        .unwrap();
    world.globals_mut().initialize_selected_health_for_test(40);
    let inputs = || ScriptInputs {
        hud: Some(&hud),
        shell_execution: Some(false),
        quick_keys: Some(QuickKeys {
            sword: false,
            health: true,
            use_selected: false,
        }),
        nominal_ticks: Some(1),
        frame_scale: Some(256),
        ..Default::default()
    };
    let mut random = cyrus_game::random::EngineRandom::default();
    world
        .run_quick_keys(inputs(), &mut random, &mut |_, _| Ok(()))
        .unwrap();
    assert_eq!(
        world.actors()[player.index].attributes()[Attribute::Health],
        55,
        "heals fifteen"
    );
    assert_eq!(
        hud.inventory(None).unwrap().counts[&4],
        0,
        "one potion drunk"
    );
    world
        .run_quick_keys(inputs(), &mut random, &mut |_, _| Ok(()))
        .unwrap();
    assert_eq!(
        world.actors()[player.index].attributes()[Attribute::Health],
        55,
        "nothing left to drink"
    );
}

fn selected_player_update(
    world: &mut ScriptWorld,
    data: &cyrus_assets::animation::Animations,
    scene: &cyrus_script::player::data::PlayerScene,
    cheats: (Option<bool>, Option<bool>),
    keys: Option<cyrus_script::script::ScriptKeys>,
) {
    let held = cyrus_script::player::input::PlayerInput::default();
    let hud = cyrus_script::script::ScriptHud::default();
    world
        .update_ordinary(
            data,
            [10, 30],
            cyrus_script::script::OrdinaryInputs {
                scene: Some(scene),
                scripts: cyrus_script::script::ScriptInputs {
                    hud: Some(&hud),
                    nominal_ticks: Some(1),
                    frame_scale: Some(256),
                    fine_ticks: Some(1),
                    step_fps: Some(30),
                    shell_execution: Some(false),
                    player_controller: Some(cyrus_script::script::PlayerControllerInput {
                        held: &held,
                        walk_mode: true,
                        auto_defend: false,
                        steer: None,
                        claim: None,
                        probes: None,
                    }),
                    ground_bypass: cheats.0,
                    magic_carpet: cheats.1,
                    script_keys: keys,
                    ..Default::default()
                },
            },
            &mut cyrus_game::random::EngineRandom::default(),
            &mut |_, _| Ok(()),
            100,
        )
        .unwrap();
}

#[test]
fn the_home_toggle_flips_once_per_press_while_yeahbaby_is_on_and_persists() {
    use cyrus_script::script::ScriptKeys;
    let (_, mut world, data, scene) =
        player_walking_fixture_with(|scripts| scripts.updates.final_version = true);
    let home = |pressed| {
        Some(ScriptKeys {
            home_pressed: pressed,
            ..Default::default()
        })
    };
    // (cheat state, Home pressed, toggle afterwards): on and pressed flips; on and not pressed
    // keeps; off (or absent) ignores the key and keeps the state.
    for (cheat, pressed, toggle) in [
        (Some(true), true, true),
        (Some(true), false, true),
        (Some(true), true, false),
        (Some(false), true, false),
        (Some(true), true, true),
        (Some(false), true, true),
        (None, true, true),
        (Some(true), true, false),
    ] {
        selected_player_update(&mut world, &data, &scene, (cheat, None), home(pressed));
        assert_eq!(world.ground_bypass_toggle(), toggle, "{cheat:?} {pressed}");
    }
}

#[test]
fn the_ground_bypass_refuses_unknown_keys_and_clears_the_falling_byte_when_taken() {
    use cyrus_script::script::ScriptKeys;
    let (_, mut world, data, scene) =
        player_walking_fixture_with(|scripts| scripts.updates.final_version = true);
    let player = world.selected_player().unwrap();
    // Unknown key state is refused, not read as released.
    selected_player_update(&mut world, &data, &scene, (Some(true), None), None);
    let failure = world.actors()[player.index].failure().expect("refused");
    assert!(failure.contains("debug navigation keys"), "{failure}");
    // Held Page Up takes the bypass: the falling byte clears and the update does not fail.
    let (_, mut world, data, scene) =
        player_walking_fixture_with(|scripts| scripts.updates.final_version = true);
    let player = world.selected_player().unwrap();
    let y = world.spatial(player).unwrap().local.position[1];
    world.set_falling_for_test(player, JumpMotion::STANDING, y);
    let page_up = Some(ScriptKeys {
        page_up: true,
        ..Default::default()
    });
    selected_player_update(&mut world, &data, &scene, (Some(true), None), page_up);
    let actor = &world.actors()[player.index];
    assert!(actor.failure().is_none(), "{:?}", actor.failure());
    let control = actor.control_state().unwrap();
    assert_eq!(control.falling_state, JumpMotion::NONE);
    assert_eq!(control.living_state, LivingState::ORDINARY);
}

#[test]
fn the_magic_carpet_clears_the_falling_byte_and_ends_the_selected_players_probe() {
    let (_, mut world, data, scene) =
        player_walking_fixture_with(|scripts| scripts.updates.final_version = true);
    let player = world.selected_player().unwrap();
    let y = world.spatial(player).unwrap().local.position[1];
    world.set_falling_for_test(player, JumpMotion::STANDING, y);
    selected_player_update(&mut world, &data, &scene, (None, Some(true)), None);
    let actor = &world.actors()[player.index];
    assert!(actor.failure().is_none(), "{:?}", actor.failure());
    assert_eq!(
        actor.control_state().unwrap().falling_state,
        JumpMotion::NONE
    );
}

#[test]
fn developer_mode_enables_the_ground_bypass_keys_without_the_cheat_but_only_with_known_keys() {
    use cyrus_script::script::ScriptKeys;
    // The fixture's `final_version` is clear: developer behaviour.
    let (_, mut world, data, scene) = player_walking_fixture_with(|_| {});
    let player = world.selected_player().unwrap();
    let home = Some(ScriptKeys {
        home_pressed: true,
        ..Default::default()
    });
    selected_player_update(&mut world, &data, &scene, (None, None), home);
    assert!(world.ground_bypass_toggle());
    // Without key state the developer-mode bypass stays inactive and nothing is refused.
    selected_player_update(&mut world, &data, &scene, (None, None), None);
    assert!(world.ground_bypass_toggle());
    assert!(world.actors()[player.index].failure().is_none());
}

#[test]
fn page_up_and_page_down_move_the_selected_player_by_sixteen_map_units_while_yeahbaby_is_on() {
    use cyrus_script::script::ScriptKeys;
    // (cheat, Page Up, Page Down, Y change in Q8): each held key moves once per update; both held
    // cancel; the cheat off leaves the keys without effect.
    for (cheat, up, down, change) in [
        (Some(true), true, false, Some(-0x1000)),
        (Some(true), false, true, Some(0x1000)),
        (Some(true), true, true, Some(0)),
        (Some(false), true, false, None),
        (None, false, true, None),
    ] {
        let (_, mut world, data, scene) =
            player_walking_fixture_with(|scripts| scripts.updates.final_version = true);
        let player = world.selected_player().unwrap();
        let before = world.spatial(player).unwrap().local.position[1];
        let keys = Some(ScriptKeys {
            page_up: up,
            page_down: down,
            ..Default::default()
        });
        selected_player_update(&mut world, &data, &scene, (cheat, None), keys);
        let after = world.spatial(player).unwrap().local.position[1];
        let actor = &world.actors()[player.index];
        assert!(actor.failure().is_none(), "{:?}", actor.failure());
        match change {
            // The held key also takes the ground bypass, so no support result moves the actor.
            Some(change) => assert_eq!(after, before + change, "{cheat:?} {up} {down}"),
            None => assert_ne!(after, before - 0x1000, "{cheat:?} {up} {down}"),
        }
    }
}

#[test]
fn insert_suspends_the_camera_limiter_and_steps_forward_until_it_is_released() {
    use cyrus_script::script::ScriptKeys;
    let (_, mut world, data, scene) =
        player_walking_fixture_with(|scripts| scripts.updates.final_version = true);
    let player = world.selected_player().unwrap();
    let limiter_off = |world: &ScriptWorld| {
        let camera = world.camera().unwrap();
        camera.records[camera.selected.index()]
            .flags
            .contains(cyrus_script::script::camera::CameraFlags::UNLIMITED_MOVEMENT)
    };
    assert!(!limiter_off(&world));
    let insert = Some(ScriptKeys {
        insert: true,
        ..Default::default()
    });
    // Held with the cheat on: the first update suspends the limiter, every update steps forward
    // by the frame scale times 128 Q8 (32768 for the nominal scale of 256).
    for held in 0..2 {
        let before = world.spatial(player).unwrap().local;
        selected_player_update(&mut world, &data, &scene, (Some(true), None), insert);
        let expected = cyrus_world::transform::translate_forward(
            before.position,
            before.wrapped_angles()[1],
            32768,
        )
        .unwrap();
        let after = world.spatial(player).unwrap().local.position;
        assert_eq!(
            (after[0], after[2]),
            (expected[0], expected[2]),
            "held {held}"
        );
        assert!(limiter_off(&world));
        assert!(world.yeahbaby_insert_active());
    }
    // Released with the cheat still on: the limiter is restored once and nothing steps.
    let before = world.spatial(player).unwrap().local.position;
    selected_player_update(
        &mut world,
        &data,
        &scene,
        (Some(true), None),
        Some(ScriptKeys::default()),
    );
    let after = world.spatial(player).unwrap().local.position;
    assert_eq!((after[0], after[2]), (before[0], before[2]));
    assert!(!limiter_off(&world));
    assert!(!world.yeahbaby_insert_active());
    // Turning the cheat off while Insert is held restores the limiter and does not step.
    selected_player_update(&mut world, &data, &scene, (Some(true), None), insert);
    assert!(limiter_off(&world));
    let before = world.spatial(player).unwrap().local.position;
    selected_player_update(&mut world, &data, &scene, (Some(false), None), insert);
    let after = world.spatial(player).unwrap().local.position;
    assert_eq!((after[0], after[2]), (before[0], before[2]));
    assert!(!limiter_off(&world));
    assert!(!world.yeahbaby_insert_active());
}
