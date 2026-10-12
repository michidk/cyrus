//! Script cache contracts use authored data and optional read-only installed extraction.
use cyrus_assets::script::*;
use std::collections::BTreeMap;
fn cache() -> Scripts {
    let mut scripts = Scripts {
        version: 3,
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
    let scripts = cache();
    let decoded: Scripts = serde_json::from_slice(&serde_json::to_vec(&scripts).unwrap()).unwrap();
    decoded.validate().unwrap();
    assert_eq!(decoded.content_id, scripts.content_id);
    assert_eq!(decoded.definition.flags[0].initial_value, 1);
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
    scripts.version = 2;
    assert!(
        scripts
            .validate()
            .unwrap_err()
            .to_string()
            .contains("re-export")
    );
}
