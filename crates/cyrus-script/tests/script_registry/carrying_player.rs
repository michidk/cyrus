//! An actor's authored weapon-holding flag selects the ordinary armed controller.
use super::*;

#[test]
fn carrying_weapon_attribute_admits_armed_forward_control() {
    use cyrus_script::script::{OrdinaryGroundQuery, PlayerControllerInput, ScriptInputs};
    for (walk_mode, walk, expected) in [
        (false, false, AnimationGroup::FIGHT_FORW),
        (true, false, AnimationGroup::FIGHT_FORW),
        (true, true, AnimationGroup::FIGHT_RUN),
    ] {
        let (_, mut world, data, mut scene) = player_walking_fixture_with_animation(
            |scripts| {
                scripts.actors.get_mut("CYRUS").unwrap().attributes.extend([
                    (Attribute::HoldingWeapon, 1),
                    (Attribute::Weapon, 1),
                    (Attribute::FightForwardSpeed, 24),
                    (Attribute::FightForwardWalk, 8),
                ]);
            },
            |data| {
                let groups = data.metadata["actors"]
                    .as_array_mut()
                    .unwrap()
                    .iter_mut()
                    .find(|actor| actor["script_name"] == "CYRUS")
                    .unwrap()["animation_groups"]
                    .as_array_mut()
                    .unwrap();
                for group in [8, 31, 92] {
                    groups.push(serde_json::json!({
                        "group_index":group,"anim_id":1,"anim_type":0,
                        "commands":[{"opcode":0,"frame_reference":0},{"opcode":1}]
                    }));
                }
            },
        );
        scene.settings.tap_time = Some(6);
        scene.settings.sheath_sword_delay = Some(100);
        scene.settings.defend_interval = Some(100);
        let player = world.selected_player().unwrap();
        let map = world.id(0).unwrap();
        let prepared = world
            .prepare_ordinary_collision_query(player, &scene, &data, [10, 30], &[player, map])
            .unwrap();
        let query = OrdinaryGroundQuery {
            scene: &scene,
            animations: &data,
            prepared: &prepared,
            distance: 4,
            sidedness: false,
        };
        let hud = cyrus_script::script::ScriptHud::default();
        hud.configure(
            &inventory_asset(&[]),
            &cyrus_game::preferences::Preferences::default(),
        );
        let held = cyrus_script::player::input::PlayerInput {
            forward: true,
            walk,
            ..Default::default()
        };
        world.begin_player_movement_update(player, 0).unwrap();
        world
            .advance_player_controller(
                player,
                query,
                cyrus_script::clock::ClockStep {
                    frame_scale: 256,
                    nominal_ticks: 0,
                    fine_ticks: 0,
                },
                ScriptInputs {
                    player_controller: Some(PlayerControllerInput {
                        held: &held,
                        walk_mode,
                        auto_defend: false,
                        steer: None,
                        claim: None,
                        probes: None,
                    }),
                    hud: Some(&hud),
                    shell_execution: Some(false),
                    nominal_ticks: Some(0),
                    frame_scale: Some(256),
                    forward_step_clear: Some(true),
                    locomotion_geometry: Some(cyrus_script::script::LocomotionGeometry {
                        ground: query,
                        environment: cyrus_script::script::ObstacleEnvironment {
                            selected_player: Some(player),
                            auto_grab: false,
                            item_system_ready: true,
                            shell_execution: false,
                            ticks: 0,
                        },
                    }),
                    ..Default::default()
                },
                &mut cyrus_game::random::EngineRandom::from_state(42),
                &mut |_, _| Ok(()),
            )
            .unwrap();
        world.finish_player_movement_update(player).unwrap();
        let state = world.inspect(player.index, 1);
        let animation = &state["actors"][0]["animation"];
        assert!(
            animation["current_group"] == expected.index()
                || animation["pending_group"] == expected.index(),
            "{animation}"
        );
        assert_eq!(
            world.actors()[player.index].attributes()[Attribute::HoldingWeapon],
            1
        );
        assert_eq!(
            world.actors()[player.index]
                .control_state()
                .unwrap()
                .motion_direction,
            if walk {
                MotionDirection::WALK_FORWARD
            } else {
                MotionDirection::FORWARD
            }
        );
    }
}
