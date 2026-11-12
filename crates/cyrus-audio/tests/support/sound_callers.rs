//! Synthetic SOUP callers shared by output acceptance and public ownership regressions.
pub fn separate_sound_world(shells: bool) -> cyrus_script::script::ScriptWorld {
    use cyrus_assets::script::{Actor, Scripts};
    use cyrus_soup::program::{
        Call, CallKind, Flag, Formula, Function, Object, Operation, Place, Program, Step, Value,
    };
    use cyrus_soup::{Attributes, Native};
    use std::collections::BTreeMap;

    let mut scripts: Scripts = serde_json::from_value(serde_json::json!({
        "version":30,"weapons":{},"combat_clank_effects":[null,null],
        "combat_defenses":[],"combat_attacks":[],"world":7,"content_id":"",
        "actor_pool_capacity":32,"player_distance_override":false,
        "clock":{"normal_frame_rate":12,"min_frame_rate":6,"max_frame_rate":300,
            "use_smooth_fps":true,"use_smooth_divisor":true},
        "updates":{"fall_death_ticks":144,"task_system":true,"animation_system":true,
            "disable_slaves":false,"disable_master_slaves":false,"dummy_model_manager":false},
        "dialogue":{"menu_start":[35,25],"max_menu_items":300,"max_dialog_lines":20,
            "max_text_width":500,"menu_traverse_delay":4,"print_text":true,
            "use_speech":true,"max_distance":1600,"rtx_filename":"ENGLISH.RTX"},
        "definition":{"functions":[],"flags":[],"references":[],"attributes":[]},
        "actors":{},"unsupported":{},"instances":[]
    }))
    .unwrap();
    let definitions = [
        (Native::NullFunction, 0),
        (Native::FlatSound, 3),
        (Native::EndMySound, 1),
        (Native::EndMySounds, 0),
        (Native::KillMe, 0),
        (Native::StopAllSounds, 0),
    ];
    scripts.definition.flags.push(Flag {
        name: "Continued".into(),
        flag_type: "NUMBER".into(),
        initial_value: 99,
    });
    scripts.definition.functions = definitions
        .iter()
        .map(|&(name, param_count)| Function {
            name,
            param_count,
            kind: CallKind::Function,
        })
        .collect();
    let call = |function: Native, arguments: &[i32]| Operation::Call {
        call: Call {
            function_id: definitions
                .iter()
                .position(|&(name, _)| name == function)
                .unwrap() as u16,
            function,
            kind: CallKind::Function,
            target: Object::Me,
            arguments: arguments
                .iter()
                .map(|&value| Value::Integer { value })
                .collect(),
        },
    };
    let start = || {
        [
            call(Native::FlatSound, &[0, 0, 0]),
            call(Native::FlatSound, &[1, 0, 0]),
        ]
    };
    let stopper = Vec::from(start())
        .into_iter()
        .chain([
            Operation::End { target: 3 },
            call(Native::EndMySound, &[0]),
            Operation::End { target: 5 },
            call(Native::EndMySounds, &[]),
            Operation::End { target: 7 },
            call(Native::FlatSound, &[1, 0, 0]),
            call(Native::KillMe, &[]),
            Operation::End { target: 9 },
        ])
        .collect::<Vec<_>>();
    let keeper = Vec::from(start())
        .into_iter()
        .chain([
            Operation::End { target: 3 },
            Operation::End { target: 4 },
            Operation::End { target: 5 },
            Operation::End { target: 6 },
            Operation::End { target: 7 },
            call(Native::StopAllSounds, &[]),
            Operation::Assign {
                destination: Place::Flag { index: 0 },
                formula: Formula {
                    first: Value::Integer { value: 7 },
                    rest: vec![],
                },
            },
            Operation::End { target: 10 },
            call(Native::FlatSound, &[1, 0, 0]),
            Operation::End { target: 12 },
            Operation::Assign {
                destination: Place::Flag { index: 0 },
                formula: Formula {
                    first: Value::Call {
                        call: Box::new(match call(Native::StopAllSounds, &[]) {
                            Operation::Call { call } => call,
                            _ => unreachable!(),
                        }),
                    },
                    rest: vec![],
                },
            },
            Operation::End { target: 13 },
        ])
        .collect();
    for (index, (name, operations)) in [("STOPPER", stopper), ("KEEPER", keeper)]
        .into_iter()
        .enumerate()
    {
        scripts.actors.insert(
            name.into(),
            Actor {
                header_index: index,
                code_origin: (index * 100) as u32,
                instances: 1,
                namespace: None,
                instance_variables: vec![vec![]],
                location_offsets: vec![],
                attributes: Attributes::default(),
                combat_taunts: vec![],
                native_hook: None,
                program: Program {
                    version: 4,
                    entry: 0,
                    variables: vec![],
                    strings: vec![],
                    steps: operations
                        .into_iter()
                        .enumerate()
                        .map(|(step, operation)| {
                            (
                                step as u32,
                                Step {
                                    next: step as u32 + 1,
                                    operation,
                                },
                            )
                        })
                        .collect::<BTreeMap<_, _>>(),
                },
            },
        );
        if shells {
            scripts.shells.push(name.into());
        } else {
            scripts.instances.push(cyrus_assets::script::Instance {
                object_id: index as u32,
                actor: name.into(),
                object_type: cyrus_assets::script::PlacementType::SCRIPT_ONLY,
                active: 1,
                model_name: String::new(),
                model_mode: 0,
                light_intensity: 0,
                light_radius: 0,
                light_rgb: [0; 3],
                texture: 0,
                map_position: [600, 20, 600],
                angles: [0; 3],
            });
        }
    }
    scripts.content_id = scripts.content_identity().unwrap();
    cyrus_script::script::ScriptWorld::prepare(&scripts, 17).unwrap()
}
