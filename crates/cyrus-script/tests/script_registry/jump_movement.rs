//! Held jump must not install a locomotion request behind a ledge-owned animation.
use super::*;

#[test]
fn held_jump_preserves_the_low_climb_pending_owner() {
    use cyrus_script::script::{
        OrdinaryGroundQuery, PlayerControllerInput, ScriptInputs, WorldRestoreContext,
        WorldSnapshotSources,
    };
    let (scripts, mut world, mut data, scene) = player_walking_fixture_with_animation(
        |_| {},
        |data| {
            let groups = data.metadata["actors"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|actor| actor["script_name"] == "CYRUS")
                .unwrap()["animation_groups"]
                .as_array_mut()
                .unwrap();
            groups.push(serde_json::json!({
                "group_index":AnimationGroup::HAND_PLANT_LEDGE.index(),
                "anim_id":1,"anim_type":0,
                "commands":[{"opcode":0,"frame_reference":0},{"opcode":2,"value":0}]
            }));
        },
    );
    data.metadata["markers"] = serde_json::json!([]);
    data.metadata["walk_node_maps"] = serde_json::json!([]);
    let index = world.selected_player().unwrap().index;
    let mut saved = movement_snapshot(&world, &scripts, &data);
    saved["actors"][index]["control"]["living_state"] = LivingState::LOW_CLIMB.byte().into();
    saved["actors"][index]["animation"]["playback"]["current"][0] =
        AnimationGroup::HAND_PLANT_LEDGE.index().into();
    // The ordinary update requests the default before entering PlayerMain.
    saved["actors"][index]["animation"]["playback"]["pending"] =
        serde_json::json!([AnimationGroup::PANIC.index(), 0]);
    let globals = std::mem::take(world.globals_mut());
    let mut world = ScriptWorld::from_snapshot(
        serde_json::from_value(saved).unwrap(),
        &WorldSnapshotSources {
            identity_memo: None,
            effects: None,
            player: None,
            scripts: &scripts,
            animations: &data,
            collision: &Default::default(),
        },
        WorldRestoreContext {
            generation: world.generation() + 1,
            globals,
            samples: &BTreeMap::new(),
        },
    )
    .unwrap();
    let player = world.id(index).unwrap();
    let map = world.id(0).unwrap();
    let before = world.spatial(player).unwrap().local.position;
    let prepared = world
        .prepare_ordinary_collision_query(player, &scene, &data, [10, 30], &[player, map])
        .unwrap();
    let hud = cyrus_script::script::ScriptHud::default();
    hud.configure(
        &inventory_asset(&[]),
        &cyrus_game::preferences::Preferences::default(),
    );
    for jump in [true, false] {
        world.begin_player_movement_update(player, 0).unwrap();
        world
            .advance_player_controller(
                player,
                OrdinaryGroundQuery {
                    scene: &scene,
                    animations: &data,
                    prepared: &prepared,
                    distance: 4,
                    sidedness: false,
                },
                cyrus_script::clock::ClockStep {
                    frame_scale: 256,
                    nominal_ticks: 0,
                    fine_ticks: 0,
                },
                ScriptInputs {
                    player_controller: Some(PlayerControllerInput {
                        held: &cyrus_script::player::input::PlayerInput {
                            forward: true,
                            jump,
                            ..Default::default()
                        },
                        walk_mode: false,
                        auto_defend: false,
                        steer: None,
                        claim: None,
                        probes: None,
                    }),
                    hud: Some(&hud),
                    shell_execution: Some(false),
                    nominal_ticks: Some(0),
                    fine_ticks: Some(0),
                    frame_scale: Some(256),
                    forward_step_clear: Some(true),
                    ..Default::default()
                },
                &mut cyrus_game::random::EngineRandom::default(),
                &mut |_, _| Ok(()),
            )
            .unwrap();
        world.finish_player_movement_update(player).unwrap();
        let state = world.inspect(player.index, 1);
        let animation = &state["actors"][0]["animation"];
        assert_eq!(
            animation["current_group"],
            AnimationGroup::HAND_PLANT_LEDGE.index()
        );
        assert_eq!(
            animation["pending_group"],
            if jump {
                AnimationGroup::PANIC
            } else {
                AnimationGroup::RUN
            }
            .index(),
            "{animation}"
        );
        assert_eq!(world.spatial(player).unwrap().local.position, before);
        assert_eq!(
            world.actors()[player.index]
                .control_state()
                .unwrap()
                .living_state,
            LivingState::LOW_CLIMB
        );
    }
}
