//! `PlayerHealth`, `PlayerArmed` and `PlayerOffset` take the selected player whatever their receiver,
//! and `GetMovementSpeed` maps the receiver's current group to a speed attribute.
use super::*;
use cyrus_script::script::{ScriptActorSource, ScriptHud, ScriptInputs, ScriptStage};

/// LOGIC (health 20) calls `function(arguments)` into local 0; the selected player has health 77.
fn run(
    function: Native,
    arguments: &[i32],
    holding: bool,
    with_hud: bool,
) -> (ScriptWorld, usize, anyhow::Result<()>) {
    let (mut scripts, _, data, _) = player_walking_fixture();
    scripts.actors.get_mut("CYRUS").unwrap().attributes[Attribute::Health] = 77;
    scripts.actors.get_mut("CYRUS").unwrap().attributes[Attribute::HoldingWeapon] =
        u8::from(holding);
    scripts.actors.get_mut("LOGIC").unwrap().attributes[Attribute::Health] = 20;
    let function_id = scripts.definition.functions.len() as u16;
    scripts.definition.functions.push(Function {
        name: function,
        param_count: arguments.len() as u8,
        kind: CallKind::Function,
    });
    scripts
        .actors
        .get_mut("LOGIC")
        .unwrap()
        .program
        .steps
        .insert(0, assign_call(1, function_id, function, arguments));
    scripts.content_id = scripts.content_identity().unwrap();
    let mut data = data;
    data.metadata["script_content_id"] = scripts.content_id.clone().into();
    let (mut world, _, _) = static_model_fixture_session(scripts, None, [10, 20, 30], true);
    let index = world
        .actors()
        .iter()
        .position(|actor| matches!(actor.source, ScriptActorSource::Player))
        .unwrap();
    let player = world.id(index).unwrap();
    world
        .initialize_player_animation(player, &data, 0, &mut |_| Ok(()))
        .unwrap();
    world
        .initialize_player_spatial(player, [12, 34, 56], [0; 3])
        .unwrap();
    let hud = ScriptHud::default();
    let result = world.advance(
        world.id(0).unwrap(),
        ScriptStage::Body(Thread::Main),
        ScriptInputs {
            hud: with_hud.then_some(&hud),
            nominal_ticks: Some(1),
            frame_scale: Some(256),
            ..Default::default()
        },
        None,
        None,
        100,
    );
    (world, index, result)
}

fn local(world: &ScriptWorld) -> i32 {
    world.actors()[0].runtime().unwrap().variables()[0]
        .integer()
        .unwrap()
}

#[test]
fn player_health_is_the_selected_players_health_not_the_receivers() {
    let (world, _, result) = run(Native::PlayerHealth, &[], false, false);
    result.unwrap();
    assert_eq!(local(&world), 77);
    assert_eq!(world.actors()[0].attributes()[Attribute::Health], 20);
}

#[test]
fn player_armed_is_the_selected_players_holding_predicate() {
    // A nonzero carrying byte on the player answers one before any ordinary gate.
    let (world, _, result) = run(Native::PlayerArmed, &[], true, false);
    result.unwrap();
    assert_eq!(local(&world), 1);
    // Without it the ordinary checks find a player with no drawn weapon.
    for with_hud in [false, true] {
        let (world, _, result) = run(Native::PlayerArmed, &[], false, with_hud);
        result.unwrap();
        assert_eq!(local(&world), 0);
    }
}

#[test]
fn player_offset_places_the_player_relative_to_the_receiver_once() {
    let (world, player, result) = run(Native::PlayerOffset, &[256, -512, 1024], false, false);
    result.unwrap();
    assert_eq!(local(&world), 0);
    let receiver = world.spatial(world.id(0).unwrap()).unwrap().local.position;
    let moved = world.spatial(world.id(player).unwrap()).unwrap();
    // The receiver's orientation is the identity, so the offset is added as given.
    assert_eq!(
        moved.local.position,
        [receiver[0] + 256, receiver[1] - 512, receiver[2] + 1024]
    );
    // The player's own orientation is left alone.
    assert_eq!(moved.local.angles, [0; 3]);
}

#[test]
fn get_movement_speed_maps_the_current_group_to_a_speed_attribute() {
    // Each speed attribute holds a distinct byte, so the mapping shows through the result.
    let attributes = [
        (Attribute::ForwardSpeed, 11),
        (Attribute::BackwardSpeed, 12),
        (Attribute::SideSpeed, 13),
        (Attribute::ForwardWalk, 14),
        (Attribute::BackwardWalk, 15),
        (Attribute::FightForwardSpeed, 16),
        (Attribute::FightForwardWalk, 17),
    ];
    for (group, expected) in [
        (18, 11),
        (30, 11),
        (142, 11),
        (19, 12),
        (143, 12),
        (20, 14),
        (21, 14),
        (22, 15),
        (23, 15),
        (24, 13),
        (27, 13),
        (31, 17),
        (92, 16),
        (93, 16),
        (5, 0),
        (28, 0),
    ] {
        let mut scripts = wait_cache(vec![
            Operation::Assign {
                destination: Place::Local { index: 0 },
                formula: Formula {
                    first: Value::Call {
                        call: Box::new(Call {
                            function_id: 1,
                            function: Native::GetMovementSpeed,
                            kind: CallKind::Function,
                            target: Object::Me,
                            arguments: vec![],
                        }),
                    },
                    rest: vec![],
                },
            },
            Operation::End { target: 0 },
        ]);
        scripts.definition.functions[1] = Function {
            name: Native::GetMovementSpeed,
            param_count: 0,
            kind: CallKind::Function,
        };
        let logic = scripts.actors.get_mut("LOGIC").unwrap();
        for (attribute, value) in attributes {
            logic.attributes[attribute] = value;
        }
        scripts.content_id = scripts.content_identity().unwrap();
        let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
        let id = world.id(0).unwrap();
        let data = cyrus_assets::animation::Animations {
            version: 2,
            world: scripts.world,
            metadata: serde_json::json!({"script_content_id": scripts.content_id, "actors": [{
                "script_name": world.actors()[0].name(),
                "animation_groups": [{"group_index": group, "anim_id": 1, "anim_type": 0, "commands": [
                    {"opcode": 0, "frame_reference": 1}, {"opcode": 2, "value": 0}]}]}]}),
            sections: Default::default(),
            models: Default::default(),
            unsupported: Default::default(),
        };
        world
            .bind_animation(id, &data, AnimationGroup::from_index(group))
            .unwrap();
        let inputs = ScriptInputs {
            nominal_ticks: Some(1),
            frame_scale: Some(256),
            ..Default::default()
        };
        world
            .advance(id, ScriptStage::Animation, inputs, None, None, 0)
            .unwrap();
        world
            .advance(id, ScriptStage::Body(Thread::Main), inputs, None, None, 100)
            .unwrap();
        assert_eq!(local(&world), expected, "group {group}");
    }
}

#[test]
fn selected_player_queries_return_zero_without_optional_resources() {
    let mut results = Vec::new();
    for native in [
        Native::PlayerTurned,
        Native::PlayerTurnedDirection,
        Native::PlayerMovementSpeed,
    ] {
        let mut scripts = random_scripts(native, vec![], Object::Me);
        scripts.definition.functions[1].param_count = 0;
        add_standalone_receivers(&mut scripts);
        let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
        let result = world.step(world.id(0).unwrap(), Thread::Main, 20);
        results.push((native, result.map(|_| local(&world))));
    }
    assert!(
        results.iter().all(|(_, result)| matches!(result, Ok(0))),
        "{results:?}"
    );
}

#[test]
fn player_queries_follow_selected_state_and_model_presence() {
    for (native, hidden) in [Native::PlayerTurned, Native::PlayerTurnedDirection]
        .into_iter()
        .flat_map(|native| [false, true].map(|hidden| (native, hidden)))
    {
        let (_, mut world, _, _) = player_walking_fixture_with(|scripts| {
            let id = scripts.definition.functions.len() as u16;
            scripts.definition.functions.push(Function {
                name: native,
                param_count: 0,
                kind: CallKind::Function,
            });
            scripts
                .actors
                .get_mut("LOGIC")
                .unwrap()
                .program
                .steps
                .insert(0, assign_call(1, id, native, &[]));
            let hide_id = scripts.definition.functions.len() as u16;
            scripts.definition.functions.push(Function {
                name: Native::HideMe,
                param_count: 0,
                kind: CallKind::Function,
            });
            scripts
                .actors
                .get_mut("CYRUS")
                .unwrap()
                .program
                .steps
                .get_mut(&0)
                .unwrap()
                .operation = wait(hide_id, Native::HideMe, CallKind::Function, vec![]);
        });
        let player = world.selected_player().unwrap();
        assert!(world.model_presentation(player).unwrap().is_some());
        world.snapshot_spatial(player).unwrap();
        world.set_local_angles_for_test(player, [0, 12, 0]);
        if hidden {
            world
                .advance(
                    player,
                    ScriptStage::Body(Thread::Main),
                    ScriptInputs {
                        nominal_ticks: Some(1),
                        frame_scale: Some(256),
                        ..Default::default()
                    },
                    None,
                    None,
                    20,
                )
                .unwrap();
        }
        assert_eq!(world.model_presentation(player).unwrap().is_none(), hidden);
        let position = world.spatial(player).unwrap().local;
        world
            .advance(
                world.id(0).unwrap(),
                ScriptStage::Body(Thread::Main),
                ScriptInputs {
                    nominal_ticks: Some(1),
                    frame_scale: Some(256),
                    ..Default::default()
                },
                None,
                None,
                20,
            )
            .unwrap();
        let expected = if hidden {
            0
        } else if native == Native::PlayerTurned {
            1
        } else {
            -1
        };
        assert_eq!(local(&world), expected, "{native}, hidden {hidden}");
        assert_eq!(
            world.spatial(player).unwrap().local.position,
            position.position
        );
        assert_eq!(world.spatial(player).unwrap().local.angles, position.angles);
    }
}

#[test]
fn player_movement_speed_reads_animation_without_a_model_or_spatial_state() {
    let mut scripts = random_scripts(Native::PlayerMovementSpeed, vec![], Object::Me);
    scripts.definition.functions[1].param_count = 0;
    add_standalone_receivers(&mut scripts);
    scripts.actors.get_mut("CYRUS").unwrap().attributes[Attribute::ForwardWalk] = 37;
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
    let player = world.selected_player().unwrap();
    let data = cyrus_assets::animation::Animations {
        version: 2,
        world: scripts.world,
        metadata: serde_json::json!({"script_content_id":scripts.content_id,"actors":[{
            "script_name":"CYRUS","animation_groups":[{"group_index":AnimationGroup::WALK.index(),"anim_id":1,"anim_type":0,"commands":[{"opcode":0,"frame_reference":1},{"opcode":2,"value":0}]}]
        }]}),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    };
    world
        .bind_animation(player, &data, AnimationGroup::WALK)
        .unwrap();
    world
        .advance(
            player,
            ScriptStage::Animation,
            ScriptInputs {
                nominal_ticks: Some(1),
                frame_scale: Some(256),
                ..Default::default()
            },
            None,
            None,
            20,
        )
        .unwrap();
    assert!(world.model_presentation(player).unwrap().is_none());
    assert!(world.spatial(player).is_err());
    world
        .advance(
            world.id(0).unwrap(),
            ScriptStage::Body(Thread::Main),
            ScriptInputs {
                nominal_ticks: Some(1),
                frame_scale: Some(256),
                ..Default::default()
            },
            None,
            None,
            20,
        )
        .unwrap();
    assert_eq!(local(&world), 37);
}
