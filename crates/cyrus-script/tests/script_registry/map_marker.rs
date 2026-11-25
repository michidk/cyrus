use super::*;

fn marker_world(position: [i32; 3], marker: i32, distance: i32) -> ScriptWorld {
    use cyrus_assets::animation::Animations;
    use cyrus_script::script::{ScriptInputs, ScriptStage};
    let mut scripts = cache();
    scripts.definition.functions.extend([
        Function {
            name: Native::MoveByAxis,
            param_count: 3,
            kind: CallKind::Task,
        },
        Function {
            name: Native::AtNodeMarker,
            param_count: 2,
            kind: CallKind::Function,
        },
    ]);
    let mut operations: Vec<_> = position
        .into_iter()
        .enumerate()
        .map(|(axis, value)| {
            wait(
                1,
                Native::MoveByAxis,
                CallKind::Multitask,
                vec![
                    Value::Integer { value: axis as i32 },
                    Value::Integer { value },
                    Value::Integer { value: 1 },
                ],
            )
        })
        .collect();
    operations.push(Operation::Assign {
        destination: Place::Flag { index: 0 },
        formula: Formula {
            first: Value::Call {
                call: Box::new(Call {
                    function_id: 2,
                    function: Native::AtNodeMarker,
                    kind: CallKind::Function,
                    target: Object::Me,
                    arguments: vec![
                        Value::Integer { value: marker },
                        Value::Integer { value: distance },
                    ],
                }),
            },
            rest: vec![],
        },
    });
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
    let data = Animations {
        version: 2,
        world: scripts.world,
        metadata: serde_json::json!({"script_content_id":scripts.content_id,"markers":[{"map_position":[0,0,0],"reserved":1},{"map_position":[0,0,0],"reserved":0}]}),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    };
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world.prepare_markers(&data).unwrap();
    world.prepare_spatial().unwrap();
    let id = world.id(0).unwrap();
    let result = world.advance(
        id,
        ScriptStage::Body(Thread::Main),
        ScriptInputs {
            frame_scale: Some(256),
            nominal_ticks: Some(0),
            ..Default::default()
        },
        None,
        None,
        20,
    );
    if matches!(marker, 0) {
        result.unwrap();
    } else {
        assert!(result.is_err());
    }
    world
}

#[test]
fn node_marker_query_uses_euclidean_distance_including_the_boundary_and_vertical_axis() {
    for (position, distance, expected) in [
        ([768, 1024, 0], 1279, 0),
        ([768, 1024, 0], 1280, 1),
        ([0, 0, 0], -1, 0),
        ([0, 0, 0], 0, 1),
        ([40265340, 53687120, 1], 67108900, 0),
        ([40265340, 53687120, 1], 67108904, 1),
    ] {
        assert_eq!(
            marker_world(position, 0, distance).flags(),
            [expected],
            "position {position:?} distance {distance}"
        );
    }
}

#[test]
fn unavailable_marker_query_is_contextual_and_sticky() {
    for (marker, message) in [(-1, "outside"), (1, "unavailable"), (128, "outside")] {
        let mut world = marker_world([0; 3], marker, 0);
        let error = world.actors()[0].failure().unwrap().to_owned();
        assert!(
            error.contains("world 7")
                && error.contains(&format!("marker {marker}"))
                && error.contains(message),
            "{error}"
        );
        assert_eq!(world.flags(), [12]);
        let id = world.id(0).unwrap();
        assert!(world.step(id, Thread::Main, 20).is_err());
        assert_eq!(world.actors()[0].failure(), Some(error.as_str()));
    }
}

#[test]
fn marker_preparation_rejects_malformed_or_mismatched_data_before_installing_state() {
    use cyrus_assets::animation::Animations;
    let scripts = cache();
    let data = |markers| Animations {
        version: 2,
        world: scripts.world,
        metadata: serde_json::json!({"script_content_id":scripts.content_id,"markers":markers}),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    };
    let valid = serde_json::json!([{"map_position":[0,0,0],"reserved":1}]);
    for malformed in [
        serde_json::Value::Null,
        serde_json::json!([{"map_position":[0,0],"reserved":1}]),
        serde_json::json!([{"map_position":[8388608,0,0],"reserved":1}]),
    ] {
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        let error = world.prepare_markers(&data(malformed)).unwrap_err();
        assert!(format!("{error:#}").contains("world 7 marker"));
        world.prepare_markers(&data(valid.clone())).unwrap();
        assert!(world.prepare_markers(&data(valid.clone())).is_err());
    }
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let mut mismatch = data(valid.clone());
    mismatch.metadata["script_content_id"] = "old".into();
    assert!(
        world
            .prepare_markers(&mismatch)
            .unwrap_err()
            .to_string()
            .contains("cache generation")
    );
    mismatch.metadata["script_content_id"] = scripts.content_id.clone().into();
    mismatch.world += 1;
    assert!(world.prepare_markers(&mismatch).is_err());
    world.prepare_markers(&data(valid)).unwrap();
}

#[test]
fn routed_node_marker_query_reads_the_receivers_local_attached_position() {
    use cyrus_assets::animation::Animations;
    use cyrus_script::script::{ScriptInputs, ScriptStage};
    let mut scripts = cache();
    scripts.definition.functions.push(Function {
        name: Native::AtNodeMarker,
        param_count: 2,
        kind: CallKind::Function,
    });
    let actor = scripts.actors.get_mut("LOGIC").unwrap();
    actor.program.strings = vec!["child".into()];
    actor.program.steps = BTreeMap::from([
        (
            0,
            Step {
                next: 1,
                operation: Operation::Assign {
                    destination: Place::Flag { index: 0 },
                    formula: Formula {
                        first: Value::Call {
                            call: Box::new(Call {
                                function_id: 1,
                                function: Native::AtNodeMarker,
                                kind: CallKind::Function,
                                target: Object::Named { string: 0 },
                                arguments: vec![
                                    Value::Integer { value: 0 },
                                    Value::Integer { value: 1280 },
                                ],
                            }),
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
    let mut child = actor.clone();
    child.namespace = Some("child".into());
    child.attributes[Attribute::Slave] = 1;
    actor.attributes[Attribute::Master] = 1;
    scripts.actors.insert("CHILD".into(), child);
    scripts.instances[0].map_position = [100, 0, 0];
    scripts.instances[1].map_position = [103, 4, 0];
    scripts.instances[1].actor = "CHILD".into();
    scripts.content_id = scripts.content_identity().unwrap();
    let data = Animations {
        version: 2,
        world: scripts.world,
        metadata: serde_json::json!({"script_content_id":scripts.content_id,"markers":[{"map_position":[0,0,0],"reserved":1}]}),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    };
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world.prepare_markers(&data).unwrap();
    world.prepare_spatial().unwrap();
    world.refresh_families().unwrap();
    let parent = world.id(0).unwrap();
    let child = world.id(1).unwrap();
    assert_eq!(world.spatial(child).unwrap().local.position, [768, 1024, 0]);
    assert_eq!(
        world.spatial(child).unwrap().effective.unwrap().position,
        [26368, 1024, 0]
    );
    world
        .advance(
            parent,
            ScriptStage::Body(Thread::Main),
            ScriptInputs {
                frame_scale: Some(256),
                nominal_ticks: Some(0),
                ..Default::default()
            },
            None,
            None,
            20,
        )
        .unwrap();
    assert_eq!(world.flags(), [1]);
    assert_eq!(
        world.actors()[1]
            .runtime()
            .unwrap()
            .program_counter(Thread::Main),
        Some(0)
    );
}
