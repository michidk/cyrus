//! HeaderHook's three console lines, and the AI record properties it reports.
use super::*;
use cyrus_script::script::{ScriptInputs, ScriptStage};

/// Actor 0 runs `Me.<property> = value` for each write, then HeaderHook, then stores the
/// HeaderHook result in a local and reads each property back into locals 1.. .
fn header_world(writes: &[(Property, i32)], reads: &[Property]) -> ScriptWorld {
    let mut scripts = cache();
    let hook = u16::try_from(scripts.definition.functions.len()).unwrap();
    scripts.definition.functions.push(Function {
        name: Native::HeaderHook,
        param_count: 0,
        kind: CallKind::Function,
    });
    let mut properties: Vec<Property> = writes.iter().map(|(p, _)| *p).collect();
    properties.extend(reads);
    properties.dedup();
    scripts.definition.references = properties.clone();
    let reference =
        |property: Property| properties.iter().position(|p| *p == property).unwrap() as u16;
    let mut operations = vec![];
    for (property, value) in writes {
        operations.push(Operation::Assign {
            destination: Place::Property {
                object: Object::Me,
                reference: reference(*property),
            },
            formula: Formula {
                first: Value::Integer { value: *value },
                rest: vec![],
            },
        });
    }
    operations.push(Operation::Assign {
        destination: Place::Local { index: 0 },
        formula: Formula {
            first: Value::Call {
                call: Box::new(Call {
                    function_id: hook,
                    function: Native::HeaderHook,
                    target: Object::Me,
                    kind: CallKind::Function,
                    arguments: vec![],
                }),
            },
            rest: vec![],
        },
    });
    for (slot, property) in reads.iter().enumerate() {
        operations.push(Operation::Assign {
            destination: Place::Local {
                index: slot as u8 + 1,
            },
            formula: Formula {
                first: Value::Read {
                    place: Place::Property {
                        object: Object::Me,
                        reference: reference(*property),
                    },
                },
                rest: vec![],
            },
        });
    }
    operations.push(Operation::End { target: 0 });
    let logic = scripts.actors.get_mut("LOGIC").unwrap();
    logic.program.variables = vec![0; reads.len() + 1];
    logic.instance_variables = vec![vec![0; reads.len() + 1]; 2];
    logic.program.steps = operations
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
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let id = world.id(0).unwrap();
    world
        .advance(
            id,
            ScriptStage::OrdinaryMain,
            ScriptInputs {
                nominal_ticks: Some(1),
                frame_scale: Some(256),
                ..Default::default()
            },
            None,
            None,
            100,
        )
        .unwrap();
    world
}

#[test]
fn header_hook_prints_three_lines_with_the_allocation_default_ai_record_and_returns_one() {
    let world = header_world(&[], &[]);
    let name = world.actors()[0].name().to_owned();
    let lines = world.globals().printed_lines();
    assert_eq!(lines.len(), 3, "{lines:?}");
    assert_eq!(lines[0], "\nHeader Hook:");
    assert_eq!(
        lines[1],
        format!("Object \"logic\"(id 36) at {:#010x} using \"{name}\"", 0),
        "{lines:?}"
    );
    // Allocation leaves the node fields and the first variable at minus one and the rest zero.
    assert_eq!(
        lines[2],
        "Node -1, dnode -1, ai_type 0, ai_mode 0, ai_delay 0, ai_length 0, ai_var1 -1, ai_var2 0"
    );
    assert_eq!(world.actors()[0].runtime().unwrap().variables()[0], 1);
}

#[test]
fn ai_record_properties_round_trip_with_their_widths_and_header_hook_reports_them() {
    let writes = [
        (Property::Node, 123_456),
        (Property::Dnode, -7),
        (Property::Fdnode, 5),
        (Property::AiLength, i32::MIN),
        (Property::AiVar1, 0x1_2345),
        (Property::AiVar2, 0x8000),
        (Property::AiMode, 9),
        (Property::AiDelay, 200),
    ];
    let reads = [
        Property::Node,
        Property::Dnode,
        Property::Fdnode,
        Property::AiLength,
        Property::AiVar1,
        Property::AiVar2,
    ];
    let world = header_world(&writes, &reads);
    let locals = world.actors()[0].runtime().unwrap().variables().to_vec();
    // The words keep the low sixteen bits and read sign-extended; the dwords keep everything.
    assert_eq!(
        locals,
        [1, 123_456, -7, 5, i32::MIN, 0x2345, -0x8000].map(cyrus_soup::ScriptValue::Integer)
    );
    // The node fields are the navigation owner's held nodes.
    let navigation = world.actors()[0].control_state().unwrap().navigation;
    assert_eq!(
        (navigation.current, navigation.next, navigation.destination),
        (123_456, -7, 5)
    );
    assert_eq!(
        world.globals().printed_lines()[2],
        format!(
            "Node 123456, dnode -7, ai_type 0, ai_mode 9, ai_delay 200, ai_length {}, ai_var1 {}, ai_var2 {}",
            i32::MIN,
            0x2345,
            -0x8000
        )
    );
}

#[test]
fn header_hook_with_an_argument_is_rejected_when_the_world_is_prepared() {
    let error = run_native_world_with(&[(Native::HeaderHook, &[1])], true)
        .err()
        .expect("an argument is refused");
    assert!(format!("{error:#}").contains("HeaderHook"), "{error:#}");
}

#[test]
fn the_ai_record_is_saved_with_the_actor_and_older_saves_restore_the_allocation_defaults() {
    use cyrus_script::script::{WorldRestoreContext, WorldSnapshotSources};
    let mut scripts = cache();
    for instance in &mut scripts.instances {
        instance.object_type = cyrus_assets::script::PlacementType::MODEL;
    }
    scripts.content_id = scripts.content_identity().unwrap();
    let world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let data = cyrus_assets::animation::Animations {
        version: 2,
        world: scripts.world,
        metadata: serde_json::json!({"script_content_id": scripts.content_id,
            "actors": scripts.actors.keys().map(|name| serde_json::json!({"script_name":name,"model_load_path":"none","mpsz_bounds_primary":-1})).collect::<Vec<_>>() }),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    };
    let collision = Default::default();
    let sources = WorldSnapshotSources {
        identity_memo: None,
        effects: None,
        player: None,
        scripts: &scripts,
        animations: &data,
        collision: &collision,
    };
    let restore = |edit: &dyn Fn(&mut serde_json::Value)| {
        let mut saved = serde_json::to_value(world.snapshot(&sources).unwrap()).unwrap();
        edit(&mut saved);
        ScriptWorld::from_snapshot(
            serde_json::from_value(saved).unwrap(),
            &sources,
            WorldRestoreContext {
                generation: 2,
                globals: Default::default(),
                samples: &Default::default(),
            },
        )
        .unwrap()
    };
    let saved = restore(&|saved| {
        saved["actors"][0]["control"]["ai_location"] = 9.into();
        saved["actors"][0]["control"]["ai_record"] = serde_json::json!({"var2": -3, "length": 22});
    });
    let ai = saved.actors()[0].control_state().unwrap().ai;
    assert_eq!([ai.var1 as i32, ai.var2 as i32, ai.length], [9, -3, 22]);
    // Saves written before the record existed restore the allocation defaults, keeping the
    // location word they did save.
    let older = restore(&|saved| {
        saved["actors"][0]["control"]["ai_location"] = 4.into();
        saved["actors"][0]["control"]
            .as_object_mut()
            .unwrap()
            .remove("ai_record");
    });
    let ai = older.actors()[0].control_state().unwrap().ai;
    assert_eq!([ai.var1 as i32, ai.var2 as i32, ai.length], [4, 0, 0]);
    // Saves written when the rope release speed was kept apart restore it as the last `move` word.
    let earlier = restore(&|saved| {
        saved["actors"][0]["control"]["rope_speed"] = (-96).into();
        saved["actors"][0]["control"]["motion_vector"] = serde_json::json!([1, 2, 0]);
    });
    assert_eq!(
        earlier.actors()[0].control_state().unwrap().motion_vector,
        [1, 2, -96]
    );
}

#[test]
fn message_and_broadcast_index_properties_are_the_receivers_selection_bytes() {
    // Unsigned bytes: a write keeps the low eight bits and a read zero-extends.
    let mut world = header_world(
        &[(Property::BroadcastIndex, 1), (Property::MessageIndex, -3)],
        &[Property::BroadcastIndex, Property::MessageIndex],
    );
    let locals = world.actors()[0].runtime().unwrap().variables().to_vec();
    assert_eq!(locals, [1, 1, 253].map(cyrus_soup::ScriptValue::Integer));
    // The property is the selection the broadcast natives use: with the selection at slot one, a
    // single broadcast (write cursor one) leaves nothing selected; at slot zero it is selected.
    let (sender, receiver) = (world.id(1).unwrap(), world.id(0).unwrap());
    world
        .send_broadcast(sender, cyrus_script::script::CombatMessageCode::HIT)
        .unwrap();
    assert_eq!(world.selected_broadcast_sender(receiver).unwrap(), None);
    let mut at_zero = header_world(&[(Property::BroadcastIndex, 0)], &[]);
    let (sender, receiver) = (at_zero.id(1).unwrap(), at_zero.id(0).unwrap());
    at_zero
        .send_broadcast(sender, cyrus_script::script::CombatMessageCode::HIT)
        .unwrap();
    assert_eq!(
        at_zero.selected_broadcast_sender(receiver).unwrap(),
        Some(sender)
    );
    // Any byte survives a raw write and a save; the natives report an out-of-queue cursor on use.
    let wide = header_world(&[(Property::MessageIndex, 200)], &[Property::MessageIndex]);
    assert_eq!(
        wide.actors()[0].runtime().unwrap().variables()[1],
        cyrus_soup::ScriptValue::Integer(200)
    );
}

#[test]
fn the_node_properties_are_the_walk_map_owners_held_nodes() {
    // The properties read and write the actor's held current, next and destination nodes, the
    // fields navigation keeps, so the walk code sees a script's write and HeaderHook reports it.
    let world = header_world(
        &[
            (Property::Node, 11),
            (Property::Dnode, 5),
            (Property::Fdnode, 6),
        ],
        &[Property::Node, Property::Dnode, Property::Fdnode],
    );
    let navigation = world.actors()[0].control_state().unwrap().navigation;
    assert_eq!(
        (navigation.current, navigation.next, navigation.destination),
        (11, 5, 6)
    );
    assert_eq!(
        world.actors()[0].runtime().unwrap().variables()[1..],
        [11, 5, 6].map(cyrus_soup::ScriptValue::Integer)
    );
    assert!(
        world.globals().printed_lines()[2].starts_with("Node 11, dnode 5, "),
        "{:?}",
        world.globals().printed_lines()
    );
}
