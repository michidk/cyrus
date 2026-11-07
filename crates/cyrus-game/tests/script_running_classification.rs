//! Forward amount queries retain classification before animation admission.
use cyrus_assets::script::*;
use cyrus_game::script::MotionDirection;
use cyrus_game::script::{OrdinaryForwardAmountInputs, ScriptInputs, ScriptStage, ScriptWorld};
use cyrus_soup::program::*;
use cyrus_soup::{Attribute, Thread};
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
        view_distance: None,
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
                name: cyrus_soup::Native::NullFunction,
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
                object_type: Default::default(),
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

fn fixture(
    group_present: bool,
    sound: bool,
    collide: u8,
    living: u8,
    pending: bool,
) -> ScriptWorld {
    let mut scripts = cache();
    scripts.definition.references = vec![cyrus_soup::Property::ExtraFlag];
    let actor = scripts.actors.get_mut("LOGIC").unwrap();
    actor.attributes[Attribute::ForwardSpeed] = 8;
    actor.attributes[Attribute::StepType] = 1;
    actor.attributes[Attribute::Collide] = collide;
    actor.program.steps = BTreeMap::from([
        (
            0,
            Step {
                next: 1,
                operation: Operation::Assign {
                    destination: Place::Property {
                        object: Object::Me,
                        reference: 0,
                    },
                    formula: Formula {
                        first: Value::Integer {
                            value: i32::from(living),
                        },
                        rest: vec![],
                    },
                },
            },
        ),
        (
            1,
            Step {
                next: 2,
                operation: Operation::End { target: 0 },
            },
        ),
    ]);
    if pending {
        scripts.definition.functions.push(Function {
            name: cyrus_soup::Native::PlayAnimation,
            param_count: 2,
            kind: CallKind::Function,
        });
        actor.program.steps.insert(
            1,
            Step {
                next: 2,
                operation: Operation::Assign {
                    destination: Place::Local { index: 0 },
                    formula: Formula {
                        first: Value::Call {
                            call: Box::new(Call {
                                target: Object::Me,
                                function_id: 1,
                                function: cyrus_soup::Native::PlayAnimation,
                                kind: CallKind::Function,
                                arguments: vec![
                                    Value::Integer { value: 30 },
                                    Value::Integer { value: 0 },
                                ],
                            }),
                        },
                        rest: vec![],
                    },
                },
            },
        );
        actor.program.steps.insert(
            2,
            Step {
                next: 3,
                operation: Operation::End { target: 0 },
            },
        );
    }
    scripts.content_id = scripts.content_identity().unwrap();
    let mut groups = vec![
        serde_json::json!({"group_index":0,"anim_id":1,"anim_type":0,
        "commands":[{"opcode":0,"frame_reference":0},{"opcode":2,"value":0}]}),
    ];
    if pending {
        groups.push(
            serde_json::json!({"group_index":30,"anim_id":1,"anim_type":0,
        "commands":[{"opcode":0,"frame_reference":30},{"opcode":2,"value":0}]}),
        );
    }
    if group_present {
        let mut commands = vec![];
        if sound {
            commands.push(serde_json::json!({"opcode":4,"sound_param":-17,"volume_shift":0}));
        }
        commands.push(serde_json::json!({"opcode":0,"frame_reference":18}));
        commands.push(serde_json::json!({"opcode":2,"value":0}));
        groups.push(
            serde_json::json!({"group_index":18,"anim_id":1,"anim_type":0,"commands":commands}),
        );
    }
    let data = cyrus_assets::animation::Animations {
        version: 2,
        world: scripts.world,
        metadata: serde_json::json!({"script_content_id":scripts.content_id,
            "actors":[{"script_name":"LOGIC","animation_groups":groups}]}),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    };
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world.prepare_spatial().unwrap();
    let id = world.id(0).unwrap();
    world
        .bind_animation(id, &data, cyrus_soup::AnimationGroup::PANIC)
        .unwrap();
    world
        .advance(
            id,
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
    world
}

fn inputs() -> OrdinaryForwardAmountInputs {
    OrdinaryForwardAmountInputs {
        selected_player: None,
        holding_weapon: false,
        frame_scale: 256,
        ticks: 1,
        duration: 4,
        elapsed: 0,
    }
}

#[test]
fn admitted_amount_sets_receiver_classification_without_translating_or_touching_neighbor() {
    let mut world = fixture(true, false, 0, 0, false);
    let id = world.id(0).unwrap();
    let position = world.spatial(id).unwrap().local.position;
    assert_eq!(
        world.actors()[0]
            .control_state()
            .unwrap()
            .motion_classification,
        MotionDirection::NONE
    );
    assert_eq!(
        world
            .ordinary_forward_amount(id, inputs(), None, &mut |_, _| Ok(()))
            .unwrap(),
        512
    );
    let control = world.actors()[0].control_state().unwrap();
    assert_eq!(control.motion_classification, MotionDirection::FORWARD);
    assert_eq!(control.motion_direction, MotionDirection::FORWARD);
    assert_eq!(control.move_counter, 0x10);
    assert_eq!(
        world.actors()[1]
            .control_state()
            .unwrap()
            .motion_classification,
        MotionDirection::NONE
    );
    assert_eq!(world.spatial(id).unwrap().local.position, position);
    assert_eq!(world.inspect(0, 1)["globals"]["locomotion_called"], 4);
}

#[test]
fn living_refusal_precedes_classification_animation_and_support_requirements() {
    let mut world = fixture(false, false, 2, 1, false);
    let id = world.id(0).unwrap();
    assert_eq!(
        world.actors()[0]
            .control_state()
            .unwrap()
            .living_state
            .byte(),
        1
    );
    assert_eq!(
        world
            .ordinary_forward_amount(id, inputs(), None, &mut |_, _| panic!(
                "refused amount reached sound callback"
            ))
            .unwrap(),
        0
    );
    let control = world.actors()[0].control_state().unwrap();
    assert_eq!(control.motion_classification, MotionDirection::NONE);
    assert_eq!(control.motion_direction, MotionDirection::NONE);
    assert_eq!(control.move_counter, 0);
    assert_eq!(world.inspect(0, 1)["globals"]["locomotion_called"], 0);
}

#[test]
fn unavailable_support_does_not_publish_running_classification() {
    let mut world = fixture(false, false, 2, 0, false);
    let id = world.id(0).unwrap();
    let error = world
        .ordinary_forward_amount(id, inputs(), None, &mut |_, _| {
            panic!("missing support reached sound callback")
        })
        .unwrap_err();
    let error = format!("{error:#}");
    assert!(
        error.contains("world 7 instance 0 forward amount"),
        "{error}"
    );
    assert!(error.contains("step geometry unavailable"), "{error}");
    assert_eq!(
        world.actors()[0]
            .control_state()
            .unwrap()
            .motion_classification,
        MotionDirection::NONE
    );
    assert_eq!(world.inspect(0, 1)["globals"]["locomotion_called"], 0);
}

#[test]
fn missing_group_keeps_classification_but_does_not_advance_startup() {
    let mut world = fixture(false, false, 0, 0, false);
    let id = world.id(0).unwrap();
    let error = world
        .ordinary_forward_amount(id, inputs(), None, &mut |_, _| {
            panic!("missing group reached sound callback")
        })
        .unwrap_err();
    let error = format!("{error:#}");
    assert!(
        error.contains("world 7 instance 0 forward amount"),
        "{error}"
    );
    assert!(error.contains("animation group 18 unavailable"), "{error}");
    let control = world.actors()[0].control_state().unwrap();
    assert_eq!(control.motion_classification, MotionDirection::FORWARD);
    assert_eq!(control.move_counter, 0);
    assert_eq!(world.inspect(0, 1)["globals"]["locomotion_called"], 0);
}

#[test]
fn failed_sound_callback_keeps_classification_and_stops_before_startup() {
    let mut world = fixture(true, true, 0, 0, false);
    let id = world.id(0).unwrap();
    let mut calls = 0;
    let error = world
        .ordinary_forward_amount(id, inputs(), None, &mut |receiver, _| {
            assert_eq!(receiver, id);
            calls += 1;
            anyhow::bail!("authored sound refused")
        })
        .unwrap_err();
    let error = format!("{error:#}");
    assert_eq!(calls, 1);
    assert!(
        error.contains("world 7 instance 0 forward amount"),
        "{error}"
    );
    assert!(error.contains("authored sound refused"), "{error}");
    let control = world.actors()[0].control_state().unwrap();
    assert_eq!(control.motion_classification, MotionDirection::FORWARD);
    assert_eq!(control.move_counter, 0);
    assert_eq!(world.inspect(0, 1)["globals"]["locomotion_called"], 0);
    assert!(world.actors()[0].animation_failure().is_some());
}

#[test]
fn pending_animation_keeps_running_classification_before_startup_admission() {
    let mut world = fixture(true, false, 0, 0, true);
    let id = world.id(0).unwrap();
    assert_eq!(
        world.inspect(0, 1)["actors"][0]["animation"]["current_group"],
        30
    );
    assert_eq!(
        world.actors()[0]
            .control_state()
            .unwrap()
            .motion_classification,
        MotionDirection::NONE
    );
    assert_eq!(
        world
            .ordinary_forward_amount(id, inputs(), None, &mut |_, _| Ok(()))
            .unwrap(),
        1
    );
    let control = world.actors()[0].control_state().unwrap();
    assert_eq!(control.motion_classification, MotionDirection::FORWARD);
    assert_eq!(control.move_counter, 0);
    let state = world.inspect(0, 1);
    assert_eq!(state["globals"]["locomotion_called"], 0);
    assert_eq!(state["actors"][0]["animation"]["current_group"], 30);
    assert_eq!(state["actors"][0]["animation"]["pending_group"], 18);
}
