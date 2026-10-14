//! Script cache contracts use authored data and optional read-only installed extraction.
use cyrus_assets::script::*;
use cyrus_script::program::*;
use std::collections::BTreeMap;
fn cache() -> Scripts {
    let mut scripts = Scripts {
        version: 10,
        player_distance_override: false,
        updates: cyrus_assets::script::UpdateSettings {
            task_system: true,
            animation_system: true,
            disable_slaves: false,
            disable_master_slaves: false,
        },
        clock: cyrus_assets::script::ClockSettings {
            normal_frame_rate: 12,
            min_frame_rate: 6,
            max_frame_rate: 300,
            use_smooth_fps: true,
            use_smooth_divisor: true,
        },
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
    let definition = rgpre::import::soup_def::load_soup_def(&root).unwrap();
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
            let hook = rgpre::import::rgm::script_program::decode_native_hook(
                &map,
                actor.header_index,
                &definition,
            )
            .unwrap();
            assert_eq!(
                serde_json::to_value(&actor.native_hook).unwrap(),
                serde_json::to_value(hook).unwrap(),
                "{} actor {name} native hook",
                path.display()
            );
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
            location_offsets: vec![],
            attributes: BTreeMap::new(),
            native_hook: Some(Call {
                function_id: 3,
                function: "Hook".into(),
                kind: CallKind::Function,
                target: Object::Me,
                arguments: vec![],
            }),
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
    assert_eq!(actor.native_hook.as_ref().unwrap().function, "Hook");
    assert_eq!(
        actor.calls,
        BTreeMap::from([
            ("1:Function:Outer".into(), 1),
            ("2:Function:Inner".into(), 1),
            ("3:Function:Hook".into(), 1)
        ])
    );
    assert_eq!(
        actor.references,
        BTreeMap::from([
            ("object:player".into(), 1),
            ("object:local".into(), 1),
            ("object:camera".into(), 1),
            ("object:me".into(), 1),
            ("property:3".into(), 1),
        ])
    );
    assert_eq!(actor.operands["value:call"], 2);
    assert_eq!(actor.operands["value:update"], 1);
    assert_eq!(actor.instructions["assign"], 1);
    assert_eq!(actor.unsupported, ["unknown form"]);
}

#[test]
fn clock_inputs_are_validated_and_part_of_script_identity() {
    let mut scripts = cache();
    scripts.clock.normal_frame_rate += 1;
    assert!(
        scripts
            .validate()
            .unwrap_err()
            .to_string()
            .contains("identity mismatch")
    );
    for (normal, min, max) in [
        (0, 6, 300),
        (12, 0, 300),
        (12, 301, 300),
        (u32::MAX, 6, 300),
    ] {
        let mut invalid = cache();
        invalid.clock.normal_frame_rate = normal;
        invalid.clock.min_frame_rate = min;
        invalid.clock.max_frame_rate = max;
        invalid.content_id = invalid.content_identity().unwrap();
        assert!(format!("{:#}", invalid.validate().unwrap_err()).contains("SYSTEM.INI"));
    }
    let mut old = cache();
    old.version = 5;
    assert!(
        old.validate()
            .unwrap_err()
            .to_string()
            .contains("re-export")
    );
}

#[cfg(feature = "parsers")]
#[test]
fn byte_source_preparation_resolves_clock_inputs_and_reports_invalid_fields() {
    use cyrus_assets::source::{FileInfo, Manifest, ReadAssets};
    struct Source {
        manifest: Manifest,
        system: Vec<u8>,
        cheats: Option<Vec<u8>>,
    }
    impl ReadAssets for Source {
        fn manifest(&self) -> &Manifest {
            &self.manifest
        }
        fn read_range(&self, path: &str, start: u64, length: usize) -> anyhow::Result<Vec<u8>> {
            let bytes = match path {
                "SYSTEM.INI" => self.system.as_slice(),
                "soup386/SOUP386.DEF" => &[],
                "REDGUARD.CHT" => self.cheats.as_deref().unwrap(),
                _ => anyhow::bail!("unexpected source {path}"),
            };
            let start = usize::try_from(start)?;
            Ok(bytes
                .get(start..start + length)
                .ok_or_else(|| anyhow::anyhow!("{path}: truncated"))?
                .to_vec())
        }
    }
    let prepare = |normal: i32, min: i32, cheats: Option<Vec<u8>>| {
        let system = format!("[system]\nmax_active_objects=10\nnormal_frame_rate={normal}\nmin_frame_rate={min}\nmax_frame_rate=300\nuse_smooth_fps=yes\nuse_smooth_divisor=no\ntask_system=yes\nanimation_system=no\n[debug]\ndisable_slaves=yes\ndisable_master_slaves=no\n").into_bytes();
        let mut source = Source {
            manifest: Manifest {
                files: vec![
                    FileInfo {
                        path: "SYSTEM.INI".into(),
                        bytes: system.len() as u64,
                        digest: "0".repeat(64),
                    },
                    FileInfo {
                        path: "soup386/SOUP386.DEF".into(),
                        bytes: 0,
                        digest: "0".repeat(64),
                    },
                ],
            },
            system,
            cheats,
        };
        if let Some(cheats) = &source.cheats {
            source.manifest.files.push(FileInfo {
                path: "REDGUARD.CHT".into(),
                bytes: cheats.len() as u64,
                digest: "0".repeat(64),
            });
        }
        cyrus_assets::script_export::prepare_source(
            &rgpre::import::rgm::RgmFile { sections: vec![] },
            &source,
            17,
        )
    };
    let scripts = prepare(0, 6, None).unwrap();
    assert_eq!(
        scripts.clock,
        ClockSettings {
            normal_frame_rate: 12,
            min_frame_rate: 6,
            max_frame_rate: 300,
            use_smooth_fps: true,
            use_smooth_divisor: false,
        }
    );
    assert_eq!(scripts.version, 10);
    assert!(!scripts.player_distance_override);
    let mut cheats = vec![0; 256];
    cheats[40..44].copy_from_slice(&u32::MAX.to_le_bytes());
    cheats[24..28].copy_from_slice(&1u32.to_le_bytes());
    let overridden = prepare(0, 6, Some(cheats.clone())).unwrap();
    assert!(overridden.player_distance_override);
    assert_ne!(overridden.content_id, scripts.content_id);
    cheats[28..32].copy_from_slice(&1u32.to_le_bytes());
    cheats[12..16].copy_from_slice(&1u32.to_le_bytes());
    cheats[16..20].copy_from_slice(&1u32.to_le_bytes());
    let with_other_cheats = prepare(0, 6, Some(cheats.clone())).unwrap();
    assert_eq!(with_other_cheats.content_id, overridden.content_id);
    assert_eq!(with_other_cheats.updates, scripts.updates);
    cheats[40..44].copy_from_slice(&0u32.to_le_bytes());
    let without_override = prepare(0, 6, Some(cheats)).unwrap();
    assert!(!without_override.player_distance_override);
    assert_eq!(without_override.content_id, scripts.content_id);
    assert!(prepare(0, 6, Some(vec![0; 255])).is_err());
    assert_eq!(
        scripts.updates,
        UpdateSettings {
            task_system: true,
            animation_system: false,
            disable_slaves: true,
            disable_master_slaves: false
        }
    );
    for (normal, min, field) in [(-1, 6, "normal_frame_rate"), (12, 0, "min_frame_rate")] {
        let error = format!("{:#}", prepare(normal, min, None).unwrap_err());
        assert!(
            error.contains("world 17") && error.contains(field),
            "{error}"
        );
    }
}

#[test]
fn native_hooks_are_validated_and_participate_in_content_identity() {
    let mut scripts = cache();
    scripts.actors.insert(
        "HOOK".into(),
        Actor {
            header_index: 0,
            code_origin: 0,
            instances: 1,
            namespace: None,
            instance_variables: vec![vec![]],
            location_offsets: vec![],
            attributes: BTreeMap::new(),
            native_hook: Some(Call {
                function_id: 0,
                function: "NullFunction".into(),
                kind: CallKind::Function,
                target: Object::Me,
                arguments: vec![],
            }),
            program: Program {
                version: 2,
                entry: 0,
                variables: vec![],
                strings: vec![],
                steps: BTreeMap::from([(
                    0,
                    Step {
                        next: 1,
                        operation: Operation::End { target: 0 },
                    },
                )]),
            },
        },
    );
    scripts.content_id = scripts.content_identity().unwrap();
    scripts.validate().unwrap();
    let mut changed = scripts.clone();
    changed.actors.get_mut("HOOK").unwrap().native_hook = None;
    assert!(
        changed
            .validate()
            .unwrap_err()
            .to_string()
            .contains("identity mismatch")
    );
    for hook in [
        Call {
            function_id: 1,
            function: "Missing".into(),
            kind: CallKind::Function,
            target: Object::Me,
            arguments: vec![],
        },
        Call {
            function_id: 0,
            function: "NullFunction".into(),
            kind: CallKind::Task,
            target: Object::Me,
            arguments: vec![],
        },
        Call {
            function_id: 0,
            function: "NullFunction".into(),
            kind: CallKind::Function,
            target: Object::Player,
            arguments: vec![],
        },
    ] {
        let mut changed = scripts.clone();
        changed.actors.get_mut("HOOK").unwrap().native_hook = Some(hook);
        changed.content_id = changed.content_identity().unwrap();
        assert!(
            changed
                .validate()
                .unwrap_err()
                .to_string()
                .contains("actor HOOK native hook")
        );
    }
}

#[test]
fn update_switches_are_required_and_part_of_cache_identity() {
    let scripts = cache();
    for field in [
        "task_system",
        "animation_system",
        "disable_slaves",
        "disable_master_slaves",
    ] {
        let mut data = serde_json::to_value(&scripts).unwrap();
        let value = data["updates"][field].as_bool().unwrap();
        data["updates"][field] = (!value).into();
        let changed: Scripts = serde_json::from_value(data.clone()).unwrap();
        assert!(
            changed
                .validate()
                .unwrap_err()
                .to_string()
                .contains("content identity")
        );
        data["updates"].as_object_mut().unwrap().remove(field);
        assert!(serde_json::from_value::<Scripts>(data).is_err());
    }
}

#[cfg(feature = "parsers")]
#[test]
fn installed_update_switch_conversion_rejects_missing_inputs() {
    let text = "[system]\ntask_system=no\nanimation_system=yes\n[debug]\ndisable_slaves=no\ndisable_master_slaves=yes\n";
    let parse = |text: &str| {
        UpdateSettings::from_system(&rgpre::import::system_ini::SystemIni::parse(text).unwrap())
    };
    assert_eq!(
        parse(text).unwrap(),
        UpdateSettings {
            task_system: false,
            animation_system: true,
            disable_slaves: false,
            disable_master_slaves: true
        }
    );
    for line in [
        "task_system=no\n",
        "animation_system=yes\n",
        "disable_slaves=no\n",
        "disable_master_slaves=yes\n",
    ] {
        let error = parse(&text.replace(line, "")).unwrap_err().to_string();
        assert!(error.contains(line.split('=').next().unwrap()), "{error}");
    }
}

#[test]
fn authored_location_offsets_participate_in_content_identity() {
    let mut scripts = cache();
    let mut actor = cyrus_assets::script::Actor {
        header_index: 0,
        code_origin: 0,
        instances: 1,
        namespace: None,
        instance_variables: vec![vec![]],
        location_offsets: vec![[1, 2, 3]],
        program: Program {
            version: 2,
            entry: 0,
            variables: vec![],
            strings: vec![],
            steps: BTreeMap::from([(
                0,
                Step {
                    next: 0,
                    operation: Operation::End { target: 0 },
                },
            )]),
        },
        attributes: BTreeMap::new(),
        native_hook: None,
    };
    scripts.actors.insert("ACTOR".into(), actor.clone());
    scripts.content_id = scripts.content_identity().unwrap();
    actor.location_offsets[0][1] = 4;
    scripts.actors.insert("ACTOR".into(), actor);
    assert!(
        scripts
            .validate()
            .unwrap_err()
            .to_string()
            .contains("identity mismatch")
    );
}
