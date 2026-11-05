//! GotoLocation composes public location lookup, arrival and AI refresh without an ordinary timer hook.
use cyrus_assets::script::*;
use cyrus_game::script::{ScriptInputs, ScriptStage, ScriptWorld};
use cyrus_script::program::*;
use cyrus_script::{Attribute, Thread};
use std::collections::BTreeMap;

fn cache() -> Scripts {
    let program = Program {
        version: 4,
        entry: 0,
        variables: vec![4],
        strings: vec!["logic".into()],
        steps: BTreeMap::new(),
    };
    let mut scripts = Scripts {
        version: 30,
        weapons: Default::default(),
        camera: Default::default(),
        combat_attacks: vec![],
        combat_defenses: vec![],
        in_combat_threshold: None,
        lineup_distance: None,
        texture_frames: Default::default(),
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
            rope: None,
            ropes_load: Some(true),
            fall_death_ticks: 144,
            fall_landing: None,
            dummy_model_manager: false,
            task_system: true,
            animation_system: true,
            disable_slaves: false,
            disable_master_slaves: false,
            final_version: false,
        },
        clock: cyrus_assets::script::ClockSettings {
            normal_frame_rate: 12,
            min_frame_rate: 6,
            max_frame_rate: 300,
            use_smooth_fps: true,
            use_smooth_divisor: true,
        },
        actor_pool_capacity: 32,
        content_id: String::new(),
        shells: Vec::new(),
        world: 7,
        definition: Definition {
            functions: vec![Function {
                name: cyrus_script::Native::NullFunction,
                param_count: 0,
                kind: CallKind::Function,
            }],
            flags: vec![Flag {
                name: "Counter".into(),
                flag_type: "NUMBER".into(),
                initial_value: 12,
            }],
            references: vec![],
            attributes: vec![],
        },
        actors: BTreeMap::from([(
            "LOGIC".into(),
            Actor {
                header_index: 0,
                code_origin: 0,
                instances: 2,
                namespace: Some("logic".into()),
                instance_variables: vec![vec![4], vec![9]],
                location_offsets: vec![],
                program,
                attributes: Default::default(),
                combat_taunts: vec![],
                native_hook: None,
            },
        )]),
        instances: (0..2)
            .map(|id| Instance {
                object_id: id,
                actor: "LOGIC".into(),
                object_type: 0,
                active: 0,
                model_name: String::new(),
                model_mode: 0,
                light_intensity: 0,
                light_radius: 0,
                light_rgb: [0; 3],
                texture: 0,
                map_position: [0; 3],
                angles: [0; 3],
            })
            .collect(),
        unsupported: BTreeMap::new(),
    };
    scripts.content_id = scripts.content_identity().unwrap();
    scripts
}

fn fixture(location: i32, delay: u8, speed: u8, shape: bool) -> ScriptWorld {
    fixture_for(location, delay, speed, shape, false)
}

fn fixture_for(location: i32, delay: u8, speed: u8, shape: bool, named: bool) -> ScriptWorld {
    use cyrus_script::{AnimationGroup, Native};
    let mut scripts = cache();
    scripts.definition.references = vec![
        cyrus_script::Property::AiDelay,
        cyrus_script::Property::AiType,
        cyrus_script::Property::AiMode,
    ];
    scripts.definition.functions.push(Function {
        name: Native::GotoLocation,
        param_count: 1,
        kind: CallKind::Task,
    });
    for instance in &mut scripts.instances {
        instance.object_type = 1;
    }
    scripts.instances[1].map_position = [50, 0, 0];
    let actor = scripts.actors.get_mut("LOGIC").unwrap();
    actor.location_offsets = vec![[3 * 256, 0, 0]];
    actor.attributes[Attribute::Collide] = 129;
    actor.attributes[Attribute::ReservedCollide] = 207;
    actor.attributes[Attribute::ForwardSpeed] = speed;
    let assign = |destination, value| Operation::Assign {
        destination,
        formula: Formula {
            first: Value::Integer { value },
            rest: vec![],
        },
    };
    let object = if named {
        Object::Named { string: 0 }
    } else {
        Object::Me
    };
    let mut operations = vec![];
    for (reference, value) in [(0, i32::from(delay)), (1, 6), (2, 11)] {
        operations.push(assign(
            Place::Property {
                object: object.clone(),
                reference,
            },
            value,
        ));
    }
    let call = Call {
        function_id: 1,
        function: Native::GotoLocation,
        target: object,
        kind: CallKind::Task,
        arguments: vec![Value::Integer { value: location }],
    };
    operations.push(if named {
        Operation::ObjectCall { call }
    } else {
        Operation::Call { call }
    });
    operations.push(assign(Place::Flag { index: 0 }, 73));
    operations.push(Operation::End { target: 5 });
    actor.program.steps = operations
        .into_iter()
        .enumerate()
        .map(|(i, operation)| {
            (
                i as u32,
                Step {
                    next: i as u32 + 1,
                    operation,
                },
            )
        })
        .collect();
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
    world.prepare_spatial().unwrap();
    let data = cyrus_assets::animation::Animations {
        version: 2,
        world: 7,
        metadata: serde_json::json!({"script_content_id":scripts.content_id,
          "actors":[{"script_name":"LOGIC", "model_load_path":"animation_table", "mpsz_bounds_primary":0,
          "raex":{"unknown_12":256}, "animation_groups":[{"group_index":18, "anim_id":1, "anim_type":0,
          "commands":[{"opcode":0,"frame_reference":18},{"opcode":2,"value":0}]}]}],
          "mpsz_entries":[{"total_extent":[24,12,24],"pos_extent":[12,0,12],"neg_extent":[12,12,12],"center_offset":[0,0,0],"flags":0}]}),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    };
    let id = world.id(0).unwrap();
    if speed != 0 {
        world
            .bind_animation(id, &data, AnimationGroup::RUN)
            .unwrap();
        world
            .advance(
                id,
                ScriptStage::Animation,
                ScriptInputs {
                    nominal_ticks: Some(1),
                    frame_scale: Some(256),
                    ..Default::default()
                },
                Some(&mut |_, _| Ok(())),
                None,
                20,
            )
            .unwrap();
    }
    if shape {
        let scene = scene();
        world
            .prepare_ordinary_collision_shape(id, &data, &scene)
            .unwrap();
    }
    world
}

fn advance(world: &mut ScriptWorld, stage: ScriptStage, ticks: Option<u32>) -> anyhow::Result<()> {
    world.advance(
        world.id(0)?,
        stage,
        ScriptInputs {
            nominal_ticks: ticks,
            frame_scale: Some(256),
            ..Default::default()
        },
        None,
        None,
        20,
    )?;
    Ok(())
}

fn scene() -> cyrus_game::player::data::PlayerScene {
    use cyrus_collision::collision_world::StaticCollision;
    use cyrus_game::player::data::{MovementSettings, PlayerScene};
    PlayerScene {
        source_identity: None,
        world: 7,
        model_radii: Default::default(),
        control_codes: None,
        settings: MovementSettings {
            standing_height: -2,
            post_collide_height: 1,
            slide_range: 1,
            slide_speed: None,
            slide_hurt: None,
            tap_time: None,
            sheath_sword_delay: None,
            defend_interval: None,
            swim_depth: None,
            turn_speed: 1,
            turn_max_speed: 1,
            poly_push_units: 1,
            joystick: None,
        },
        definition: None,
        terrain: None,
        collision: cyrus_assets::collision::CollisionScene {
            sphere_object_scale: Some(256),
            ..Default::default()
        },
        static_collision: StaticCollision::new(&Default::default()).unwrap(),
        actor_collision: None,
        unavailable: None,
    }
}

#[test]
fn self_nonarrival_refreshes_on_initial_call_and_poll_without_running_continuation() {
    for (delay, ticks, after_call, after_poll) in [
        (0, 3, 0, 0),
        (255, 3, 255, 255),
        (2, 3, 0, 0),
        (3, 3, 0, 0),
        (8, 3, 5, 2),
        (254, 300, 0, 0),
        (8, 0, 8, 8),
    ] {
        let mut world = fixture(1, delay, 0, true);
        advance(&mut world, ScriptStage::Body(Thread::Main), Some(ticks)).unwrap();
        let control = world.actors()[0].control_state().unwrap();
        assert_eq!(
            (
                control.ai_type.byte(),
                control.ai_location,
                control.ai_delay
            ),
            (4, 1, after_call)
        );
        assert_eq!(control.ai_mode, 11);
        assert_eq!(world.actors()[0].attributes()[Attribute::Collide], 4);
        assert_eq!(
            world.spatial(world.id(0).unwrap()).unwrap().local.position,
            [0; 3]
        );
        assert_eq!(world.flags(), [12]);
        assert_eq!(
            world.actors()[0].attributes()[Attribute::ReservedCollide],
            207
        );
        assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 1);
        advance(&mut world, ScriptStage::Tasks, Some(ticks)).unwrap();
        assert_eq!(
            world.actors()[0].control_state().unwrap().ai_delay,
            after_poll
        );
        assert_eq!(world.flags(), [12]);
        assert_eq!(world.actors()[1].control_state().unwrap().ai_delay, 0);
        assert_eq!(world.actors()[1].control_state().unwrap().ai_type.byte(), 0);
    }
}

#[test]
fn arrival_completes_before_refresh_while_equal_speed_threshold_stays_pending() {
    for (speed, arrives) in [(2, false), (3, true)] {
        let mut world = fixture(1, 8, speed, true);
        advance(&mut world, ScriptStage::Body(Thread::Main), Some(3)).unwrap();
        let control = world.actors()[0].control_state().unwrap();
        assert_eq!(world.flags(), [12]);
        assert_eq!(control.ai_type.byte(), if arrives { 0 } else { 4 });
        assert_eq!(control.ai_delay, if arrives { 8 } else { 5 });
        assert_eq!(control.ai_location, if arrives { 0 } else { 1 });
        assert_eq!(control.ai_mode, 11);
        assert_eq!(
            world.actors()[0].attributes()[Attribute::Collide],
            if arrives { 129 } else { 4 }
        );
        assert_eq!(
            world.spatial(world.id(0).unwrap()).unwrap().local.position,
            if arrives { [3 * 256, 0, 0] } else { [0; 3] }
        );
        assert_eq!(
            world
                .spatial(world.id(0).unwrap())
                .unwrap()
                .previous_local_position,
            [0; 3]
        );
        if arrives {
            advance(&mut world, ScriptStage::Tasks, Some(3)).unwrap();
            advance(&mut world, ScriptStage::Body(Thread::Main), Some(3)).unwrap();
            assert_eq!(world.flags(), [73]);
        }
    }
    // Arrival retains the sentinel through the public stage's admitted clock.
    let mut world = fixture(1, 255, 3, true);
    advance(&mut world, ScriptStage::Body(Thread::Main), Some(3)).unwrap();
    assert_eq!(world.flags(), [12]);
    advance(&mut world, ScriptStage::Tasks, Some(3)).unwrap();
    advance(&mut world, ScriptStage::Body(Thread::Main), Some(3)).unwrap();
    assert_eq!(world.flags(), [73]);
    assert_eq!(world.actors()[0].control_state().unwrap().ai_delay, 255);
}

#[test]
fn low_word_minus_one_completes_only_after_selector_delay_and_collide_writes() {
    for location in [-1, 65535] {
        let mut world = fixture(location, 8, 0, true);
        advance(&mut world, ScriptStage::Body(Thread::Main), Some(3)).unwrap();
        let control = world.actors()[0].control_state().unwrap();
        assert_eq!(
            (
                control.ai_type.byte(),
                control.ai_location,
                control.ai_delay
            ),
            (4, -1, 5)
        );
        assert_eq!(world.actors()[0].attributes()[Attribute::Collide], 4);
        assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 1);
        assert_eq!(world.flags(), [12]);
        advance(&mut world, ScriptStage::Tasks, Some(3)).unwrap();
        assert_eq!(world.actors()[0].control_state().unwrap().ai_delay, 2);
        assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
        advance(&mut world, ScriptStage::Body(Thread::Main), Some(3)).unwrap();
        assert_eq!(world.flags(), [73]);
    }
}

#[test]
fn unsupported_lookup_and_shape_failures_do_not_fabricate_native_success() {
    for (location, shape, ticks, error_text, selector, index) in [
        (2, true, Some(3), "selector equals location count", 6, 0),
        (1, false, Some(3), "collision", 6, 0),
    ] {
        let mut world = fixture(location, 8, 0, shape);
        let error = advance(&mut world, ScriptStage::Body(Thread::Main), ticks).unwrap_err();
        assert!(format!("{error:#}").contains(error_text), "{error:#}");
        let control = world.actors()[0].control_state().unwrap();
        assert_eq!(
            (
                control.ai_type.byte(),
                control.ai_location,
                control.ai_delay
            ),
            (selector, index, 8)
        );
        assert_eq!(world.actors()[0].attributes()[Attribute::Collide], 129);
        assert_eq!(world.flags(), [12]);
        let failure = world.actors()[0].failure().unwrap().to_owned();
        assert!(advance(&mut world, ScriptStage::Body(Thread::Main), Some(3)).is_err());
        assert_eq!(world.actors()[0].failure(), Some(failure.as_str()));
    }
}

#[test]
fn retained_timer_sentinels_survive_admitted_ticks_and_keep_collide_refresh() {
    for delay in [0, 255] {
        let mut world = fixture(1, delay, 0, true);
        advance(&mut world, ScriptStage::Body(Thread::Main), Some(3)).unwrap();
        assert_eq!(world.actors()[0].control_state().unwrap().ai_delay, delay);
        assert_eq!(world.actors()[0].attributes()[Attribute::Collide], 4);
        assert_eq!(world.flags(), [12]);
        advance(&mut world, ScriptStage::Tasks, Some(3)).unwrap();
        assert_eq!(world.actors()[0].control_state().unwrap().ai_delay, delay);
    }
}

#[test]
fn object_targeted_task_continues_caller_and_keeps_its_token_on_the_receiver() {
    let mut world = fixture_for(1, 8, 0, true, true);
    world
        .advance(
            world.id(1).unwrap(),
            ScriptStage::Body(Thread::Main),
            ScriptInputs {
                nominal_ticks: Some(3),
                frame_scale: Some(256),
                ..Default::default()
            },
            None,
            None,
            20,
        )
        .unwrap();
    assert_eq!(world.flags(), [73]);
    assert_eq!(world.actors()[0].control_state().unwrap().ai_delay, 5);
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 1);
    assert_eq!(world.actors()[1].runtime().unwrap().active_tasks(), 0);
    assert_eq!(world.actors()[1].control_state().unwrap().ai_type.byte(), 0);
    advance(&mut world, ScriptStage::Tasks, Some(3)).unwrap();
    assert_eq!(world.actors()[0].control_state().unwrap().ai_delay, 2);
    assert_eq!(world.flags(), [73]);
}

#[test]
fn missing_public_stage_clock_refuses_before_script_writes_and_allows_retry() {
    let mut world = fixture(1, 8, 0, true);
    let before = serde_json::to_value(world.actors()[0].control_state().unwrap()).unwrap();
    let error = advance(&mut world, ScriptStage::Body(Thread::Main), None).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("spatial script stages require frame_scale and nominal_ticks")
    );
    assert_eq!(
        serde_json::to_value(world.actors()[0].control_state().unwrap()).unwrap(),
        before
    );
    assert_eq!(world.flags(), [12]);
    assert!(world.actors()[0].failure().is_none());
    advance(&mut world, ScriptStage::Body(Thread::Main), Some(3)).unwrap();
    let control = world.actors()[0].control_state().unwrap();
    assert_eq!(
        (
            control.ai_type.byte(),
            control.ai_location,
            control.ai_delay
        ),
        (4, 1, 5)
    );
    assert_eq!(world.actors()[0].attributes()[Attribute::Collide], 4);
    assert_eq!(world.flags(), [12]);
}
