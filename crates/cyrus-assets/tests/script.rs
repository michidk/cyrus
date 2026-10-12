//! Script cache contracts use authored data and optional read-only installed extraction.
use cyrus_assets::script::*;
use std::collections::BTreeMap;
fn cache() -> Scripts {
    let mut scripts = Scripts {
        version: 5,
        actor_pool_capacity: 32,
        world: 7,
        content_id: String::new(),
        definition: Definition {
            functions: vec![Function {
                name: "NullFunction".into(),
                param_count: 0,
                kind: CallKind::Function,
            }],
            flags: vec![Flag {
                name: "Ready".into(),
                flag_type: "bool".into(),
                initial_value: 1,
            }],
            references: vec![],
            attributes: vec![],
        },
        actors: BTreeMap::new(),
        instances: vec![],
        unsupported: BTreeMap::new(),
    };
    scripts.content_id = scripts.content_identity().unwrap();
    scripts
}
#[test]
fn content_identity_rejects_changed_programs_and_defaults() {
    let mut scripts = cache();
    scripts.validate().unwrap();
    scripts.definition.flags[0].initial_value = 0;
    assert!(
        scripts
            .validate()
            .unwrap_err()
            .to_string()
            .contains("identity mismatch")
    );
}
#[test]
fn portable_script_records_round_trip_with_the_same_identity() {
    let mut scripts = cache();
    scripts.instances = [0, 1, 2, 255]
        .into_iter()
        .enumerate()
        .map(|(index, model_mode)| Instance {
            object_id: index as u32,
            actor: "LOGIC".into(),
            object_type: 1,
            active: 0,
            model_name: format!("MODEL{index}"),
            model_mode,
            map_position: [0; 3],
            angles: [0; 3],
        })
        .collect();
    scripts.content_id = scripts.content_identity().unwrap();
    let decoded: Scripts = serde_json::from_slice(&serde_json::to_vec(&scripts).unwrap()).unwrap();
    decoded.validate().unwrap();
    assert_eq!(decoded.content_id, scripts.content_id);
    assert_eq!(decoded.definition.flags[0].initial_value, 1);
    assert_eq!(
        decoded
            .instances
            .iter()
            .map(|instance| (instance.model_name.as_str(), instance.model_mode))
            .collect::<Vec<_>>(),
        [("MODEL0", 0), ("MODEL1", 1), ("MODEL2", 2), ("MODEL3", 255)]
    );
    for change_name in [false, true] {
        let mut changed = decoded.clone();
        if change_name {
            changed.instances[0].model_name = "OTHER".into();
        } else {
            changed.instances[0].model_mode = 1;
        }
        assert!(
            changed
                .validate()
                .unwrap_err()
                .to_string()
                .contains("identity mismatch")
        );
    }
}

#[test]
fn installed_indices_are_checked_inside_nested_function_arguments() {
    let mut scripts = cache();
    scripts.definition.functions.push(Function {
        name: "Query".into(),
        param_count: 1,
        kind: CallKind::Function,
    });
    let nested = Call {
        function_id: 1,
        kind: CallKind::Function,
        target: Object::Me,
        function: "Query".into(),
        arguments: vec![Value::Read {
            place: Place::Flag { index: 1 },
        }],
    };
    let mut program = Program {
        version: 2,
        entry: 0,
        variables: vec![0],
        strings: vec![],
        steps: BTreeMap::from([
            (
                0,
                Step {
                    next: 1,
                    operation: Operation::Assign {
                        destination: Place::Local { index: 0 },
                        formula: Formula {
                            first: Value::Call {
                                call: Box::new(nested),
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
        ]),
    };
    let error = program
        .validate_definition(&scripts.definition)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("instruction 0: global flag 1 absent"),
        "{error}"
    );
    if let Operation::Assign { formula, .. } = &mut program.steps.get_mut(&0).unwrap().operation {
        let Value::Call { call } = &mut formula.first else {
            panic!("call expected")
        };
        call.arguments = vec![Value::Integer { value: 3 }];
        call.function_id = 0;
    }
    let error = program
        .validate_definition(&scripts.definition)
        .unwrap_err()
        .to_string();
    assert!(error.contains("declaration mismatch"), "{error}");
}
#[cfg(feature = "native")]
#[test]
fn installed_worlds_keep_script_records_and_explicit_unsupported_actors() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../redguard");
    if !root.join("RGFX.EXE").exists() {
        eprintln!("skipped: no install (SOUP scripts)");
        return;
    }
    let mut maps = 0;
    for file in std::fs::read_dir(root.join("maps")).unwrap() {
        let path = file.unwrap().path();
        if !path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("rgm"))
        {
            continue;
        }
        let map = rgpre::import::rgm::parse_rgm_file(&std::fs::read(&path).unwrap()).unwrap();
        let scripts = cyrus_assets::script_export::prepare(&map, &root, 0)
            .unwrap_or_else(|error| panic!("{}: {error:#}", path.display()));
        scripts.validate().unwrap();
        assert!(!scripts.actors.is_empty());
        let placements: Vec<_> = map
            .sections
            .iter()
            .filter_map(|section| match section {
                rgpre::import::rgm::RgmSection::MpobParsed(_, records) => Some(records),
                _ => None,
            })
            .flatten()
            .collect();
        assert_eq!(scripts.instances.len(), placements.len());
        for (instance, placement) in scripts.instances.iter().zip(placements) {
            assert_eq!(instance.object_id, placement.id);
            assert_eq!(
                instance.model_name,
                placement.model_name(),
                "{} object {} model name",
                path.display(),
                placement.id
            );
            assert_eq!(
                instance.model_mode,
                placement.is_static,
                "{} object {} model mode",
                path.display(),
                placement.id
            );
        }
        for (name, actor) in &scripts.actors {
            let attribute = |index: usize| {
                let key = scripts
                    .definition
                    .attributes
                    .get(index)
                    .filter(|name| !name.is_empty())
                    .cloned()
                    .unwrap_or_else(|| format!("attr_{index}"));
                actor.attributes[&key]
            };
            assert_eq!(attribute(115), attribute(6), "{name}: reserved weight");
            assert_eq!(attribute(119), attribute(1), "{name}: reserved collision");
            assert_ne!(attribute(34), 0, "{name}: sheath frame default");
            assert_ne!(attribute(35), 0, "{name}: draw frame default");
        }
        assert!(
            scripts
                .unsupported
                .keys()
                .all(|name| scripts.actors.contains_key(name))
        );
        maps += 1;
    }
    assert!(maps > 0);
}

#[test]
fn pool_capacity_participates_in_cache_identity_and_old_schemas_require_reexport() {
    let mut scripts = cache();
    scripts.actor_pool_capacity += 1;
    assert!(
        scripts
            .validate()
            .unwrap_err()
            .to_string()
            .contains("identity mismatch")
    );
    scripts.content_id = scripts.content_identity().unwrap();
    scripts.validate().unwrap();
    scripts.actor_pool_capacity = 9;
    scripts.content_id = scripts.content_identity().unwrap();
    assert!(
        scripts
            .validate()
            .unwrap_err()
            .to_string()
            .contains("invalid script actor pool capacity")
    );
    scripts.version = 4;
    assert!(
        scripts
            .validate()
            .unwrap_err()
            .to_string()
            .contains("re-export")
    );
}

#[test]
fn inventory_counts_nested_calls_references_and_unplaced_actors_without_support_claims() {
    let mut scripts = cache();
    scripts.actors.insert(
        "Unplaced".into(),
        Actor {
            header_index: 0,
            code_origin: 0,
            instances: 1,
            namespace: None,
            instance_variables: vec![vec![]],
            attributes: BTreeMap::new(),
            hooks: vec![4, 9],
            program: Program {
                version: 2,
                entry: 0,
                variables: vec![],
                strings: vec![],
                steps: BTreeMap::from([
                    (
                        0,
                        Step {
                            next: 1,
                            operation: Operation::Assign {
                                destination: Place::Property {
                                    object: Object::Player,
                                    reference: 3,
                                },
                                formula: Formula {
                                    first: Value::Call {
                                        call: Box::new(Call {
                                            function_id: 1,
                                            kind: CallKind::Function,
                                            target: Object::Local { index: 0 },
                                            function: "Outer".into(),
                                            arguments: vec![Value::Call {
                                                call: Box::new(Call {
                                                    function_id: 2,
                                                    kind: CallKind::Function,
                                                    target: Object::Camera,
                                                    function: "Inner".into(),
                                                    arguments: vec![Value::Update {
                                                        place: Place::Flag { index: 0 },
                                                        delta: 1,
                                                    }],
                                                }),
                                            }],
                                        }),
                                    },
                                    rest: vec![(1, Value::Integer { value: 2 })],
                                },
                            },
                        },
                    ),
                    (
                        1,
                        Step {
                            next: 1,
                            operation: Operation::Unsupported {
                                reason: "unknown form".into(),
                            },
                        },
                    ),
                ]),
            },
        },
    );
    let report = inventory::inspect(&scripts);
    assert_eq!(report.declared_functions, ["NullFunction"]);
    let actor = &report.actors["Unplaced"];
    assert_eq!(actor.placements, 0);
    assert_eq!(actor.instances, 1);
    assert_eq!(actor.hooks, [4, 9]);
    assert_eq!(
        actor.calls,
        BTreeMap::from([
            ("1:Function:Outer".into(), 1),
            ("2:Function:Inner".into(), 1)
        ])
    );
    assert_eq!(
        actor.references,
        BTreeMap::from([
            ("object:player".into(), 1),
            ("object:local".into(), 1),
            ("object:camera".into(), 1),
            ("property:3".into(), 1),
        ])
    );
    assert_eq!(actor.operands["value:call"], 2);
    assert_eq!(actor.operands["value:update"], 1);
    assert_eq!(actor.instructions["assign"], 1);
    assert_eq!(actor.unsupported, ["unknown form"]);
}
