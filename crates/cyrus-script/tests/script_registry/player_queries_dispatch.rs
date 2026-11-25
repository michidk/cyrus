use super::*;

#[test]
fn player_looking_uses_signed_tolerance_and_quantized_position_bearing() {
    use cyrus_script::script::{PlayerQuery, QueryInputs};
    let mut world = ScriptWorld::prepare(&cache(), 1).unwrap();
    world.prepare_spatial().unwrap();
    let mut actor = world.spatial(world.id(0).unwrap()).unwrap().clone();
    actor.local.position = [0; 3];
    for (position, heading, tolerance, expected) in [
        ([0, 900, 100], 1024, 0, 1),
        ([0, -900, 100], 1023, 0, 0),
        ([0, 0, 100], 1023, 1, 1),
        ([100, 0, 100], 768, 0, 1),
        ([100, 0, 100], 770, 1, 0),
        ([0, 0, 100], 1024, -1, 0),
        ([i32::MIN, 0, 1], 1024, 0, 1),
        ([i32::MIN, 0, i32::MIN], 0, 0, 1),
    ] {
        let inputs = QueryInputs {
            player: Some(PlayerQuery { position, heading }),
            distance_override: None,
            fallback_position: None,
            linked_position: Some(None),
        };
        assert_eq!(
            actor
                .query(
                    Native::PlayerLooking,
                    &[Argument::Integer(tolerance)],
                    inputs
                )
                .unwrap(),
            Some(expected),
            "{position:?}/{heading}/{tolerance}"
        );
    }
}

#[test]
fn player_looking_preserves_attached_then_linked_position_priority() {
    use cyrus_script::script::{PlayerQuery, QueryInputs};
    let mut world = ScriptWorld::prepare(&cache(), 1).unwrap();
    world.prepare_spatial().unwrap();
    let mut actor = world.spatial(world.id(0).unwrap()).unwrap().clone();
    actor.local.position = [100, 0, 0];
    let mut inputs = QueryInputs {
        player: Some(PlayerQuery {
            position: [0, 0, 100],
            heading: 1024,
        }),
        distance_override: None,
        fallback_position: None,
        linked_position: Some(Some([0; 3])),
    };
    let args = [Argument::Integer(0)];
    assert_eq!(
        actor.query(Native::PlayerLooking, &args, inputs).unwrap(),
        Some(1)
    );
    inputs.linked_position = Some(None);
    assert_eq!(
        actor.query(Native::PlayerLooking, &args, inputs).unwrap(),
        Some(0)
    );
    inputs.linked_position = None;
    assert!(actor.query(Native::PlayerLooking, &args, inputs).is_err());
    actor.parent = Some(1);
    actor.effective = None;
    assert!(actor.query(Native::PlayerLooking, &args, inputs).is_err());
    let mut effective = actor.local;
    effective.position = [0; 3];
    actor.effective = Some(effective);
    assert_eq!(
        actor.query(Native::PlayerLooking, &args, inputs).unwrap(),
        Some(1)
    );
    inputs.player = None;
    assert!(actor.query(Native::PlayerLooking, &args, inputs).is_err());
    assert!(actor.query(Native::PlayerLooking, &[], inputs).is_err());
}

#[test]
fn player_looking_dispatch_returns_result_to_script_local() {
    use cyrus_script::script::{PlayerQuery, QueryInputs, ScriptInputs, ScriptStage};
    for (heading, expected) in [(1024, 1), (1023, 0)] {
        let scripts = random_scripts(
            Native::PlayerLooking,
            vec![Value::Integer { value: 0 }],
            Object::Me,
        );
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        world.prepare_spatial().unwrap();
        let id = world.id(0).unwrap();
        let origin = world.spatial(id).unwrap().local.position;
        world
            .advance(
                id,
                ScriptStage::Body(Thread::Main),
                ScriptInputs {
                    nominal_ticks: Some(1),
                    frame_scale: Some(256),
                    player_query: Some(QueryInputs {
                        player: Some(PlayerQuery {
                            position: [origin[0], origin[1], origin[2].wrapping_add(100)],
                            heading,
                        }),
                        distance_override: None,
                        fallback_position: None,
                        linked_position: None,
                    }),
                    ..Default::default()
                },
                None,
                None,
                20,
            )
            .unwrap();
        assert_eq!(world.actors()[0].runtime().unwrap().variables(), [expected]);
    }
}

#[test]
fn player_stand_reads_selected_player_support_without_changing_it() {
    use cyrus_script::script::ModelSupportOwners;
    for (support_index, receiver, expected) in [
        (Some(0), Object::Me, 1),
        (Some(1), Object::Me, 0),
        (None, Object::Me, 0),
        (Some(2), Object::Player, 1),
    ] {
        let mut scripts = random_scripts(Native::PlayerStand, vec![], receiver);
        scripts.definition.functions[1].param_count = 0;
        add_standalone_receivers(&mut scripts);
        let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
        let caller = world.id(0).unwrap();
        let player = world.id(2).unwrap();
        let support = ModelSupportOwners {
            actor: support_index.map(|index| world.id(index).unwrap()),
            static_placement: None,
        };
        world.set_model_support(player, support).unwrap();
        world.step(caller, Thread::Main, 20).unwrap();
        assert_eq!(world.actors()[0].runtime().unwrap().variables(), [expected]);
        assert_eq!(world.model_support(player).unwrap(), support);
    }
}

#[test]
fn player_stand_rejects_missing_player_and_arguments() {
    for (arguments, player, expected) in [
        (vec![], false, "selected player unavailable"),
        (
            vec![Value::Integer { value: 0 }],
            true,
            "requires no arguments",
        ),
    ] {
        let mut scripts = random_scripts(Native::PlayerStand, arguments, Object::Me);
        if !player {
            scripts.definition.functions[1].param_count = 0;
        }
        scripts.content_id = scripts.content_identity().unwrap();
        if player {
            add_standalone_receivers(&mut scripts);
        }
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        let id = world.id(0).unwrap();
        let error = world.step(id, Thread::Main, 20).unwrap_err();
        assert!(error.to_string().contains(expected), "{error:#}");
    }
}

#[test]
fn player_stand_validates_explicit_selected_identity() {
    use cyrus_script::script::{ActivationInputs, ModelSupportOwners, ScriptInputs, ScriptStage};
    for (selected_index, stale, expected_error) in [
        (2, false, None),
        (2, true, Some("stale script actor generation")),
        (
            3,
            false,
            Some("activation player is not the selected player"),
        ),
    ] {
        let mut scripts = random_scripts(Native::PlayerStand, vec![], Object::Me);
        scripts.definition.functions[1].param_count = 0;
        add_standalone_receivers(&mut scripts);
        let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
        let caller = world.id(0).unwrap();
        let mut selected = world.id(selected_index).unwrap();
        world
            .set_model_support(
                selected,
                ModelSupportOwners {
                    actor: Some(caller),
                    static_placement: None,
                },
            )
            .unwrap();
        if stale {
            selected.generation += 1;
        }
        let inputs = ActivationInputs {
            player: selected,
            actors: BTreeMap::new(),
            labels: BTreeMap::new(),
            lockout: 0,
            dialogue_open: false,
            lineup_result: 0,
            use_held: false,
            item_system_ready: false,
            shell_execution: false,
            render_disabled: false,
        };
        let result = world.advance(
            caller,
            ScriptStage::Body(Thread::Main),
            ScriptInputs {
                activation: Some(&inputs),
                frame_scale: Some(256),
                nominal_ticks: Some(1),
                ..Default::default()
            },
            None,
            None,
            20,
        );
        if let Some(expected) = expected_error {
            let error = result.unwrap_err();
            assert!(error.to_string().contains(expected), "{error:#}");
        } else {
            result.unwrap();
            assert_eq!(world.actors()[0].runtime().unwrap().variables(), [1]);
            assert_eq!(world.model_support(selected).unwrap().actor, Some(caller));
        }
    }
}

#[test]
fn player_arc_uses_receiver_heading_signed_offset_and_inclusive_tolerance() {
    use cyrus_script::script::{PlayerQuery, QueryInputs};
    let mut world = ScriptWorld::prepare(&cache(), 1).unwrap();
    world.prepare_spatial().unwrap();
    let mut actor = world.spatial(world.id(0).unwrap()).unwrap().clone();
    for (position, yaw, offset, tolerance, expected) in [
        ([0, 900, 100], 0, 0, 0, 1),
        ([100, 0, 0], 1536, 0, 0, 1),
        ([-100, 0, 0], 512, 0, 0, 1),
        ([0, 0, -100], 1024, 0, 0, 1),
        ([100, 0, 100], 1792, 0, 0, 1),
        ([0, 0, 100], 10, 10, 0, 1),
        ([0, 0, 100], 2047, 0, 1, 1),
        ([0, 0, 100], 2047, 0, 0, 0),
        ([0, 0, 100], 0, 0, -1, 0),
        ([0, 0, 100], 10, 0, 10, 1),
        ([0, 0, 100], 10, 0, 9, 0),
        ([0, 0, 100], 0, i32::MAX, 1, 1),
        ([0, 0, 100], 0, i32::MAX, 0, 0),
        ([0, 0, 100], 0, i32::MIN, 0, 1),
    ] {
        actor.local.position = position;
        actor.local.angles[1] = yaw;
        let inputs = QueryInputs {
            player: Some(PlayerQuery {
                position: [0, -900, 0],
                heading: 777,
            }),
            linked_position: Some(None),
            distance_override: None,
            fallback_position: None,
        };
        assert_eq!(
            actor
                .query(
                    Native::PlayerArc,
                    &[Argument::Integer(offset), Argument::Integer(tolerance)],
                    inputs
                )
                .unwrap(),
            Some(expected),
            "{position:?}/{yaw}/{offset}/{tolerance}"
        );
    }
}

#[test]
fn player_arc_distinguishes_linked_position_from_attached_heading() {
    use cyrus_script::script::{PlayerQuery, QueryInputs};
    let mut world = ScriptWorld::prepare(&cache(), 1).unwrap();
    world.prepare_spatial().unwrap();
    let mut actor = world.spatial(world.id(0).unwrap()).unwrap().clone();
    actor.local.position = [100, 0, 0];
    actor.local.angles[1] = 0;
    let args = [Argument::Integer(0), Argument::Integer(0)];
    let mut inputs = QueryInputs {
        player: Some(PlayerQuery {
            position: [0; 3],
            heading: 777,
        }),
        linked_position: Some(None),
        distance_override: None,
        fallback_position: None,
    };
    assert_eq!(
        actor.query(Native::PlayerArc, &args, inputs).unwrap(),
        Some(0)
    );
    inputs.linked_position = Some(Some([0, 0, 100]));
    assert_eq!(
        actor.query(Native::PlayerArc, &args, inputs).unwrap(),
        Some(1)
    );
    actor.parent = Some(1);
    let mut effective = actor.local;
    effective.position = [0, 0, -100];
    effective.angles[1] = 1024;
    actor.effective = Some(effective);
    assert_eq!(
        actor.query(Native::PlayerArc, &args, inputs).unwrap(),
        Some(1)
    );
    actor.effective = None;
    assert!(actor.query(Native::PlayerArc, &args, inputs).is_err());
    actor.parent = None;
    inputs.linked_position = None;
    assert!(actor.query(Native::PlayerArc, &args, inputs).is_err());
    inputs.linked_position = Some(None);
    inputs.player = None;
    assert!(actor.query(Native::PlayerArc, &args, inputs).is_err());
    assert!(actor.query(Native::PlayerArc, &args[..1], inputs).is_err());
}

#[test]
fn player_arc_dispatch_returns_receiver_relative_result() {
    use cyrus_script::script::{PlayerQuery, QueryInputs, ScriptInputs, ScriptStage};
    for (offset_delta, expected) in [(0, 1), (1, 0)] {
        let mut scripts = random_scripts(
            Native::PlayerArc,
            vec![Value::Integer { value: 0 }, Value::Integer { value: 0 }],
            Object::Me,
        );
        scripts.definition.functions[1].param_count = 2;
        scripts.instances[0].angles[1] = offset_delta;
        scripts.content_id = scripts.content_identity().unwrap();
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        world.prepare_spatial().unwrap();
        let id = world.id(0).unwrap();
        let origin = world.spatial(id).unwrap().local.position;
        world
            .advance(
                id,
                ScriptStage::Body(Thread::Main),
                ScriptInputs {
                    frame_scale: Some(256),
                    nominal_ticks: Some(1),
                    player_query: Some(QueryInputs {
                        player: Some(PlayerQuery {
                            position: [origin[0], origin[1], origin[2].wrapping_sub(100)],
                            heading: 777,
                        }),
                        linked_position: None,
                        distance_override: None,
                        fallback_position: None,
                    }),
                    ..Default::default()
                },
                None,
                None,
                20,
            )
            .unwrap();
        assert_eq!(world.actors()[0].runtime().unwrap().variables(), [expected]);
    }
}

#[test]
fn player_looking_facing_and_arc_match_the_original_routines() {
    use cyrus_script::script::{PlayerQuery, QueryInputs};
    // Rows from the original routines in an emulator: function, receiver heading, player heading,
    // player offset X and Z, first and second argument (PlayerLooking takes only the second),
    // result.
    type Row = (String, u16, u16, i32, i32, i32, i32, i32);
    let rows: Vec<Row> = serde_json::from_str(
        r#"[
["looking",0,0,0,5000,0,100,0],
["looking",0,0,0,5000,256,200,0],
["looking",0,0,0,5000,1024,50,0],
["looking",0,0,5000,0,0,100,0],
["looking",0,0,5000,0,256,200,0],
["looking",0,0,5000,0,1024,50,0],
["looking",0,0,0,-5000,0,100,1],
["looking",0,0,0,-5000,256,200,1],
["looking",0,0,0,-5000,1024,50,1],
["looking",0,0,-5000,0,0,100,0],
["looking",0,0,-5000,0,256,200,0],
["looking",0,0,-5000,0,1024,50,0],
["looking",0,0,3000,4000,0,100,0],
["looking",0,0,3000,4000,256,200,0],
["looking",0,0,3000,4000,1024,50,0],
["looking",0,0,-3000,4000,0,100,0],
["looking",0,0,-3000,4000,256,200,0],
["looking",0,0,-3000,4000,1024,50,0],
["looking",0,0,4000,-3000,0,100,0],
["looking",0,0,4000,-3000,256,200,0],
["looking",0,0,4000,-3000,1024,50,0],
["looking",0,0,-2500,-2500,0,100,0],
["looking",0,0,-2500,-2500,256,200,0],
["looking",0,0,-2500,-2500,1024,50,0],
["looking",0,1000,0,5000,0,100,1],
["looking",0,1000,0,5000,256,200,1],
["looking",0,1000,0,5000,1024,50,1],
["looking",0,1000,5000,0,0,100,0],
["looking",0,1000,5000,0,256,200,0],
["looking",0,1000,5000,0,1024,50,0],
["looking",0,1000,0,-5000,0,100,0],
["looking",0,1000,0,-5000,256,200,0],
["looking",0,1000,0,-5000,1024,50,0],
["looking",0,1000,-5000,0,0,100,0],
["looking",0,1000,-5000,0,256,200,0],
["looking",0,1000,-5000,0,1024,50,0],
["looking",0,1000,3000,4000,0,100,0],
["looking",0,1000,3000,4000,256,200,1],
["looking",0,1000,3000,4000,1024,50,0],
["looking",0,1000,-3000,4000,0,100,0],
["looking",0,1000,-3000,4000,256,200,0],
["looking",0,1000,-3000,4000,1024,50,0],
["looking",0,1000,4000,-3000,0,100,0],
["looking",0,1000,4000,-3000,256,200,0],
["looking",0,1000,4000,-3000,1024,50,0],
["looking",0,1000,-2500,-2500,0,100,0],
["looking",0,1000,-2500,-2500,256,200,0],
["looking",0,1000,-2500,-2500,1024,50,0],
["looking",1024,0,0,5000,0,100,0],
["looking",1024,0,0,5000,256,200,0],
["looking",1024,0,0,5000,1024,50,0],
["looking",1024,0,5000,0,0,100,0],
["looking",1024,0,5000,0,256,200,0],
["looking",1024,0,5000,0,1024,50,0],
["looking",1024,0,0,-5000,0,100,1],
["looking",1024,0,0,-5000,256,200,1],
["looking",1024,0,0,-5000,1024,50,1],
["looking",1024,0,-5000,0,0,100,0],
["looking",1024,0,-5000,0,256,200,0],
["looking",1024,0,-5000,0,1024,50,0],
["looking",1024,0,3000,4000,0,100,0],
["looking",1024,0,3000,4000,256,200,0],
["looking",1024,0,3000,4000,1024,50,0],
["looking",1024,0,-3000,4000,0,100,0],
["looking",1024,0,-3000,4000,256,200,0],
["looking",1024,0,-3000,4000,1024,50,0],
["looking",1024,0,4000,-3000,0,100,0],
["looking",1024,0,4000,-3000,256,200,0],
["looking",1024,0,4000,-3000,1024,50,0],
["looking",1024,0,-2500,-2500,0,100,0],
["looking",1024,0,-2500,-2500,256,200,0],
["looking",1024,0,-2500,-2500,1024,50,0],
["looking",1024,1000,0,5000,0,100,1],
["looking",1024,1000,0,5000,256,200,1],
["looking",1024,1000,0,5000,1024,50,1],
["looking",1024,1000,5000,0,0,100,0],
["looking",1024,1000,5000,0,256,200,0],
["looking",1024,1000,5000,0,1024,50,0],
["looking",1024,1000,0,-5000,0,100,0],
["looking",1024,1000,0,-5000,256,200,0],
["looking",1024,1000,0,-5000,1024,50,0],
["looking",1024,1000,-5000,0,0,100,0],
["looking",1024,1000,-5000,0,256,200,0],
["looking",1024,1000,-5000,0,1024,50,0],
["looking",1024,1000,3000,4000,0,100,0],
["looking",1024,1000,3000,4000,256,200,1],
["looking",1024,1000,3000,4000,1024,50,0],
["looking",1024,1000,-3000,4000,0,100,0],
["looking",1024,1000,-3000,4000,256,200,0],
["looking",1024,1000,-3000,4000,1024,50,0],
["looking",1024,1000,4000,-3000,0,100,0],
["looking",1024,1000,4000,-3000,256,200,0],
["looking",1024,1000,4000,-3000,1024,50,0],
["looking",1024,1000,-2500,-2500,0,100,0],
["looking",1024,1000,-2500,-2500,256,200,0],
["looking",1024,1000,-2500,-2500,1024,50,0],
["facing",0,0,0,5000,0,100,1],
["facing",0,0,0,5000,256,200,0],
["facing",0,0,0,5000,1024,50,0],
["facing",0,0,5000,0,0,100,1],
["facing",0,0,5000,0,256,200,0],
["facing",0,0,5000,0,1024,50,0],
["facing",0,0,0,-5000,0,100,1],
["facing",0,0,0,-5000,256,200,0],
["facing",0,0,0,-5000,1024,50,0],
["facing",0,0,-5000,0,0,100,1],
["facing",0,0,-5000,0,256,200,0],
["facing",0,0,-5000,0,1024,50,0],
["facing",0,0,3000,4000,0,100,1],
["facing",0,0,3000,4000,256,200,0],
["facing",0,0,3000,4000,1024,50,0],
["facing",0,0,-3000,4000,0,100,1],
["facing",0,0,-3000,4000,256,200,0],
["facing",0,0,-3000,4000,1024,50,0],
["facing",0,0,4000,-3000,0,100,1],
["facing",0,0,4000,-3000,256,200,0],
["facing",0,0,4000,-3000,1024,50,0],
["facing",0,0,-2500,-2500,0,100,1],
["facing",0,0,-2500,-2500,256,200,0],
["facing",0,0,-2500,-2500,1024,50,0],
["facing",0,1000,0,5000,0,100,0],
["facing",0,1000,0,5000,256,200,0],
["facing",0,1000,0,5000,1024,50,1],
["facing",0,1000,5000,0,0,100,0],
["facing",0,1000,5000,0,256,200,0],
["facing",0,1000,5000,0,1024,50,1],
["facing",0,1000,0,-5000,0,100,0],
["facing",0,1000,0,-5000,256,200,0],
["facing",0,1000,0,-5000,1024,50,1],
["facing",0,1000,-5000,0,0,100,0],
["facing",0,1000,-5000,0,256,200,0],
["facing",0,1000,-5000,0,1024,50,1],
["facing",0,1000,3000,4000,0,100,0],
["facing",0,1000,3000,4000,256,200,0],
["facing",0,1000,3000,4000,1024,50,1],
["facing",0,1000,-3000,4000,0,100,0],
["facing",0,1000,-3000,4000,256,200,0],
["facing",0,1000,-3000,4000,1024,50,1],
["facing",0,1000,4000,-3000,0,100,0],
["facing",0,1000,4000,-3000,256,200,0],
["facing",0,1000,4000,-3000,1024,50,1],
["facing",0,1000,-2500,-2500,0,100,0],
["facing",0,1000,-2500,-2500,256,200,0],
["facing",0,1000,-2500,-2500,1024,50,1],
["facing",1024,0,0,5000,0,100,0],
["facing",1024,0,0,5000,256,200,0],
["facing",1024,0,0,5000,1024,50,1],
["facing",1024,0,5000,0,0,100,0],
["facing",1024,0,5000,0,256,200,0],
["facing",1024,0,5000,0,1024,50,1],
["facing",1024,0,0,-5000,0,100,0],
["facing",1024,0,0,-5000,256,200,0],
["facing",1024,0,0,-5000,1024,50,1],
["facing",1024,0,-5000,0,0,100,0],
["facing",1024,0,-5000,0,256,200,0],
["facing",1024,0,-5000,0,1024,50,1],
["facing",1024,0,3000,4000,0,100,0],
["facing",1024,0,3000,4000,256,200,0],
["facing",1024,0,3000,4000,1024,50,1],
["facing",1024,0,-3000,4000,0,100,0],
["facing",1024,0,-3000,4000,256,200,0],
["facing",1024,0,-3000,4000,1024,50,1],
["facing",1024,0,4000,-3000,0,100,0],
["facing",1024,0,4000,-3000,256,200,0],
["facing",1024,0,4000,-3000,1024,50,1],
["facing",1024,0,-2500,-2500,0,100,0],
["facing",1024,0,-2500,-2500,256,200,0],
["facing",1024,0,-2500,-2500,1024,50,1],
["facing",1024,1000,0,5000,0,100,1],
["facing",1024,1000,0,5000,256,200,0],
["facing",1024,1000,0,5000,1024,50,0],
["facing",1024,1000,5000,0,0,100,1],
["facing",1024,1000,5000,0,256,200,0],
["facing",1024,1000,5000,0,1024,50,0],
["facing",1024,1000,0,-5000,0,100,1],
["facing",1024,1000,0,-5000,256,200,0],
["facing",1024,1000,0,-5000,1024,50,0],
["facing",1024,1000,-5000,0,0,100,1],
["facing",1024,1000,-5000,0,256,200,0],
["facing",1024,1000,-5000,0,1024,50,0],
["facing",1024,1000,3000,4000,0,100,1],
["facing",1024,1000,3000,4000,256,200,0],
["facing",1024,1000,3000,4000,1024,50,0],
["facing",1024,1000,-3000,4000,0,100,1],
["facing",1024,1000,-3000,4000,256,200,0],
["facing",1024,1000,-3000,4000,1024,50,0],
["facing",1024,1000,4000,-3000,0,100,1],
["facing",1024,1000,4000,-3000,256,200,0],
["facing",1024,1000,4000,-3000,1024,50,0],
["facing",1024,1000,-2500,-2500,0,100,1],
["facing",1024,1000,-2500,-2500,256,200,0],
["facing",1024,1000,-2500,-2500,1024,50,0],
["arc",0,0,0,5000,0,100,0],
["arc",0,0,0,5000,256,200,0],
["arc",0,0,0,5000,1024,50,1],
["arc",0,0,5000,0,0,100,0],
["arc",0,0,5000,0,256,200,0],
["arc",0,0,5000,0,1024,50,0],
["arc",0,0,0,-5000,0,100,1],
["arc",0,0,0,-5000,256,200,0],
["arc",0,0,0,-5000,1024,50,0],
["arc",0,0,-5000,0,0,100,0],
["arc",0,0,-5000,0,256,200,0],
["arc",0,0,-5000,0,1024,50,0],
["arc",0,0,3000,4000,0,100,0],
["arc",0,0,3000,4000,256,200,0],
["arc",0,0,3000,4000,1024,50,0],
["arc",0,0,-3000,4000,0,100,0],
["arc",0,0,-3000,4000,256,200,0],
["arc",0,0,-3000,4000,1024,50,0],
["arc",0,0,4000,-3000,0,100,0],
["arc",0,0,4000,-3000,256,200,0],
["arc",0,0,4000,-3000,1024,50,0],
["arc",0,0,-2500,-2500,0,100,0],
["arc",0,0,-2500,-2500,256,200,1],
["arc",0,0,-2500,-2500,1024,50,0],
["arc",0,1000,0,5000,0,100,0],
["arc",0,1000,0,5000,256,200,0],
["arc",0,1000,0,5000,1024,50,1],
["arc",0,1000,5000,0,0,100,0],
["arc",0,1000,5000,0,256,200,0],
["arc",0,1000,5000,0,1024,50,0],
["arc",0,1000,0,-5000,0,100,1],
["arc",0,1000,0,-5000,256,200,0],
["arc",0,1000,0,-5000,1024,50,0],
["arc",0,1000,-5000,0,0,100,0],
["arc",0,1000,-5000,0,256,200,0],
["arc",0,1000,-5000,0,1024,50,0],
["arc",0,1000,3000,4000,0,100,0],
["arc",0,1000,3000,4000,256,200,0],
["arc",0,1000,3000,4000,1024,50,0],
["arc",0,1000,-3000,4000,0,100,0],
["arc",0,1000,-3000,4000,256,200,0],
["arc",0,1000,-3000,4000,1024,50,0],
["arc",0,1000,4000,-3000,0,100,0],
["arc",0,1000,4000,-3000,256,200,0],
["arc",0,1000,4000,-3000,1024,50,0],
["arc",0,1000,-2500,-2500,0,100,0],
["arc",0,1000,-2500,-2500,256,200,1],
["arc",0,1000,-2500,-2500,1024,50,0],
["arc",1024,0,0,5000,0,100,1],
["arc",1024,0,0,5000,256,200,0],
["arc",1024,0,0,5000,1024,50,0],
["arc",1024,0,5000,0,0,100,0],
["arc",1024,0,5000,0,256,200,0],
["arc",1024,0,5000,0,1024,50,0],
["arc",1024,0,0,-5000,0,100,0],
["arc",1024,0,0,-5000,256,200,0],
["arc",1024,0,0,-5000,1024,50,1],
["arc",1024,0,-5000,0,0,100,0],
["arc",1024,0,-5000,0,256,200,0],
["arc",1024,0,-5000,0,1024,50,0],
["arc",1024,0,3000,4000,0,100,0],
["arc",1024,0,3000,4000,256,200,1],
["arc",1024,0,3000,4000,1024,50,0],
["arc",1024,0,-3000,4000,0,100,0],
["arc",1024,0,-3000,4000,256,200,0],
["arc",1024,0,-3000,4000,1024,50,0],
["arc",1024,0,4000,-3000,0,100,0],
["arc",1024,0,4000,-3000,256,200,0],
["arc",1024,0,4000,-3000,1024,50,0],
["arc",1024,0,-2500,-2500,0,100,0],
["arc",1024,0,-2500,-2500,256,200,0],
["arc",1024,0,-2500,-2500,1024,50,0],
["arc",1024,1000,0,5000,0,100,1],
["arc",1024,1000,0,5000,256,200,0],
["arc",1024,1000,0,5000,1024,50,0],
["arc",1024,1000,5000,0,0,100,0],
["arc",1024,1000,5000,0,256,200,0],
["arc",1024,1000,5000,0,1024,50,0],
["arc",1024,1000,0,-5000,0,100,0],
["arc",1024,1000,0,-5000,256,200,0],
["arc",1024,1000,0,-5000,1024,50,1],
["arc",1024,1000,-5000,0,0,100,0],
["arc",1024,1000,-5000,0,256,200,0],
["arc",1024,1000,-5000,0,1024,50,0],
["arc",1024,1000,3000,4000,0,100,0],
["arc",1024,1000,3000,4000,256,200,1],
["arc",1024,1000,3000,4000,1024,50,0],
["arc",1024,1000,-3000,4000,0,100,0],
["arc",1024,1000,-3000,4000,256,200,0],
["arc",1024,1000,-3000,4000,1024,50,0],
["arc",1024,1000,4000,-3000,0,100,0],
["arc",1024,1000,4000,-3000,256,200,0],
["arc",1024,1000,4000,-3000,1024,50,0],
["arc",1024,1000,-2500,-2500,0,100,0],
["arc",1024,1000,-2500,-2500,256,200,0],
["arc",1024,1000,-2500,-2500,1024,50,0]
]"#,
    )
    .unwrap();
    let mut world = ScriptWorld::prepare(&cache(), 1).unwrap();
    world.prepare_spatial().unwrap();
    let mut actor = world.spatial(world.id(0).unwrap()).unwrap().clone();
    actor.local.position = [0; 3];
    for (name, receiver_heading, player_heading, x, z, first, second, expected) in rows {
        actor.local.angles[1] = i32::from(receiver_heading);
        let inputs = QueryInputs {
            player: Some(PlayerQuery {
                position: [x, 0, z],
                heading: player_heading,
            }),
            distance_override: None,
            fallback_position: None,
            linked_position: Some(None),
        };
        let (function, arguments) = match name.as_str() {
            "looking" => (Native::PlayerLooking, vec![Argument::Integer(second)]),
            "facing" => (
                Native::PlayerFacing,
                vec![Argument::Integer(first), Argument::Integer(second)],
            ),
            _ => (
                Native::PlayerArc,
                vec![Argument::Integer(first), Argument::Integer(second)],
            ),
        };
        assert_eq!(
            actor.query(function, &arguments, inputs).unwrap(),
            Some(expected),
            "{function} receiver {receiver_heading} player {player_heading} at ({x},{z}) args ({first},{second})"
        );
    }
}

#[test]
fn player_distance_natives_match_the_original_routines() {
    use cyrus_script::script::{PlayerQuery, QueryInputs};
    // Rows from the original routines in an emulator: function, receiver position, player offset
    // from it, result.
    type Row = (String, [i32; 3], [i32; 3], i32);
    let rows: Vec<Row> = serde_json::from_str(
        r#"[
["distance",[0,0,0],[0,0,0],0],
["distance",[50,-60,70],[0,0,0],0],
["distance",[0,0,0],[100,200,300],600],
["distance",[50,-60,70],[100,200,300],600],
["distance",[0,0,0],[-5000,70000,123],75123],
["distance",[50,-60,70],[-5000,70000,123],75123],
["distance",[0,0,0],[256,256,256],768],
["distance",[50,-60,70],[256,256,256],768],
["distance",[0,0,0],[1,1,1],3],
["distance",[50,-60,70],[1,1,1],3],
["distance",[0,0,0],[3000,-4000,0],7000],
["distance",[50,-60,70],[3000,-4000,0],7000],
["distance",[0,0,0],[65536,65536,65536],196608],
["distance",[50,-60,70],[65536,65536,65536],196608],
["distance",[0,0,0],[-7,11,-13],31],
["distance",[50,-60,70],[-7,11,-13],31],
["distance",[0,0,0],[100000,0,0],100000],
["distance",[50,-60,70],[100000,0,0],100000],
["distance",[0,0,0],[123456,-654321,99999],877776],
["distance",[50,-60,70],[123456,-654321,99999],877776],
["acurate",[0,0,0],[0,0,0],0],
["acurate",[50,-60,70],[0,0,0],0],
["acurate",[0,0,0],[100,200,300],374],
["acurate",[50,-60,70],[100,200,300],374],
["acurate",[0,0,0],[-5000,70000,123],70178],
["acurate",[50,-60,70],[-5000,70000,123],70178],
["acurate",[0,0,0],[256,256,256],443],
["acurate",[50,-60,70],[256,256,256],443],
["acurate",[0,0,0],[1,1,1],2],
["acurate",[50,-60,70],[1,1,1],2],
["acurate",[0,0,0],[3000,-4000,0],5000],
["acurate",[50,-60,70],[3000,-4000,0],5000],
["acurate",[0,0,0],[65536,65536,65536],113512],
["acurate",[50,-60,70],[65536,65536,65536],113512],
["acurate",[0,0,0],[-7,11,-13],18],
["acurate",[50,-60,70],[-7,11,-13],18],
["acurate",[0,0,0],[100000,0,0],100000],
["acurate",[50,-60,70],[100000,0,0],100000],
["acurate",[0,0,0],[123456,-654321,99999],673333],
["acurate",[50,-60,70],[123456,-654321,99999],673333]
]"#,
    )
    .unwrap();
    let mut world = ScriptWorld::prepare(&cache(), 1).unwrap();
    world.prepare_spatial().unwrap();
    let mut actor = world.spatial(world.id(0).unwrap()).unwrap().clone();
    for (name, base, offset, expected) in rows {
        actor.local.position = base;
        let inputs = QueryInputs {
            player: Some(PlayerQuery {
                position: std::array::from_fn(|axis| base[axis].wrapping_add(offset[axis])),
                heading: 0,
            }),
            distance_override: Some(false),
            fallback_position: None,
            linked_position: Some(None),
        };
        let function = if name == "distance" {
            Native::PlayerDistance
        } else {
            Native::AcuratePlayerDistance
        };
        assert_eq!(
            actor.query(function, &[], inputs).unwrap(),
            Some(expected),
            "{function} {base:?} {offset:?}"
        );
    }
}
