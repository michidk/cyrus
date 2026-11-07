//! `ZapObject`, `ZapObjectNoKill`, `ZapPlayer` and `ZapPlayerNoKill`: health adjustments whose
//! selected-player branches are the services `AdjustHealth` uses.
use super::*;
use cyrus_script::script::{ScriptActorSource, ScriptHud, ScriptInputs, ScriptStage};

/// Actor 0 runs `function(amount)` as a statement, with `edit` applied to its attributes.
fn object_world(
    function: Native,
    amount: i32,
    edit: &[(Attribute, u8)],
) -> (ScriptWorld, anyhow::Result<()>) {
    let mut scripts = wait_cache(vec![
        wait(
            1,
            function,
            CallKind::Function,
            vec![Value::Integer { value: amount }],
        ),
        Operation::End { target: 0 },
    ]);
    scripts.definition.functions[1] = Function {
        name: function,
        param_count: 1,
        kind: CallKind::Function,
    };
    let logic = scripts.actors.get_mut("LOGIC").unwrap();
    logic.attributes[Attribute::Health] = 100;
    for (attribute, value) in edit {
        logic.attributes[*attribute] = *value;
    }
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let id = world.id(0).unwrap();
    let result = world.advance(
        id,
        ScriptStage::Body(Thread::Main),
        ScriptInputs {
            nominal_ticks: Some(0),
            frame_scale: Some(256),
            ..Default::default()
        },
        None,
        None,
        100,
    );
    (world, result)
}

fn health(world: &ScriptWorld) -> u8 {
    world.actors()[0].attributes()[Attribute::Health]
}

#[test]
fn a_receivers_health_byte_takes_the_wrapped_sum_clamped_to_a_byte() {
    for function in [Native::ZapObjectNoKill, Native::ZapObject] {
        for (amount, expected) in [(30, 130), (0, 100), (200, 255), (i32::MAX, 0), (-60, 40)] {
            // i32::MAX wraps the sum negative: the byte clears.
            let (world, result) = object_world(function, amount, &[]);
            if function == Native::ZapObject && expected == 0 {
                // The kill first asks whether the receiver holds a weapon, which needs the inventory.
                let error = format!("{:#}", result.unwrap_err());
                assert!(error.contains("inventory owner unavailable"), "{error}");
            } else {
                result.unwrap();
            }
            assert_eq!(health(&world), expected, "{function} {amount}");
        }
    }
    // Without the kill variant a nonpositive sum only clears the byte.
    let (world, result) = object_world(Native::ZapObjectNoKill, -500, &[]);
    result.unwrap();
    assert_eq!(health(&world), 0);
}

#[test]
fn invulnerable_receivers_are_left_alone_and_npcs_need_the_weapon_owner() {
    for function in [Native::ZapObjectNoKill, Native::ZapObject] {
        let (world, result) = object_world(function, -30, &[(Attribute::Invulnerable, 1)]);
        result.unwrap();
        assert_eq!(health(&world), 100);
        // An NPC needs a held weapon or the stance flag; deciding that needs the inventory
        // owner, which this world lacks, so the native stops with that explicit error.
        let (world, result) = object_world(function, -30, &[(Attribute::Npc, 1)]);
        let error = format!("{:#}", result.unwrap_err());
        assert!(error.contains("inventory owner unavailable"), "{error}");
        assert_eq!(health(&world), 100);
    }
}

#[test]
fn the_results_are_pointer_residuals_so_consuming_one_is_unsupported() {
    let mut scripts = random_scripts(
        Native::ZapObjectNoKill,
        vec![Value::Integer { value: 5 }],
        Object::Me,
    );
    scripts.definition.functions[1].param_count = 1;
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let id = world.id(0).unwrap();
    let error = world
        .advance(
            id,
            ScriptStage::Body(Thread::Main),
            ScriptInputs {
                nominal_ticks: Some(0),
                ..Default::default()
            },
            None,
            None,
            100,
        )
        .unwrap_err();
    assert!(
        format!("{error:#}").contains("has no supported numeric result"),
        "{error:#}"
    );
}

/// The selected player takes `function(amount)` from LOGIC (receiver) or, with `at_player`, as an
/// object call on the player. Returns the player's health, the death-override group and the error.
fn player_world(
    function: Native,
    amount: i32,
    at_player: bool,
) -> (u8, Option<i32>, Option<String>) {
    let (mut scripts, _, data, _) = player_walking_fixture();
    scripts.actors.get_mut("CYRUS").unwrap().attributes[Attribute::Health] = 40;
    scripts.actors.get_mut("LOGIC").unwrap().attributes[Attribute::ZapAnim] = 77;
    let function_id = scripts.definition.functions.len() as u16;
    scripts.definition.functions.push(Function {
        name: function,
        param_count: 1,
        kind: CallKind::Function,
    });
    let call = Call {
        function_id,
        function,
        kind: CallKind::Function,
        target: if at_player {
            Object::Player
        } else {
            Object::Me
        },
        arguments: vec![Value::Integer { value: amount }],
    };
    scripts
        .actors
        .get_mut("LOGIC")
        .unwrap()
        .program
        .steps
        .get_mut(&0)
        .unwrap()
        .operation = if at_player {
        Operation::ObjectCall { call }
    } else {
        Operation::Call { call }
    };
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
    world
        .advance(
            world.id(0).unwrap(),
            ScriptStage::Entry,
            ScriptInputs {
                nominal_ticks: Some(0),
                frame_scale: Some(256),
                ..Default::default()
            },
            None,
            None,
            100,
        )
        .unwrap();
    world.set_actor_surface(player, Surface::UNKNOWN).unwrap();
    let hud = ScriptHud::default();
    let mut random = cyrus_game::random::EngineRandom::from_state(42);
    let result = world.advance(
        world.id(0).unwrap(),
        ScriptStage::Body(Thread::Main),
        ScriptInputs {
            hud: Some(&hud),
            nominal_ticks: Some(0),
            frame_scale: Some(256),
            death_suppressed: Some(false),
            ..Default::default()
        },
        None,
        Some(&mut random),
        100,
    );
    (
        world.actors()[index].attributes()[Attribute::Health],
        world.globals().player_death_override_group(),
        result.err().map(|error| format!("{error:#}")),
    )
}

#[test]
fn the_player_natives_heal_through_the_additive_and_death_owning_services() {
    // Healing is the same in both services: the health rises by the amount.
    for function in [Native::ZapPlayerNoKill, Native::ZapPlayer] {
        for at_player in [false, true] {
            let (health, _, error) = player_world(function, 7, at_player);
            assert_eq!((health, error), (47, None), "{function}");
        }
    }
    // A selected receiver of the object natives takes the same services.
    for function in [Native::ZapObjectNoKill, Native::ZapObject] {
        let (health, _, error) = player_world(function, 7, true);
        assert_eq!((health, error), (47, None), "{function}");
    }
}

#[test]
fn a_fatal_player_zap_names_the_receiver_as_the_damage_source_only_for_zap_player() {
    // LOGIC's ZapAnim selects the death animation when it is the source. ZapPlayer called by LOGIC
    // names it; the same native routed to the player, and ZapObject's selected branch, name none.
    let (health, group, error) = player_world(Native::ZapPlayer, -40, false);
    assert_eq!((health, group, error), (0, Some(77), None));
    let (health, group, error) = player_world(Native::ZapPlayer, -40, true);
    assert_eq!((health, group, error), (0, Some(0), None));
    let (health, group, error) = player_world(Native::ZapObject, -40, true);
    assert_eq!((health, group, error), (0, Some(0), None));
}

/// Actor 0 is an animated nonplayer with 5 health; `ZapObject(-50)` kills it. `current` starts
/// that group first, `holding` sets the weapon-holding attribute and `combat` gives it a record.
fn killed_world(
    holding: bool,
    combat: bool,
    current: AnimationGroup,
) -> (ScriptWorld, anyhow::Result<()>) {
    killed_world_with(Native::ZapObject, &[-50], holding, combat, current)
}

/// As [`killed_world`] with any native that ends the receiver through the same service.
fn killed_world_with(
    function: Native,
    arguments: &[i32],
    holding: bool,
    combat: bool,
    current: AnimationGroup,
) -> (ScriptWorld, anyhow::Result<()>) {
    let mut scripts = wait_cache(vec![
        wait(
            1,
            function,
            CallKind::Function,
            arguments
                .iter()
                .map(|&value| Value::Integer { value })
                .collect(),
        ),
        Operation::End { target: 0 },
    ]);
    scripts.definition.functions[1] = Function {
        name: function,
        param_count: arguments.len() as u8,
        kind: CallKind::Function,
    };
    let logic = scripts.actors.get_mut("LOGIC").unwrap();
    logic.attributes[Attribute::Health] = 5;
    logic.attributes[Attribute::HoldingWeapon] = u8::from(holding);
    logic.attributes[Attribute::Combat] = u8::from(combat);
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
    let id = world.id(0).unwrap();
    let groups = [114, 121, 122, 74, 134].map(|group| {
        serde_json::json!({"group_index": group, "anim_id": 1, "anim_type": 0, "commands": [
            {"opcode": 0, "frame_reference": group}, {"opcode": 2, "value": 0}]})
    });
    let mut data = cyrus_assets::animation::Animations {
        version: 2,
        world: scripts.world,
        metadata: serde_json::json!({"script_content_id": scripts.content_id, "actors": [{
            "script_name": world.actors()[0].name(), "animation_groups": groups}]}),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    };
    data.metadata["actors"][0]["animation_groups"] = groups.to_vec().into();
    world.bind_animation(id, &data, current).unwrap();
    world
        .advance(
            id,
            ScriptStage::Animation,
            ScriptInputs {
                nominal_ticks: Some(1),
                ..Default::default()
            },
            None,
            None,
            0,
        )
        .unwrap();
    let hud = ScriptHud::default();
    let result = world.advance(
        id,
        ScriptStage::Body(Thread::Main),
        ScriptInputs {
            hud: Some(&hud),
            nominal_ticks: Some(1),
            frame_scale: Some(256),
            ..Default::default()
        },
        None,
        None,
        100,
    );
    (world, result)
}

fn current_group(world: &ScriptWorld) -> i64 {
    world.inspect(0, 1)["actors"][0]["animation"]["current_group"]
        .as_i64()
        .unwrap()
}

#[test]
fn a_zapped_nonplayer_dies_into_the_group_its_state_selects() {
    use cyrus_script::script::AiType;
    // Not holding a weapon: the explore death, or the land death from a fight-landing group.
    let (world, result) = killed_world(false, false, AnimationGroup::IDLE_LEDGE);
    result.unwrap();
    assert_eq!(health(&world), 0);
    assert_eq!(current_group(&world), 134);
    assert_eq!(
        world.actors()[0].control_state().unwrap().ai_type,
        AiType::DEAD
    );
    let (world, result) = killed_world(false, false, AnimationGroup::FIGHT_LAND);
    result.unwrap();
    assert_eq!(current_group(&world), 74);
    assert_eq!(
        world.actors()[0].control_state().unwrap().ai_type,
        AiType::DEAD
    );
}

#[test]
fn a_zapped_armed_nonplayer_without_a_combat_record_faults() {
    let (_, result) = killed_world(true, false, AnimationGroup::IDLE_LEDGE);
    let error = format!("{:#}", result.unwrap_err());
    assert!(error.contains("combat record unavailable"), "{error}");
}

#[test]
fn set_and_adjust_health_end_a_nonplayer_through_the_same_death_dispatch() {
    use cyrus_script::script::AiType;
    for (function, arguments) in [
        (Native::SetHealth, &[0][..]),
        (Native::AdjustHealth, &[-50, 1][..]),
    ] {
        let (world, result) = killed_world_with(
            function,
            arguments,
            false,
            false,
            AnimationGroup::IDLE_LEDGE,
        );
        result.unwrap();
        assert_eq!(health(&world), 0, "{function}");
        assert_eq!(current_group(&world), 134, "{function}");
        assert_eq!(
            world.actors()[0].control_state().unwrap().ai_type,
            AiType::DEAD
        );
    }
    // Any other mode is the additive service: the byte clears and nothing dies.
    let (world, result) = killed_world_with(
        Native::AdjustHealth,
        &[-50, 0],
        false,
        false,
        AnimationGroup::IDLE_LEDGE,
    );
    result.unwrap();
    assert_eq!(health(&world), 0);
    assert_ne!(
        world.inspect(0, 1)["actors"][0]["animation"]["current_group"].as_i64(),
        Some(134)
    );
    assert_ne!(
        world.actors()[0].control_state().unwrap().ai_type,
        AiType::DEAD
    );
}
