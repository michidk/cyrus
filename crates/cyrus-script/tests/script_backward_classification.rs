//! Backward and lateral native tasks retain receiver-owned classifications.
use cyrus_assets::script::*;
use cyrus_script::script::MotionDirection;
use cyrus_script::script::{
    OrdinaryForwardAmountInputs, ScriptHud, ScriptInputs, ScriptSound, ScriptStage, ScriptWorld,
};
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

fn fixture(name: &str, present: bool, pending: bool, sound: bool, slide: u8) -> ScriptWorld {
    fixture_with_surface(name, present, pending, sound, slide, true)
}

fn fixture_with_surface(
    name: &str,
    present: bool,
    pending: bool,
    sound: bool,
    slide: u8,
    deep: bool,
) -> ScriptWorld {
    let mut scripts = cache();
    scripts.definition.references = vec![
        cyrus_soup::Property::SlideFlag,
        cyrus_soup::Property::UnderSurface,
    ];
    scripts.definition.functions.extend([
        Function {
            name: cyrus_soup::Native::from_name(name).expect("known fixture native"),
            param_count: 1,
            kind: CallKind::Task,
        },
        Function {
            name: cyrus_soup::Native::PlayAnimation,
            param_count: 2,
            kind: CallKind::Function,
        },
    ]);
    let actor = scripts.actors.get_mut("LOGIC").unwrap();
    actor.attributes[Attribute::BackwardSpeed] = 12;
    actor.attributes[Attribute::SideSpeed] = 4;
    actor.attributes[Attribute::ForwardSpeed] = 8;
    actor.attributes[Attribute::ForwardWalk] = 4;
    actor.attributes[Attribute::StepType] = 0;
    let mut operations = vec![Operation::Assign {
        destination: Place::Property {
            object: Object::Me,
            reference: 0,
        },
        formula: Formula {
            first: Value::Integer {
                value: i32::from(slide),
            },
            rest: vec![],
        },
    }];
    if name == "WalkBackward" && deep {
        operations.push(Operation::Assign {
            destination: Place::Property {
                object: Object::Me,
                reference: 1,
            },
            formula: Formula {
                first: Value::Integer { value: 2 },
                rest: vec![],
            },
        });
    }
    if pending {
        operations.push(Operation::Call {
            call: Call {
                target: Object::Me,
                function_id: 2,
                function: cyrus_soup::Native::PlayAnimation,
                kind: CallKind::Function,
                arguments: vec![Value::Integer { value: 30 }, Value::Integer { value: 0 }],
            },
        });
    }
    operations.extend([
        Operation::Call {
            call: Call {
                target: Object::Me,
                function_id: 1,
                function: cyrus_soup::Native::from_name(name).expect("known fixture native"),
                kind: CallKind::Task,
                arguments: vec![Value::Integer { value: 4 }],
            },
        },
        Operation::Assign {
            destination: Place::Flag { index: 0 },
            formula: Formula {
                first: Value::Integer { value: 77 },
                rest: vec![],
            },
        },
        Operation::End { target: 0 },
    ]);
    actor.program.steps = operations
        .into_iter()
        .enumerate()
        .map(|(index, operation)| {
            (
                index as u32,
                Step {
                    next: index as u32 + 1,
                    operation,
                },
            )
        })
        .collect();
    scripts.content_id = scripts.content_identity().unwrap();
    let group = match name {
        "MoveBackward" => 19,
        "WalkBackward" => 143,
        "MoveLeft" => 24,
        "MoveRight" => 26,
        _ => unreachable!(),
    };
    let mut groups = vec![
        serde_json::json!({"group_index":0,"anim_id":1,"anim_type":0,
        "commands":[{"opcode":0,"frame_reference":0},{"opcode":2,"value":0}]}),
    ];
    for index in [18, 19, 24, 26, 30, 142, 143] {
        if index == group && !present {
            continue;
        }
        let mut commands = vec![];
        if sound && index == group {
            commands.push(serde_json::json!({"opcode":4,"sound_param":-17,"volume_shift":0}));
        }
        commands.push(serde_json::json!({"opcode":0,"frame_reference":index}));
        commands.push(serde_json::json!({"opcode":2,"value":0}));
        groups.push(
            serde_json::json!({"group_index":index,"anim_id":1,"anim_type":0,"commands":commands}),
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
    world
        .bind_animation(
            world.id(0).unwrap(),
            &data,
            cyrus_soup::AnimationGroup::PANIC,
        )
        .unwrap();
    world
}

fn advance(
    world: &mut ScriptWorld,
    stage: ScriptStage,
    sounds: &mut dyn FnMut(cyrus_script::script::ScriptActorId, ScriptSound) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    advance_with_lateral(world, stage, true, sounds)
}

fn advance_with_lateral(
    world: &mut ScriptWorld,
    stage: ScriptStage,
    lateral: bool,
    sounds: &mut dyn FnMut(cyrus_script::script::ScriptActorId, ScriptSound) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    let hud = ScriptHud::default();
    world.advance(
        world.id(0)?,
        stage,
        ScriptInputs {
            nominal_ticks: Some(1),
            frame_scale: Some(256),
            hud: Some(&hud),
            shell_execution: Some(false),
            lateral_step_clear: Some(lateral),
            ..Default::default()
        },
        Some(sounds),
        None,
        30,
    )
}

#[test]
fn native_backward_and_strafes_publish_classification_and_resume_the_caller() {
    for (name, class, delta) in [
        (
            "MoveBackward",
            MotionDirection::WALK_BACKWARD,
            [0, 0, -3072],
        ),
        (
            "WalkBackward",
            MotionDirection::WALK_BACKWARD,
            [0, 0, -3072],
        ),
        ("MoveLeft", MotionDirection::LEFT, [-1024, 0, 0]),
        ("MoveRight", MotionDirection::RIGHT, [1024, 0, 0]),
    ] {
        let mut world = fixture(name, true, false, false, 0);
        let id = world.id(0).unwrap();
        advance(&mut world, ScriptStage::Body(Thread::Main), &mut |_, _| {
            Ok(())
        })
        .unwrap();
        for update in 1..=4 {
            assert_eq!(
                world.actors()[0]
                    .control_state()
                    .unwrap()
                    .motion_classification,
                class,
                "{name}"
            );
            assert_eq!(
                world.actors()[1]
                    .control_state()
                    .unwrap()
                    .motion_classification,
                MotionDirection::NONE
            );
            assert_eq!(
                world.spatial(id).unwrap().local.position,
                delta.map(|axis| axis * update)
            );
            if update < 4 {
                assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 1);
                assert_eq!(world.flags(), [12]);
                advance(&mut world, ScriptStage::Tasks, &mut |_, _| Ok(())).unwrap();
            }
        }
        assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
        advance(&mut world, ScriptStage::Body(Thread::Main), &mut |_, _| {
            Ok(())
        })
        .unwrap();
        assert_eq!(world.flags(), [77]);
    }
}

#[test]
fn native_refusal_keeps_classification_zero_and_still_releases_the_caller() {
    for name in ["MoveBackward", "WalkBackward", "MoveLeft", "MoveRight"] {
        let mut world = fixture(name, false, false, false, 1);
        advance(&mut world, ScriptStage::Body(Thread::Main), &mut |_, _| {
            panic!("refusal dispatched audio")
        })
        .unwrap();
        assert_eq!(
            world.actors()[0]
                .control_state()
                .unwrap()
                .motion_classification,
            MotionDirection::NONE
        );
        assert_eq!(
            world.spatial(world.id(0).unwrap()).unwrap().local.position,
            [0; 3]
        );
        assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 1);
        advance(&mut world, ScriptStage::Tasks, &mut |_, _| {
            panic!("refusal dispatched audio")
        })
        .unwrap();
        assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
        assert_eq!(
            world.actors()[0]
                .control_state()
                .unwrap()
                .motion_classification,
            MotionDirection::NONE
        );
        advance(&mut world, ScriptStage::Body(Thread::Main), &mut |_, _| {
            Ok(())
        })
        .unwrap();
        assert_eq!(world.flags(), [77]);
    }
}

#[test]
fn missing_group_and_pending_group_retain_classification_without_startup() {
    for (name, class) in [
        ("MoveBackward", MotionDirection::WALK_BACKWARD),
        ("WalkBackward", MotionDirection::WALK_BACKWARD),
        ("MoveLeft", MotionDirection::LEFT),
        ("MoveRight", MotionDirection::RIGHT),
    ] {
        for (present, pending) in [(false, false), (true, true)] {
            let mut world = fixture(name, present, pending, false, 0);
            advance(&mut world, ScriptStage::Body(Thread::Main), &mut |_, _| {
                Ok(())
            })
            .unwrap();
            let control = world.actors()[0].control_state().unwrap();
            assert_eq!(control.motion_classification, class);
            assert_eq!(control.move_counter, 0);
            assert_eq!(world.inspect(0, 1)["globals"]["locomotion_called"], 0);
            assert_eq!(
                world.spatial(world.id(0).unwrap()).unwrap().local.position,
                [0; 3]
            );
            assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 1);
            assert_eq!(world.flags(), [12]);
        }
    }
}

#[test]
fn failed_native_animation_callback_keeps_classification_and_stops_the_caller() {
    for (name, class) in [
        ("MoveBackward", MotionDirection::WALK_BACKWARD),
        ("WalkBackward", MotionDirection::WALK_BACKWARD),
        ("MoveLeft", MotionDirection::LEFT),
        ("MoveRight", MotionDirection::RIGHT),
    ] {
        let mut world = fixture(name, true, false, true, 0);
        let mut calls = 0;
        let error = advance(&mut world, ScriptStage::Body(Thread::Main), &mut |_, _| {
            calls += 1;
            anyhow::bail!("native sound unavailable")
        })
        .unwrap_err();
        assert!(format!("{error:#}").contains("native sound unavailable"));
        assert_eq!(calls, 1);
        let control = world.actors()[0].control_state().unwrap();
        assert_eq!(control.motion_classification, class);
        assert_eq!(control.move_counter, 0);
        assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
        assert!(world.actors()[0].runtime().unwrap().failure().is_some());
        assert_eq!(world.flags(), [12]);
        assert_eq!(
            world.spatial(world.id(0).unwrap()).unwrap().local.position,
            [0; 3]
        );
    }
}

#[test]
fn native_poll_rewrites_classification_after_another_public_movement_producer() {
    for (name, class) in [
        ("MoveBackward", MotionDirection::WALK_BACKWARD),
        ("WalkBackward", MotionDirection::WALK_BACKWARD),
        ("MoveLeft", MotionDirection::LEFT),
        ("MoveRight", MotionDirection::RIGHT),
    ] {
        let mut world = fixture(name, true, false, false, 0);
        advance(&mut world, ScriptStage::Body(Thread::Main), &mut |_, _| {
            Ok(())
        })
        .unwrap();
        let id = world.id(0).unwrap();
        world
            .ordinary_forward_amount(
                id,
                OrdinaryForwardAmountInputs {
                    selected_player: None,
                    holding_weapon: false,
                    frame_scale: 256,
                    ticks: 1,
                    duration: 4,
                    elapsed: 0,
                },
                None,
                &mut |_, _| Ok(()),
            )
            .unwrap();
        assert_eq!(
            world.actors()[0]
                .control_state()
                .unwrap()
                .motion_classification,
            MotionDirection::FORWARD
        );
        advance(&mut world, ScriptStage::Tasks, &mut |_, _| Ok(())).unwrap();
        assert_eq!(
            world.actors()[0]
                .control_state()
                .unwrap()
                .motion_classification,
            class
        );
        assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 1);
        assert_eq!(world.flags(), [12]);
    }
}

#[test]
fn refused_lateral_query_retains_strafe_classification_and_releases_caller() {
    for (name, class) in [
        ("MoveLeft", MotionDirection::LEFT),
        ("MoveRight", MotionDirection::RIGHT),
    ] {
        let mut world = fixture(name, true, false, true, 0);
        advance_with_lateral(
            &mut world,
            ScriptStage::Body(Thread::Main),
            false,
            &mut |_, _| panic!("rejected lateral step requested sound"),
        )
        .unwrap();
        assert_eq!(
            world.actors()[0]
                .control_state()
                .unwrap()
                .motion_classification,
            class
        );
        assert_eq!(
            world.spatial(world.id(0).unwrap()).unwrap().local.position,
            [0; 3]
        );
        advance_with_lateral(&mut world, ScriptStage::Tasks, false, &mut |_, _| {
            panic!("rejected lateral poll requested sound")
        })
        .unwrap();
        assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
        advance(&mut world, ScriptStage::Body(Thread::Main), &mut |_, _| {
            Ok(())
        })
        .unwrap();
        assert_eq!(world.flags(), [77]);
    }
}

#[test]
fn dry_walk_backward_missing_support_fails_before_classification_store() {
    let mut world = fixture_with_surface("WalkBackward", true, false, false, 0, false);
    let error = advance(&mut world, ScriptStage::Body(Thread::Main), &mut |_, _| {
        Ok(())
    })
    .unwrap_err();
    assert!(format!("{error:#}").contains("WalkBackward step check input unavailable"));
    assert_eq!(
        world.actors()[0]
            .control_state()
            .unwrap()
            .motion_classification,
        MotionDirection::NONE
    );
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
    assert!(world.actors()[0].runtime().unwrap().failure().is_some());
    assert_eq!(world.flags(), [12]);
}
