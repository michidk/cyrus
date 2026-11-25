use super::*;

#[test]
fn ordinary_slide_direction_boundary_and_scaled_turn_rate() {
    use cyrus_script::script::{
        OrdinarySlide, OrdinarySlopeTurn, ordinary_slide, ordinary_slope_turn,
    };
    assert_eq!(
        ordinary_slide([0, -100, 200], 511, false, 36, 256),
        OrdinarySlide {
            movement_heading: 0,
            target_heading: 0,
            group: AnimationGroup::SLIDE,
            step: StepDirection::Forward,
            distance: 4608,
        }
    );
    assert_eq!(
        ordinary_slide([0, -100, 200], 512, true, 36, 128),
        OrdinarySlide {
            movement_heading: 0,
            target_heading: 1024,
            group: AnimationGroup::SLIDE_BACKWARD_TORCH,
            step: StepDirection::Backward,
            distance: 2304,
        }
    );
    assert_eq!(
        ordinary_slide([0, -100, 200], 2047, true, -1, 256).distance,
        1024
    );
    assert_eq!(ordinary_slide([0, -100, 200], 0, false, 36, 0).distance, 0);
    assert_eq!(
        ordinary_slope_turn(0, 100, 12, 128, false, false),
        OrdinarySlopeTurn {
            heading: 8,
            group: Some(AnimationGroup::TURN_LEFT)
        }
    );
    assert_eq!(
        ordinary_slope_turn(2044, 4, 12, 128, false, false),
        OrdinarySlopeTurn {
            heading: 4,
            group: None
        }
    );
    assert_eq!(
        ordinary_slope_turn(0, 1024, 12, 256, true, false),
        OrdinarySlopeTurn {
            heading: 12,
            group: Some(AnimationGroup::FIGHT_TURN_LEFT)
        }
    );
    assert_eq!(
        ordinary_slope_turn(1024, 0, 12, 256, false, true),
        OrdinarySlopeTurn {
            heading: 1012,
            group: Some(AnimationGroup::TURN_RIGHT_TORCH)
        }
    );
}

#[test]
fn ordinary_static_model_slope_continues_without_an_animation_receiver() {
    use cyrus_collision::ground::{GroundSupport, SelectedSupport, SupportSource};
    use cyrus_script::script::{OrdinaryGroundQuery, OrdinarySlopeInputs};
    let (mut world, data, models) = static_model_fixture(cache());
    let id = world.id(0).unwrap();
    let other = world.id(1).unwrap();
    assert_eq!(world.animation_frame(id).unwrap(), None);
    world.refresh_model_presentations(&data, &[other]).unwrap();
    let mut scene = ordinary_collision_scene(world.world, models);
    scene.settings.slide_speed = Some(8);
    world
        .prepare_ordinary_collision_shape(id, &data, &scene)
        .unwrap();
    let prepared = world
        .prepare_ordinary_collision_query(id, &scene, &data, [10, 30], &[id, other])
        .unwrap();
    let support = SelectedSupport::Primary {
        source: SupportSource::Terrain,
        support: GroundSupport {
            height: 20.0,
            normal: [0, -100, 200],
        },
    };
    let query = || OrdinaryGroundQuery {
        scene: &scene,
        animations: &data,
        prepared: &prepared,
        distance: 50,
        sidedness: false,
    };
    let inputs = || OrdinarySlopeInputs {
        selected_player: None,
        holding_weapon: false,
        combat_active: false,
        frame_scale: 64,
        ticks: 0,
    };
    for distance in [256, 512] {
        world
            .resolve_ordinary_slope(id, Some(&support), query(), inputs(), &mut |_, _| {
                panic!("static slope must not emit animation sound")
            })
            .expect("static model must complete slope response without an animation receiver");
        assert_eq!(
            world.spatial(id).unwrap().local.position,
            [2560, 4608, 7680 + distance]
        );
        assert_eq!(world.model_support(id).unwrap().actor, Some(other));
        let control = world.actors()[0].control_state().unwrap();
        assert_eq!(
            (
                control.slide_flag,
                control.sliding_distance,
                control.move_counter
            ),
            (1, distance, 0x30)
        );
        assert_eq!(world.animation_frame(id).unwrap(), None);
    }
    let before = world.spatial(id).unwrap().local;
    world
        .resolve_ordinary_slope(id, None, query(), inputs(), &mut |_, _| {
            panic!("static fallback must not emit animation sound")
        })
        .unwrap();
    assert_eq!(world.spatial(id).unwrap().local.position, before.position);
    assert_eq!(
        world.actors()[0].control_state().unwrap().sliding_distance,
        512
    );
}

#[test]
fn ordinary_slope_updates_shared_animation_then_requeries_retained_support() {
    assert_ordinary_slope_animation_switch(1);
}

#[test]
fn ordinary_slope_respects_disabled_and_noncanonical_running_animation_switches() {
    assert_ordinary_slope_animation_switch(0);
    assert_ordinary_slope_animation_switch(2);
}

fn assert_ordinary_slope_animation_switch(state: u32) {
    use cyrus_collision::ground::{GroundSupport, SelectedSupport, SupportSource};
    use cyrus_game::cheats::{CheatStore, Cheats};
    use cyrus_script::script::{OrdinaryGroundQuery, OrdinarySlopeInputs};
    struct Memory;
    impl CheatStore for Memory {
        fn load(&self) -> anyhow::Result<cyrus_assets::cheats::CheatFile> {
            Ok(cyrus_assets::cheats::empty())
        }
        fn save(&self, _: &cyrus_assets::cheats::CheatFile) -> anyhow::Result<()> {
            Ok(())
        }
    }
    let mut cheats = Cheats::new(Memory);
    cheats.execute("animation", Some(state)).unwrap();
    let mut scripts = cache();
    scripts
        .actors
        .insert("SUPPORT".into(), scripts.actors["LOGIC"].clone());
    scripts.instances[1].actor = "SUPPORT".into();
    let (mut world, mut data, models) = static_model_fixture(scripts);
    let id = world.id(0).unwrap();
    let other = world.id(1).unwrap();
    data.metadata["actors"][0]["model_load_path"] = "animation_table".into();
    data.metadata["actors"][0]["animation_groups"] = serde_json::json!(
        [0, 14, 15, 16, 17, 75, 76, 77, 78].map(|group| serde_json::json!({
            "group_index":group,"anim_id":group+1,"anim_type":0,
            "commands":[{"opcode":0,"frame_reference":group},{"opcode":2,"value":0}]
        }))
    );
    world
        .initialize_map_animation(id, &data, 0, &mut |_, _| Ok(()))
        .unwrap();
    world
        .advance(
            id,
            cyrus_script::script::ScriptStage::Animation,
            cyrus_script::script::ScriptInputs {
                nominal_ticks: Some(0),
                frame_scale: Some(256),
                animation_system: Some(cheats.animation_system().unwrap()),
                animation_running: Some(cheats.animation_running().unwrap()),
                ..Default::default()
            },
            None,
            None,
            20,
        )
        .unwrap();
    world.refresh_model_presentations(&data, &[other]).unwrap();
    let mut scene = ordinary_collision_scene(world.world, models);
    scene.settings.slide_speed = Some(8);
    world
        .prepare_ordinary_collision_shape(id, &data, &scene)
        .unwrap();
    let prepared = world
        .prepare_ordinary_collision_query(id, &scene, &data, [10, 30], &[id, other])
        .unwrap();
    let query = || OrdinaryGroundQuery {
        scene: &scene,
        animations: &data,
        prepared: &prepared,
        distance: 50,
        sidedness: false,
    };
    let inputs = |selected_player| OrdinarySlopeInputs {
        selected_player,
        holding_weapon: false,
        combat_active: false,
        frame_scale: 64,
        ticks: 0,
    };
    let support = SelectedSupport::Primary {
        source: SupportSource::Terrain,
        support: GroundSupport {
            height: 20.0,
            normal: [0, -100, 200],
        },
    };
    world
        .resolve_ordinary_slope(
            id,
            Some(&support),
            query(),
            inputs(None),
            &mut |_, _| Ok(()),
        )
        .unwrap();
    assert_eq!(
        world.spatial(id).unwrap().local.position,
        [2560, 1536, 7936]
    );
    assert_eq!(
        world.animation_frame(id).unwrap(),
        Some(if state == 0 { 0 } else { 75 }),
        "animation switch {state} must gate requests while slope translation continues"
    );
    assert_eq!(world.model_support(id).unwrap().actor, Some(other));
    let control = world.actors()[0].control_state().unwrap();
    assert_eq!(
        (
            control.slide_flag,
            control.sliding_distance,
            control.move_counter
        ),
        (1, 256, 0x30)
    );
    world
        .resolve_ordinary_slope(
            id,
            Some(&support),
            query(),
            inputs(None),
            &mut |_, _| Ok(()),
        )
        .unwrap();
    assert_eq!(
        world.spatial(id).unwrap().local.position,
        [2560, 1536, 8192]
    );
    assert_eq!(
        world.actors()[0].control_state().unwrap().sliding_distance,
        512
    );
    world
        .resolve_ordinary_slope(id, None, query(), inputs(None), &mut |_, _| Ok(()))
        .unwrap();
    assert_eq!(
        world.spatial(id).unwrap().local.position,
        [2560, 1536, 8192]
    );
    assert_eq!(
        world.actors()[0].control_state().unwrap().sliding_distance,
        512
    );

    // A backward slope turns the selected player and publishes its orientation immediately.
    let backward = SelectedSupport::Primary {
        source: SupportSource::Terrain,
        support: GroundSupport {
            height: 20.0,
            normal: [200, -100, -200],
        },
    };
    let before = world.spatial(id).unwrap().local.angles;
    world
        .resolve_ordinary_slope(
            id,
            Some(&backward),
            query(),
            inputs(Some(id)),
            &mut |_, _| Ok(()),
        )
        .unwrap();
    let spatial = world.spatial(id).unwrap();
    assert_ne!(spatial.local.angles, before);
    assert_eq!(spatial.angle_history, spatial.local.angles);
    assert_eq!(
        spatial.local.matrix,
        cyrus_world::transform::Matrix::orientation(spatial.local.wrapped_angles())
    );
    if state == 0 {
        cheats.execute("animation", Some(1)).unwrap();
        assert_eq!(world.animation_frame(id).unwrap(), Some(0));
        world
            .advance(
                id,
                cyrus_script::script::ScriptStage::Animation,
                cyrus_script::script::ScriptInputs {
                    nominal_ticks: Some(0),
                    frame_scale: Some(256),
                    animation_system: Some(cheats.animation_system().unwrap()),
                    animation_running: Some(cheats.animation_running().unwrap()),
                    ..Default::default()
                },
                None,
                None,
                20,
            )
            .unwrap();
        world
            .resolve_ordinary_slope(
                id,
                Some(&support),
                query(),
                inputs(None),
                &mut |_, _| Ok(()),
            )
            .unwrap();
        assert_eq!(world.animation_frame(id).unwrap(), Some(75));
    }
    eprintln!(
        "slope switch={state}: first z=7936, second z=8192, retained distance=512, turn published, final frame={:?}",
        world.animation_frame(id).unwrap()
    );
}

#[test]
fn static_slide_termination_applies_hurt_and_continues_without_animation() {
    use cyrus_script::script::{OrdinaryInputs, ScriptInputs};
    for animation_system in [false, true] {
        for distance in [256, 257] {
            let mut scripts = cache();
            scripts.updates.animation_system = animation_system;
            scripts.actors.get_mut("LOGIC").unwrap().attributes.extend([
                (Attribute::Collide, 0),
                (Attribute::FallDamage, 1),
                (Attribute::Health, 100),
            ]);
            for placement in &mut scripts.instances {
                placement.active = 1;
                placement.object_type = PlacementType::MODEL;
                placement.map_position = [10, 20, 30];
            }
            scripts.content_id = scripts.content_identity().unwrap();
            let (world, data, models) = static_model_fixture(scripts.clone());
            let mut saved = movement_snapshot(&world, &scripts, &data);
            saved["actors"][0]["control"]["sliding_state"] = 1.into();
            saved["actors"][0]["control"]["sliding_distance"] = distance.into();
            let mut world = restore_navigation_owner(saved, &scripts, &data).unwrap();
            let id = world.id(0).unwrap();
            let ids = [id, world.id(1).unwrap()];
            world.refresh_model_presentations(&data, &ids).unwrap();
            let before = world.spatial(id).unwrap().local.position;
            let mut scene = ordinary_collision_scene(world.world, models);
            scene.settings.post_collide_height = 50;
            scene.settings.slide_hurt = Some((1, 7));
            let mut sounds = Vec::new();
            for update in 1..=2 {
                world
                    .update_ordinary(
                        &data,
                        [10, 30],
                        OrdinaryInputs {
                            scripts: ScriptInputs {
                                nominal_ticks: Some(1),
                                fine_ticks: Some(1),
                                frame_scale: Some(256),
                                ..Default::default()
                            },
                            scene: Some(&scene),
                        },
                        &mut cyrus_game::random::EngineRandom::default(),
                        &mut |actor, sound| {
                            sounds.push((actor, sound));
                            Ok(())
                        },
                        100,
                    )
                    .unwrap();
                let actor = &world.actors()[0];
                assert!(actor.failure().is_none(), "{:?}", actor.failure());
                assert_eq!(actor.control_state().unwrap().slide_flag, 0);
                assert_eq!(
                    actor.attributes()[Attribute::Health],
                    if distance > 256 { 93 } else { 100 }
                );
                assert_eq!(actor.runtime().unwrap().variables(), [4 + update]);
                assert_eq!(world.animation_frame(id).unwrap(), None);
                let after = world.spatial(id).unwrap().local.position;
                assert_eq!(
                    [after[0], after[2]],
                    [before[0], before[2] - if distance > 256 { 2048 } else { 0 }]
                );
                eprintln!(
                    "static slide: animation_system={animation_system} distance={distance} update={update} position={after:?} health={} sliding={}",
                    actor.attributes()[Attribute::Health],
                    actor.control_state().unwrap().slide_flag
                );
            }
            assert!(sounds.is_empty());
        }
    }
}
