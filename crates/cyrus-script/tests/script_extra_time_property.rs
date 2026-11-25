//! Receiver-owned extra-time countdown uses public property dispatch.
use cyrus_assets::script::PlacementType;
use cyrus_assets::script::*;
use cyrus_script::script::{ScriptInputs, ScriptStage, ScriptWorld};
use cyrus_soup::Thread;
use cyrus_soup::program::*;

mod support;
use support::advance;
use support::counter_scripts;

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
            let scripts = counter_scripts(cyrus_soup::Property::ExtraTime, object, value, true);
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
    let scripts = counter_scripts(cyrus_soup::Property::ExtraTime, Object::Me, 99, false);
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
            let mut scripts =
                counter_scripts(cyrus_soup::Property::ExtraTime, object, -12345, write);
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
    use cyrus_script::script::OrdinaryInputs;
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
        let mut scripts = counter_scripts(cyrus_soup::Property::ExtraTime, Object::Me, value, true);
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
        text_style: [0; 2].into(),
        player_text_style: [0; 2].into(),
        used_text_style: [0; 2].into(),
        torch_time: None,
        inventory_screen: Default::default(),
        island_map: Default::default(),
    }
}

#[test]
fn extra_time_torch_entry_resets_after_admission_and_retains_on_refusal_or_sound_failure() {
    for (function, group, living) in [("LightTorch", 149, 33), ("UnlightTorch", 150, 34)] {
        for (shell, failing) in [(false, false), (true, false), (false, true)] {
            let mut scripts =
                counter_scripts(cyrus_soup::Property::ExtraTime, Object::Me, 19, true);
            scripts
                .definition
                .references
                .push(cyrus_soup::Property::RopeWait);
            scripts.definition.functions.push(Function {
                name: cyrus_soup::Native::from_name(function).expect("known fixture native"),
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
                            function: cyrus_soup::Native::from_name(function)
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
                .bind_animation(id, &data, cyrus_soup::AnimationGroup::PANIC)
                .unwrap();
            let hud = cyrus_script::script::ScriptHud::default();
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
                control.jump_selector, 23,
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
    use cyrus_script::script::OrdinaryInputs;
    let mut scripts = counter_scripts(cyrus_soup::Property::ExtraTime, Object::Me, 7, true);
    scripts.updates.disable_slaves = true;
    scripts.actors.get_mut("LOGIC").unwrap().attributes[cyrus_soup::Attribute::Slave] = 1;
    for instance in &mut scripts.instances {
        instance.object_type = PlacementType::MODEL;
        instance.active = 1;
    }
    let mut master = scripts.actors["LOGIC"].clone();
    master.header_index = 1;
    master.instances = 1;
    master.namespace = Some("master".into());
    master.instance_variables.truncate(1);
    master.attributes[cyrus_soup::Attribute::Slave] = 0;
    master.attributes[cyrus_soup::Attribute::Master] = 1;
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
