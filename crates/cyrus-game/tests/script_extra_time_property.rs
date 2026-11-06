//! Receiver-owned extra-time countdown uses public property dispatch.
use cyrus_assets::script::PlacementType;
use cyrus_assets::script::*;
use cyrus_game::script::{ScriptInputs, ScriptStage, ScriptWorld};
use cyrus_script::Thread;
use cyrus_script::program::*;
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

fn counter_scripts(object: Object, value: i32, write: bool) -> Scripts {
    let mut scripts = cache();
    scripts.definition.references = vec![cyrus_script::Property::ExtraTime; 295];
    let place = Place::Property {
        object,
        reference: 294,
    };
    let assign = |destination, first| Operation::Assign {
        destination,
        formula: Formula {
            first,
            rest: vec![],
        },
    };
    let mut operations = Vec::new();
    if write {
        operations.push(assign(place.clone(), Value::Integer { value }));
    }
    operations.push(assign(Place::Local { index: 0 }, Value::Read { place }));
    operations.push(assign(
        Place::Flag { index: 0 },
        Value::Integer { value: 73 },
    ));
    operations.push(Operation::End { target: 0 });
    scripts.actors.get_mut("LOGIC").unwrap().program.steps = operations
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
    scripts
}

fn advance(world: &mut ScriptWorld, caller: usize) -> anyhow::Result<()> {
    world.advance(
        world.id(caller)?,
        ScriptStage::Body(Thread::Main),
        ScriptInputs {
            nominal_ticks: Some(1),
            frame_scale: Some(256),
            ..Default::default()
        },
        None,
        None,
        20,
    )?;
    Ok(())
}

#[test]
fn extra_time_routes_low_bytes_and_continues_without_changing_other_state() {
    for (value, expected) in [
        (0, 0),
        (2, 2),
        (3, 3),
        (6, 6),
        (7, 7),
        (25, 25),
        (-1, 255),
        (258, 2),
        (256, 0),
    ] {
        for (object, receiver) in [(Object::Me, 1), (Object::Named { string: 0 }, 0)] {
            let scripts = counter_scripts(object, value, true);
            let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
            let mut control =
                serde_json::to_value(world.actors()[receiver].control_state().unwrap()).unwrap();
            advance(&mut world, 1).unwrap();
            assert_eq!(world.actors()[1].runtime().unwrap().variables(), [expected]);
            assert_eq!(world.flags(), [73]);
            control["extra_time"] = expected.into();
            assert_eq!(
                serde_json::to_value(world.actors()[receiver].control_state().unwrap()).unwrap(),
                control
            );
            assert_eq!(
                world.actors()[1 - receiver]
                    .control_state()
                    .unwrap()
                    .extra_time,
                0
            );
        }
    }
}

#[test]
fn extra_time_reads_allocation_zero_without_spatial_or_animation_preparation() {
    let scripts = counter_scripts(Object::Me, 99, false);
    let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
    advance(&mut world, 0).unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().variables(), [0]);
    assert_eq!(world.flags(), [73]);
}

#[test]
fn extra_time_missing_owner_or_receiver_keeps_a_sticky_failure() {
    for write in [false, true] {
        for missing_receiver in [false, true] {
            let object = if missing_receiver {
                Object::Player
            } else {
                Object::Me
            };
            let mut scripts = counter_scripts(object, -12345, write);
            if !missing_receiver {
                scripts.instances[0].object_type = PlacementType::SCRIPT_ONLY;
            }
            scripts.content_id = scripts.content_identity().unwrap();
            let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
            let error = advance(&mut world, 0).unwrap_err();
            let expected = if missing_receiver {
                "Player property selected player unavailable"
            } else {
                "extra_time control owner unavailable"
            };
            assert!(format!("{error:#}").contains(expected), "{error:#}");
            assert_eq!(world.actors()[0].runtime().unwrap().variables(), [4]);
            assert_eq!(world.flags(), [12]);
            let failure = world.actors()[0].failure().unwrap().to_owned();
            assert!(advance(&mut world, 0).is_err());
            assert_eq!(world.actors()[0].failure(), Some(failure.as_str()));
            assert_eq!(world.flags(), [12]);
        }
    }
}

#[test]
fn extra_time_is_consumed_by_ordinary_updates_and_preserves_sentinels() {
    use cyrus_game::script::OrdinaryInputs;
    for (value, ticks, expected) in [
        (0, 3, 0),
        (255, 3, 255),
        (7, 3, 4),
        (3, 3, 0),
        (2, 3, 0),
        (254, 256, 0),
        (129, 1, 128),
        (7, 0, 7),
    ] {
        let mut scripts = counter_scripts(Object::Me, value, true);
        for instance in &mut scripts.instances {
            instance.object_type = PlacementType::MODEL;
            instance.active = 1;
        }
        // Seed once through the public property caller, then park beyond the writer.
        scripts
            .actors
            .get_mut("LOGIC")
            .unwrap()
            .program
            .steps
            .get_mut(&3)
            .unwrap()
            .operation = Operation::End { target: 1 };
        scripts.content_id = scripts.content_identity().unwrap();
        let data = cyrus_assets::animation::Animations {
            version: 2,
            world: scripts.world,
            metadata: serde_json::json!({"script_content_id": scripts.content_id,
                "actors": [{"script_name":"LOGIC","model_load_path":"none"}], "markers": [], "walk_node_maps": []}),
            sections: Default::default(),
            models: Default::default(),
            unsupported: Default::default(),
        };
        let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
        world.prepare_spatial().unwrap();
        advance(&mut world, 0).unwrap();
        world
            .update_ordinary(
                &data,
                [0, 0],
                OrdinaryInputs {
                    scene: None,
                    scripts: ScriptInputs {
                        nominal_ticks: Some(ticks),
                        frame_scale: Some(256),
                        ..Default::default()
                    },
                },
                &mut cyrus_game::random::EngineRandom::default(),
                &mut |_, _| Ok(()),
                20,
            )
            .unwrap();
        assert_eq!(
            world.actors()[0].control_state().unwrap().extra_time,
            expected
        );
        assert_eq!(
            world.actors()[0].runtime().unwrap().variables(),
            [i32::from(expected)]
        );
        assert!(
            world.actors()[0].failure().is_none(),
            "{:?}",
            world.actors()[0].failure()
        );
    }
}

fn inventory_asset(counts: &[(u16, u32)]) -> cyrus_assets::hud::HudAsset {
    cyrus_assets::hud::HudAsset {
        labels: Default::default(),
        images: vec![],
        candle_mode: 0,
        candle: [0; 2],
        candle_size: [0; 2],
        pickup: [0; 2],
        prompt: [0; 2],
        override_ticks: 0,
        compass: [0; 2],
        compass_size: [0; 2],
        world_compass: Default::default(),
        powerup_slots: [[0; 2]; 2],
        powerup_sizes: vec![],
        logbook: [0; 2],
        logbook_size: [0; 2],
        walk_mode: false,
        auto_defend: false,
        notices: Default::default(),
        inventory: counts
            .iter()
            .map(|&(id, count)| {
                (
                    id,
                    cyrus_assets::hud::InventoryItem {
                        count,
                        total: 100,
                        player_max: 100,
                        ..Default::default()
                    },
                )
            })
            .collect(),
        start_items: vec![],
        items: vec![],
        selected: None,
        bindings: [0; 15],
        font: Default::default(),
        text_alpha: [0; 2],
        player_text_alpha: [0; 2],
        used_text_alpha: [0; 2],
        torch_time: None,
        inventory_screen: Default::default(),
        island_map: Default::default(),
    }
}

#[test]
fn extra_time_torch_entry_resets_after_admission_and_retains_on_refusal_or_sound_failure() {
    for (function, group, living) in [("LightTorch", 149, 33), ("UnlightTorch", 150, 34)] {
        for (shell, failing) in [(false, false), (true, false), (false, true)] {
            let mut scripts = counter_scripts(Object::Me, 19, true);
            scripts
                .definition
                .references
                .push(cyrus_script::Property::RopeWait);
            scripts.definition.functions.push(Function {
                name: cyrus_script::Native::from_name(function).expect("known fixture native"),
                param_count: 0,
                kind: CallKind::Function,
            });
            let actor = scripts.actors.get_mut("LOGIC").unwrap();
            actor.program.steps.insert(
                1,
                Step {
                    next: 2,
                    operation: Operation::Assign {
                        destination: Place::Property {
                            object: Object::Me,
                            reference: 295,
                        },
                        formula: Formula {
                            first: Value::Integer { value: 23 },
                            rest: vec![],
                        },
                    },
                },
            );
            actor.program.steps.insert(
                2,
                Step {
                    next: 3,
                    operation: Operation::ObjectCall {
                        call: Call {
                            function_id: 1,
                            function: cyrus_script::Native::from_name(function)
                                .expect("known fixture native"),
                            target: Object::Me,
                            kind: CallKind::Function,
                            arguments: vec![],
                        },
                    },
                },
            );
            actor.program.steps.insert(
                3,
                Step {
                    next: 4,
                    operation: Operation::Assign {
                        destination: Place::Local { index: 0 },
                        formula: Formula {
                            first: Value::Read {
                                place: Place::Property {
                                    object: Object::Me,
                                    reference: 294,
                                },
                            },
                            rest: vec![],
                        },
                    },
                },
            );
            actor.program.steps.insert(
                4,
                Step {
                    next: 5,
                    operation: Operation::End { target: 4 },
                },
            );
            scripts.content_id = scripts.content_identity().unwrap();
            let mut commands = vec![];
            if failing {
                commands.push(serde_json::json!({"opcode":4,"sound_param":-17,"volume_shift":0}));
            }
            commands.push(serde_json::json!({"opcode":0,"frame_reference":0}));
            commands.push(serde_json::json!({"opcode":2,"value":0}));
            let data = cyrus_assets::animation::Animations {
                version: 2,
                world: scripts.world,
                metadata: serde_json::json!({"script_content_id":scripts.content_id,"actors":[{
                    "script_name":"LOGIC","animation_groups":[
                    {"group_index":0,"anim_id":1,"anim_type":0,"commands":[{"opcode":0,"frame_reference":0},{"opcode":2,"value":0}]},
                    {"group_index":group,"anim_id":1,"anim_type":0,"commands":commands}]}]}),
                sections: Default::default(),
                models: Default::default(),
                unsupported: Default::default(),
            };
            let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
            world.prepare_spatial().unwrap();
            let id = world.id(0).unwrap();
            world
                .bind_animation(id, &data, cyrus_script::AnimationGroup::PANIC)
                .unwrap();
            let hud = cyrus_game::script::ScriptHud::default();
            hud.configure(
                &inventory_asset(&[]),
                &cyrus_game::preferences::Preferences::default(),
            );
            let result = world.advance(
                id,
                ScriptStage::Body(Thread::Main),
                ScriptInputs {
                    hud: Some(&hud),
                    shell_execution: Some(shell),
                    nominal_ticks: Some(1),
                    frame_scale: Some(256),
                    ..Default::default()
                },
                Some(&mut |_, _| {
                    if failing {
                        anyhow::bail!("extra-time sound refused");
                    }
                    Ok(())
                }),
                None,
                30,
            );
            let control = world.actors()[0].control_state().unwrap();
            assert_eq!(
                control.motion_companion, 23,
                "torch entry does not clear rope_wait"
            );
            if failing {
                assert!(format!("{:#}", result.unwrap_err()).contains("extra-time sound refused"));
                assert_eq!(control.extra_time, 19);
                assert_eq!(control.living_state.byte(), 0);
                assert_eq!(world.actors()[0].runtime().unwrap().variables(), [4]);
            } else {
                result.unwrap();
                let expected = if shell { 19 } else { 0 };
                assert_eq!(control.extra_time, expected);
                assert_eq!(control.living_state.byte(), if shell { 0 } else { living });
                assert_eq!(
                    world.actors()[0].runtime().unwrap().variables(),
                    [i32::from(expected)]
                );
            }
        }
    }
}

#[test]
fn extra_time_is_retained_when_the_ordinary_slave_gate_refuses_the_actor() {
    use cyrus_game::script::OrdinaryInputs;
    let mut scripts = counter_scripts(Object::Me, 7, true);
    scripts.updates.disable_slaves = true;
    scripts.actors.get_mut("LOGIC").unwrap().attributes[cyrus_script::Attribute::Slave] = 1;
    for instance in &mut scripts.instances {
        instance.object_type = PlacementType::MODEL;
        instance.active = 1;
    }
    let mut master = scripts.actors["LOGIC"].clone();
    master.header_index = 1;
    master.instances = 1;
    master.namespace = Some("master".into());
    master.instance_variables.truncate(1);
    master.attributes[cyrus_script::Attribute::Slave] = 0;
    master.attributes[cyrus_script::Attribute::Master] = 1;
    scripts.actors.insert("MASTER".into(), master);
    scripts.instances.push(Instance {
        object_id: 2,
        actor: "MASTER".into(),
        object_type: Default::default(),
        ..scripts.instances[0].clone()
    });
    scripts.content_id = scripts.content_identity().unwrap();
    let data = cyrus_assets::animation::Animations {
        version: 2,
        world: scripts.world,
        metadata: serde_json::json!({"script_content_id":scripts.content_id,"actors":[{"script_name":"LOGIC","model_load_path":"none"}],"markers":[],"walk_node_maps":[]}),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    };
    let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
    world.prepare_spatial().unwrap();
    advance(&mut world, 0).unwrap();
    world
        .update_ordinary(
            &data,
            [0, 0],
            OrdinaryInputs {
                scene: None,
                scripts: ScriptInputs {
                    nominal_ticks: Some(3),
                    frame_scale: Some(256),
                    ..Default::default()
                },
            },
            &mut cyrus_game::random::EngineRandom::default(),
            &mut |_, _| Ok(()),
            20,
        )
        .unwrap();
    assert!(
        world.actors()[0]
            .control_state()
            .unwrap()
            .object_update_excluded
    );
    assert_eq!(world.actors()[0].control_state().unwrap().extra_time, 7);
    assert_eq!(world.actors()[0].runtime().unwrap().variables(), [7]);
    assert!(world.actors()[0].failure().is_none());
}
