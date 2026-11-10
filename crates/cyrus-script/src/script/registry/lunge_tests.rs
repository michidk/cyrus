//! Lunge and ordinary continuation retain classification at their own admission boundaries.
use crate::script::MotionDirection;
use crate::script::{ScriptHud, ScriptInputs, ScriptSound, ScriptStage, ScriptWorld};
use cyrus_assets::script::*;
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

fn fixture(group: u16, slide: u8, spatial: bool) -> ScriptWorld {
    let mut scripts = cache();
    scripts.definition.references = vec![cyrus_soup::Property::SlideFlag];
    scripts.definition.functions.extend([
        Function {
            name: cyrus_soup::Native::PlayAnimation,
            param_count: 2,
            kind: CallKind::Function,
        },
        Function {
            name: cyrus_soup::Native::Wait,
            param_count: 1,
            kind: CallKind::Task,
        },
    ]);
    let actor = scripts.actors.get_mut("LOGIC").unwrap();
    actor.attributes[Attribute::ForwardSpeed] = 8;
    actor.attributes[Attribute::BackwardSpeed] = 12;
    actor.attributes[Attribute::FightLungeSpeed] = 6;
    actor.attributes[Attribute::StepType] = 0;
    let operations = [
        Operation::Assign {
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
        },
        Operation::Call {
            call: Call {
                target: Object::Me,
                function_id: 1,
                function: cyrus_soup::Native::PlayAnimation,
                kind: CallKind::Function,
                arguments: vec![
                    Value::Integer {
                        value: i32::from(group),
                    },
                    Value::Integer { value: 0 },
                ],
            },
        },
        Operation::Call {
            call: Call {
                target: Object::Me,
                function_id: 2,
                function: cyrus_soup::Native::Wait,
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
    ];
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
    let data = cyrus_assets::animation::Animations {
        version: 2,
        world: scripts.world,
        metadata: serde_json::json!({"script_content_id":scripts.content_id,"actors":[{
            "script_name":"LOGIC","animation_groups":([0,18,19,20,22,24,26].into_iter().map(|g|
                serde_json::json!({"group_index":g,"anim_id":1,"anim_type":0,
                    "commands":[{"opcode":0,"frame_reference":g},{"opcode":2,"value":0}]})).collect::<Vec<_>>()) }]}),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    };
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    if spatial {
        world.prepare_spatial().unwrap();
    }
    world
        .bind_animation(
            world.id(0).unwrap(),
            &data,
            cyrus_soup::AnimationGroup::PANIC,
        )
        .unwrap();
    advance(&mut world, ScriptStage::Body(Thread::Main)).unwrap();
    world
}

fn advance(world: &mut ScriptWorld, stage: ScriptStage) -> anyhow::Result<()> {
    let hud = ScriptHud::default();
    world.advance(
        world.id(0)?,
        stage,
        ScriptInputs {
            nominal_ticks: Some(1),
            frame_scale: Some(256),
            hud: Some(&hud),
            shell_execution: Some(false),
            ..Default::default()
        },
        Some(&mut |_, _| Ok(())),
        None,
        30,
    )
}

fn lunge(world: &mut ScriptWorld, clear: bool, ticks: Option<u32>) -> anyhow::Result<()> {
    let hud = ScriptHud::default();
    world.step_player_operation(
        world.id(0)?,
        crate::script::tasks::LocomotionOperation::Lunge,
        ScriptInputs {
            nominal_ticks: ticks,
            frame_scale: Some(256),
            hud: Some(&hud),
            shell_execution: Some(false),
            forward_step_clear: Some(clear),
            ..Default::default()
        },
        &mut |_, _: ScriptSound| panic!("lunge requested animation sound"),
        None,
    )
}

fn continuation(world: &mut ScriptWorld) -> anyhow::Result<()> {
    let hud = ScriptHud::default();
    world.continue_ordinary_motion(
        world.id(0)?,
        ScriptInputs {
            nominal_ticks: Some(1),
            frame_scale: Some(256),
            hud: Some(&hud),
            shell_execution: Some(false),
            ..Default::default()
        },
        None,
        &mut |_, _| panic!("continuation requested animation sound"),
    )
}

#[test]
fn lunge_stores_classification_without_creating_a_task_or_requesting_animation() {
    let mut world = fixture(0, 0, true);
    for step in 1..=2 {
        lunge(&mut world, true, Some(1)).unwrap();
        assert_eq!(
            world.actors()[0]
                .control_state()
                .unwrap()
                .motion_classification,
            MotionDirection::FORWARD
        );
        assert_eq!(
            world.actors()[1]
                .control_state()
                .unwrap()
                .motion_classification,
            MotionDirection::NONE
        );
        assert_eq!(
            world.spatial(world.id(0).unwrap()).unwrap().local.position,
            [0, 0, 1536 * step]
        );
        assert_eq!(
            world.actors()[0].runtime().unwrap().active_tasks(),
            1,
            "only the real Wait task remains"
        );
        assert_eq!(
            world.animation_frame(world.id(0).unwrap()).unwrap(),
            Some(0)
        );
    }
    advance(&mut world, ScriptStage::Tasks).unwrap();
    assert_eq!(
        world.actors()[0]
            .control_state()
            .unwrap()
            .motion_classification,
        MotionDirection::FORWARD
    );
    assert_eq!(world.flags(), [12]);
}

#[test]
fn lunge_refuses_before_classification_for_slide_or_rejected_startup_check() {
    for (slide, clear) in [(1, true), (0, false)] {
        let mut world = fixture(0, slide, true);
        lunge(&mut world, clear, Some(1)).unwrap();
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
        assert_eq!(world.inspect(0, 1)["globals"]["locomotion_called"], 0);
    }
}

#[test]
fn lunge_missing_animation_timing_retains_admitted_classification_before_failure() {
    let mut world = fixture(0, 0, true);
    let error = lunge(&mut world, true, None).unwrap_err();
    assert!(format!("{error:#}").contains("Lunge requires prepared animation"));
    assert_eq!(
        world.actors()[0]
            .control_state()
            .unwrap()
            .motion_classification,
        MotionDirection::FORWARD
    );
    assert_eq!(world.actors()[0].control_state().unwrap().move_counter, 0);
    assert_eq!(
        world.spatial(world.id(0).unwrap()).unwrap().local.position,
        [0; 3]
    );
    assert_eq!(world.flags(), [12]);
}

#[test]
fn forward_and_backward_continuation_store_classification_once_and_preserve_script_poll() {
    for (group, class, delta) in [
        (18, MotionDirection::FORWARD, 2048),
        (19, MotionDirection::BACKWARD, -3072),
    ] {
        let mut world = fixture(group, 0, true);
        advance(&mut world, ScriptStage::Tasks).unwrap();
        assert_eq!(
            world.actors()[0]
                .control_state()
                .unwrap()
                .motion_classification,
            MotionDirection::NONE
        );
        continuation(&mut world).unwrap();
        assert_eq!(
            world.actors()[0]
                .control_state()
                .unwrap()
                .motion_classification,
            class
        );
        assert_eq!(
            world.spatial(world.id(0).unwrap()).unwrap().local.position,
            [0, 0, delta]
        );
        continuation(&mut world).unwrap();
        assert_eq!(
            world.spatial(world.id(0).unwrap()).unwrap().local.position,
            [0, 0, delta]
        );
        assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 1);
        assert_eq!(world.flags(), [12]);
        for _ in 0..4 {
            advance(&mut world, ScriptStage::Tasks).unwrap();
        }
        assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
        advance(&mut world, ScriptStage::Body(Thread::Main)).unwrap();
        assert_eq!(world.flags(), [77]);
    }
}

#[test]
fn dry_walk_and_sidestep_keep_classification_when_support_service_is_unavailable() {
    for (group, class) in [
        (20, MotionDirection::WALK_FORWARD),
        (22, MotionDirection::WALK_BACKWARD),
        (24, MotionDirection::LEFT),
        (26, MotionDirection::RIGHT),
    ] {
        let mut world = fixture(group, 0, true);
        continuation(&mut world).unwrap();
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
        assert_eq!(world.inspect(0, 1)["globals"]["locomotion_called"], 0);
    }
}

#[test]
fn refused_continuation_preserves_classification_and_late_spatial_failure_retains_store() {
    let mut refused = fixture(18, 1, true);
    continuation(&mut refused).unwrap();
    assert_eq!(
        refused.actors()[0]
            .control_state()
            .unwrap()
            .motion_classification,
        MotionDirection::NONE
    );
    let mut failed = fixture(18, 0, false);
    let error = continuation(&mut failed).unwrap_err();
    assert!(format!("{error:#}").contains("spatial"));
    assert_eq!(
        failed.actors()[0]
            .control_state()
            .unwrap()
            .motion_classification,
        MotionDirection::FORWARD
    );
    assert_eq!(failed.actors()[0].control_state().unwrap().move_counter, 16);
    assert_eq!(failed.flags(), [12]);
}
