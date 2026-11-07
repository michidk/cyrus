//! Public weapon operations own ExtraTime independently of rope_wait.
use cyrus_assets::script::PlacementType;
use cyrus_assets::script::*;
use cyrus_script::script::{ScriptInputs, ScriptStage, ScriptWorld};
use cyrus_soup::program::*;
use cyrus_soup::{AnimationGroup, Attribute, Thread};
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

fn assignment(destination: Place, first: Value) -> Operation {
    Operation::Assign {
        destination,
        formula: Formula {
            first,
            rest: vec![],
        },
    }
}

fn fixture(living: i32, group: u16, sound: bool) -> ScriptWorld {
    let mut scripts = cache();
    scripts.definition.references = vec![
        cyrus_soup::Property::ExtraTime,
        cyrus_soup::Property::RopeWait,
        cyrus_soup::Property::ExtraFlag,
    ];
    let property = |reference| Place::Property {
        object: Object::Me,
        reference,
    };
    scripts.definition.functions.push(Function {
        name: cyrus_soup::Native::PlayAnimation,
        param_count: 2,
        kind: CallKind::Function,
    });
    let actor = scripts.actors.get_mut("LOGIC").unwrap();
    let operations = [
        assignment(property(0), Value::Integer { value: 19 }),
        assignment(property(1), Value::Integer { value: 23 }),
        assignment(property(2), Value::Integer { value: living }),
        Operation::ObjectCall {
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
        assignment(
            Place::Local { index: 0 },
            Value::Read { place: property(0) },
        ),
        Operation::End { target: 4 },
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
    let mut commands = vec![];
    if sound {
        commands.push(serde_json::json!({"opcode":4,"sound_param":-17,"volume_shift":0}));
    }
    commands.push(serde_json::json!({"opcode":0,"frame_reference":10}));
    commands.push(serde_json::json!({"opcode":2,"value":0}));
    let data = cyrus_assets::animation::Animations {
        version: 2,
        world: scripts.world,
        metadata: serde_json::json!({"script_content_id":scripts.content_id,"actors":[{
            "script_name":"LOGIC","animation_groups":[
                {"group_index":0,"anim_id":1,"anim_type":0,"commands":commands},
                {"group_index":group,"anim_id":1,"anim_type":0,"commands":[{"opcode":0,"frame_reference":91},{"opcode":2,"value":0}]}]}]}),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    };
    let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
    let id = world.id(0).unwrap();
    world
        .bind_animation(id, &data, AnimationGroup::PANIC)
        .unwrap();
    world
        .advance(
            id,
            ScriptStage::Animation,
            inputs(),
            Some(&mut |_, _| Ok(())),
            None,
            30,
        )
        .unwrap();
    advance(&mut world).unwrap();
    world
}

fn inputs() -> ScriptInputs<'static> {
    ScriptInputs {
        nominal_ticks: Some(0),
        frame_scale: Some(256),
        ..Default::default()
    }
}
fn advance(world: &mut ScriptWorld) -> anyhow::Result<()> {
    world.advance(
        world.id(0)?,
        ScriptStage::Body(Thread::Main),
        inputs(),
        Some(&mut |_, _| Ok(())),
        None,
        30,
    )?;
    Ok(())
}

#[test]
fn cleanup_clears_extra_time_only_in_sword_states_and_public_reads_continue() {
    for state in [0, 20, 21, 33] {
        for selected in [false, true] {
            let mut world = fixture(state, 91, false);
            let id = world.id(0).unwrap();
            let player = world.id(if selected { 0 } else { 1 }).unwrap();
            let mut expected =
                serde_json::to_value(world.actors()[0].control_state().unwrap()).unwrap();
            world
                .cleanup_actor_weapon(id, player, 0, &mut |_, _| Ok(()))
                .unwrap();
            let counter = if state == 20 || state == 21 { 0 } else { 19 };
            if state == 20 || state == 21 {
                expected["living_state"] = 0.into();
                expected["extra_time"] = 0.into();
            }
            assert_eq!(
                serde_json::to_value(world.actors()[0].control_state().unwrap()).unwrap(),
                expected
            );
            assert_eq!(
                world.actors()[0].control_state().unwrap().motion_companion,
                23
            );
            advance(&mut world).unwrap();
            assert_eq!(world.actors()[0].runtime().unwrap().variables(), [counter]);
            world
                .cleanup_actor_weapon(id, player, 0, &mut |_, _| Ok(()))
                .unwrap();
            assert_eq!(
                world.actors()[0].control_state().unwrap().extra_time,
                counter as u8
            );
            assert_eq!(world.actors()[1].control_state().unwrap().extra_time, 0);
        }
    }
}

#[test]
fn cleanup_request_failure_retains_both_independent_bytes_before_the_clear() {
    for state in [20, 21] {
        let mut world = fixture(state, 91, true);
        let id = world.id(0).unwrap();
        let before = serde_json::to_value(world.actors()[0].control_state().unwrap()).unwrap();
        let error = world
            .cleanup_actor_weapon(id, id, 0, &mut |_, _| {
                anyhow::bail!("weapon cleanup cue refused")
            })
            .unwrap_err();
        assert!(format!("{error:#}").contains("weapon cleanup cue refused"));
        assert_eq!(
            serde_json::to_value(world.actors()[0].control_state().unwrap()).unwrap(),
            before
        );
        assert_eq!(world.actors()[0].control_state().unwrap().extra_time, 19);
        assert_eq!(
            world.actors()[0].control_state().unwrap().motion_companion,
            23
        );
        assert!(
            world
                .cleanup_actor_weapon(id, id, 0, &mut |_, _| Ok(()))
                .is_err()
        );
        assert_eq!(world.actors()[0].runtime().unwrap().variables(), [19]);
    }
}

#[test]
fn cleanup_rejects_a_stale_owner_before_either_byte_changes() {
    let mut world = fixture(20, 91, false);
    let id = world.id(0).unwrap();
    let mut stale = id;
    stale.generation = 6;
    assert!(
        world
            .cleanup_actor_weapon(id, stale, 0, &mut |_, _| Ok(()))
            .is_err()
    );
    let control = world.actors()[0].control_state().unwrap();
    assert_eq!(
        (
            control.extra_time,
            control.motion_companion,
            control.living_state.byte()
        ),
        (19, 23, 20)
    );
    advance(&mut world).unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().variables(), [19]);
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
fn native_sheathing_clears_extra_time_after_admission_and_keeps_rope_wait() {
    for (combat, holding, missing_hud) in [
        (true, true, false),
        (false, true, false),
        (true, false, false),
        (true, true, true),
    ] {
        let mut scripts = cache();
        scripts.definition.references = vec![
            cyrus_soup::Property::ExtraTime,
            cyrus_soup::Property::RopeWait,
        ];
        scripts.definition.functions.push(Function {
            name: cyrus_soup::Native::SheathSword,
            param_count: 0,
            kind: CallKind::Function,
        });
        for instance in &mut scripts.instances {
            instance.object_type = PlacementType::MODEL;
        }
        let actor = scripts.actors.get_mut("LOGIC").unwrap();
        actor.attributes[Attribute::Combat] = u8::from(combat);
        actor.attributes[Attribute::HoldingWeapon] = u8::from(holding);
        let property = |reference| Place::Property {
            object: Object::Me,
            reference,
        };
        let operations = [
            assignment(property(0), Value::Integer { value: 19 }),
            assignment(property(1), Value::Integer { value: 23 }),
            Operation::ObjectCall {
                call: Call {
                    target: Object::Me,
                    function_id: 1,
                    function: cyrus_soup::Native::SheathSword,
                    kind: CallKind::Function,
                    arguments: vec![],
                },
            },
            assignment(
                Place::Local { index: 0 },
                Value::Read { place: property(0) },
            ),
            Operation::End { target: 3 },
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
                "script_name":"LOGIC","model_load_path":"animation_table","animation_groups":[{
                    "group_index":0,"anim_id":1,"anim_type":0,"commands":[{"opcode":0,"frame_reference":10},{"opcode":2,"value":0}]}]}]}),
            sections: Default::default(),
            models: Default::default(),
            unsupported: Default::default(),
        };
        let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
        let id = world.id(0).unwrap();
        world
            .initialize_map_animation(id, &data, 0, &mut |_, _| Ok(()))
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
                hud: if missing_hud { None } else { Some(&hud) },
                shell_execution: Some(false),
                ..inputs()
            },
            None,
            None,
            30,
        );
        let control = world.actors()[0].control_state().unwrap();
        assert_eq!(control.motion_companion, 23);
        if missing_hud {
            assert!(format!("{:#}", result.unwrap_err()).contains("inventory owner unavailable"));
            assert_eq!((control.extra_time, control.living_state.byte()), (19, 0));
            assert_eq!(world.actors()[0].runtime().unwrap().variables(), [4]);
        } else {
            result.unwrap();
            let admitted = combat && holding;
            assert_eq!(
                (control.extra_time, control.living_state.byte()),
                if admitted { (0, 21) } else { (19, 0) }
            );
            assert_eq!(
                world.actors()[0].runtime().unwrap().variables(),
                [if admitted { 0 } else { 19 }]
            );
            if admitted {
                assert_eq!(control.combat.as_ref().unwrap().cooldown, 24);
            }
            advance(&mut world).unwrap();
            assert_eq!(
                world.actors()[0].runtime().unwrap().variables(),
                [if admitted { 0 } else { 19 }]
            );
        }
    }
}
