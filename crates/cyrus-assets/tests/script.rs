//! Script cache contracts use authored data and optional read-only installed extraction.
use cyrus_assets::script::*;
use cyrus_script::program::*;
use std::collections::BTreeMap;
fn cache() -> Scripts {
    let mut scripts = Scripts {
        version: 27,
        weapons: Default::default(),
        camera: Default::default(),
        combat_attacks: vec![],
        combat_defenses: vec![],
        in_combat_threshold: None,
        lineup_distance: None,
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
            fall_death_ticks: 144,
            dummy_model_manager: false,
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
        shells: Vec::new(),
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
fn combat_motion_metadata_participates_in_cache_identity() {
    let mut scripts = cache();
    scripts.combat_attacks.push(combat::Attack {
        is_combo: true,
        motions: vec![],
        min_range: 65535,
        max_range: -1,
    });
    scripts.content_id = scripts.content_identity().unwrap();
    let decoded: Scripts = serde_json::from_slice(&serde_json::to_vec(&scripts).unwrap()).unwrap();
    decoded.validate().unwrap();
    assert_eq!(decoded.combat_attacks, scripts.combat_attacks);
    scripts.combat_attacks[0].max_range = 0;
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
            light_intensity: -123,
            light_radius: 32767,
            light_rgb: [-32768, 19, -7],
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
    assert_eq!(decoded.instances[0].light_intensity, -123);
    assert_eq!(decoded.instances[0].light_radius, 32767);
    assert_eq!(decoded.instances[0].light_rgb, [-32768, 19, -7]);
    let mut changed = decoded.clone();
    changed.instances[0].light_rgb[0] = 0;
    assert!(
        changed
            .validate()
            .unwrap_err()
            .to_string()
            .contains("identity mismatch")
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
        version: 4,
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
    let misc = rgpre::import::combat_ini::CombatIni::parse(
        &std::fs::read_to_string(root.join("COMBAT.INI")).unwrap(),
    )
    .unwrap()
    .misc;
    let mut maps = 0;
    let mut taunts = 0;
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
        assert_eq!(scripts.node_timer, misc.node_timer, "{}", path.display());
        assert_eq!(
            scripts.combat_clank_effects,
            [misc.sword_clank_1, misc.sword_clank_2]
        );
        if !scripts.combat_attacks.is_empty() {
            assert_eq!(
                scripts.non_engaged_spacing,
                misc.non_engaged_spacing,
                "{}",
                path.display()
            );
        }
        assert_eq!(scripts.combat_defenses.len(), 5, "{}", path.display());
        assert_eq!(
            scripts
                .combat_defenses
                .iter()
                .map(|move_| move_.animation_group)
                .collect::<Vec<_>>(),
            [98, 98, 99, 100, 101],
            "{}",
            path.display()
        );
        assert_eq!(scripts.combat_defenses[0].arc, None);
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
                (
                    instance.light_intensity,
                    instance.light_radius,
                    instance.light_rgb
                ),
                (
                    placement.intensity,
                    placement.radius,
                    [placement.red, placement.green, placement.blue]
                ),
                "{} object {} authored light fields",
                path.display(),
                placement.id
            );
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
            assert_eq!(
                actor.combat_taunts.len(),
                usize::from(attribute(85)),
                "{name}: authored combat taunt count"
            );
            if attribute(14) != 0 {
                assert_eq!(
                    scripts.combat_attacks.len(),
                    89,
                    "{name}: installed attack table"
                );
                assert_eq!(scripts.combat_attacks[0].motions[0].animation_group, 102);
                assert_eq!(scripts.combat_attacks[0].motions[0].min_range, 12001);
                assert_eq!(scripts.combat_attacks[0].motions[0].max_range, 20000);
                assert_eq!(scripts.combat_attacks[0].motions[0].arc_center, Some(0));
            }
            taunts += actor.combat_taunts.len();
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
    assert!(
        taunts > 0,
        "installed maps must exercise combat-taunt preparation"
    );
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
            combat_taunts: vec![],
            native_hook: Some(Call {
                function_id: 3,
                function: "Hook".into(),
                kind: CallKind::Function,
                target: Object::Me,
                arguments: vec![],
            }),
            program: Program {
                version: 4,
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
    const WORLD_INI: &[u8] =
        b"[world]\nworld_map[17]=MAPS\\ISLAND.rgm\nworld_palette[17]=3DART\\DAY.COL\n";
    impl ReadAssets for Source {
        fn manifest(&self) -> &Manifest {
            &self.manifest
        }
        fn read_range(&self, path: &str, start: u64, length: usize) -> anyhow::Result<Vec<u8>> {
            let bytes = match path {
                "SYSTEM.INI" => self.system.as_slice(),
                "soup386/SOUP386.DEF" => &[],
                "ITEMS.INI" => b"[items]\n",
                "WORLD.INI" => WORLD_INI,
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
        let system = format!("[system]\nmax_active_objects=10\nitem_ini=ITEMS.INI\nrtx_filename=ENGLISH.RTX\nnormal_frame_rate={normal}\nmin_frame_rate={min}\nmax_frame_rate=300\nuse_smooth_fps=yes\nuse_smooth_divisor=no\ntask_system=yes\nanimation_system=no\n[debug]\nconvert_static_angles=no\ndisable_slaves=yes\ndisable_master_slaves=no\n[game]\nplayer_fall_dead_time=12\n[3dmanager]\ndummy_manager=no\n[dialog]\nmenu_start_x=35\nmenu_start_y=25\ndialog_max_menu_items=20\ndialog_max_dialog_lines=20\ndialog_max_text_width=500\nmenu_traverse_delay=4\ndialog_print_text=yes\ndialog_use_speech=yes\ndialog_max_distance=1600\n{camera}", camera = camera_ini(None).replacen("[system]\n", "", 1)).into_bytes();
        let mut source = Source {
            manifest: Manifest {
                files: vec![
                    FileInfo {
                        path: "SYSTEM.INI".into(),
                        bytes: system.len() as u64,
                        digest: "0".repeat(64),
                    },
                    FileInfo {
                        path: "ITEMS.INI".into(),
                        bytes: 8,
                        digest: "0".repeat(64),
                    },
                    FileInfo {
                        path: "WORLD.INI".into(),
                        bytes: WORLD_INI.len() as u64,
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
            None,
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
    assert_eq!(scripts.version, 27);
    assert!(scripts.shells.is_empty(), "no installed shell programs");
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
            fall_death_ticks: 144,
            dummy_model_manager: false,
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

#[cfg(feature = "parsers")]
#[test]
fn falling_death_timeout_uses_installed_signed_clamp_and_wrapping_tick_units() {
    let base = "[system]\ntask_system=yes\nanimation_system=yes\n[debug]\nconvert_static_angles=no\ndisable_slaves=no\ndisable_master_slaves=no\n[3dmanager]\ndummy_manager=no\n[game]\n";
    for (value, ticks) in [
        (-1, 36),
        (0, 36),
        (2, 36),
        (3, 36),
        (12, 144),
        (178956971, -2147483644),
    ] {
        let input = format!("{base}player_fall_dead_time={value}\n");
        let ini = rgpre::import::system_ini::SystemIni::parse(&input).unwrap();
        assert_eq!(
            UpdateSettings::from_system(&ini).unwrap().fall_death_ticks,
            ticks
        );
    }
    let ini = rgpre::import::system_ini::SystemIni::parse(base).unwrap();
    assert!(
        format!("{:#}", UpdateSettings::from_system(&ini).unwrap_err())
            .contains("[game] player_fall_dead_time")
    );
    let scripts = cache();
    let mut saved = serde_json::to_value(&scripts).unwrap();
    saved["updates"]["fall_death_ticks"] = 36.into();
    let changed: Scripts = serde_json::from_value(saved.clone()).unwrap();
    assert_ne!(
        changed.content_identity().unwrap(),
        scripts.content_identity().unwrap()
    );
    saved["updates"]
        .as_object_mut()
        .unwrap()
        .remove("fall_death_ticks");
    assert!(serde_json::from_value::<Scripts>(saved).is_err());
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
            combat_taunts: vec![],
            native_hook: Some(Call {
                function_id: 0,
                function: "NullFunction".into(),
                kind: CallKind::Function,
                target: Object::Me,
                arguments: vec![],
            }),
            program: Program {
                version: 4,
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
    let mut old = cache();
    old.version = 18;
    assert!(
        old.validate()
            .unwrap_err()
            .to_string()
            .contains("re-export")
    );
    for field in [
        "dummy_model_manager",
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
    let text = "[system]\ntask_system=no\nanimation_system=yes\n[debug]\nconvert_static_angles=no\ndisable_slaves=no\ndisable_master_slaves=yes\n[game]\nplayer_fall_dead_time=12\n[3dmanager]\ndummy_manager=no\n";
    let parse = |text: &str| {
        UpdateSettings::from_system(&rgpre::import::system_ini::SystemIni::parse(text).unwrap())
    };
    assert_eq!(
        parse(text).unwrap(),
        UpdateSettings {
            fall_death_ticks: 144,
            dummy_model_manager: false,
            task_system: false,
            animation_system: true,
            disable_slaves: false,
            disable_master_slaves: true
        }
    );
    for (dummy, convert, expected) in [
        (false, false, false),
        (true, false, true),
        (false, true, true),
        (true, true, true),
    ] {
        let input = text
            .replace(
                "dummy_manager=no",
                if dummy {
                    "dummy_manager=yes"
                } else {
                    "dummy_manager=no"
                },
            )
            .replace(
                "convert_static_angles=no",
                if convert {
                    "convert_static_angles=yes"
                } else {
                    "convert_static_angles=no"
                },
            );
        assert_eq!(parse(&input).unwrap().dummy_model_manager, expected);
    }
    for line in [
        "task_system=no\n",
        "animation_system=yes\n",
        "disable_slaves=no\n",
        "disable_master_slaves=yes\n",
        "dummy_manager=no\n",
        "convert_static_angles=no\n",
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
            version: 4,
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
        combat_taunts: vec![],
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

#[test]
fn dialogue_settings_are_required_validated_and_part_of_content_identity() {
    let original = cache();
    let mut changed = original.clone();
    changed.dialogue.max_dialog_lines += 1;
    assert_ne!(changed.content_identity().unwrap(), original.content_id);
    assert!(
        changed
            .validate()
            .unwrap_err()
            .to_string()
            .contains("content identity")
    );
    changed.content_id = changed.content_identity().unwrap();
    changed.validate().unwrap();
    for capacity in [0, 15, u32::MAX] {
        let mut invalid = cache();
        invalid.dialogue.max_dialog_lines = capacity;
        invalid.content_id = invalid.content_identity().unwrap();
        assert!(format!("{:#}", invalid.validate().unwrap_err()).contains("dialogue capacity"));
    }
    let mut old = cache();
    old.version = 10;
    assert!(
        old.validate()
            .unwrap_err()
            .to_string()
            .contains("re-export")
    );
    let mut missing = serde_json::to_value(original).unwrap();
    missing.as_object_mut().unwrap().remove("dialogue");
    assert!(serde_json::from_value::<Scripts>(missing).is_err());
}

#[cfg(feature = "parsers")]
#[test]
fn installed_dialogue_settings_apply_original_minima_without_guessing_missing_inputs() {
    let text = "[system]\nrtx_filename=ENGLISH.RTX\n[dialog]\nmenu_start_x=35\nmenu_start_y=25\ndialog_max_menu_items=20\ndialog_max_dialog_lines=1\ndialog_max_text_width=20\nmenu_traverse_delay=0\ndialog_print_text=yes\ndialog_use_speech=no\ndialog_max_distance=1600\n";
    let parse = |text: &str| {
        DialogueSettings::from_system(&rgpre::import::system_ini::SystemIni::parse(text).unwrap())
    };
    let settings = parse(text).unwrap();
    assert_eq!(settings.max_menu_items, 300);
    assert_eq!(settings.max_dialog_lines, 16);
    assert_eq!(settings.max_text_width, 150);
    assert_eq!(settings.menu_traverse_delay, 1);
    assert_eq!(settings.menu_start, [35, 25]);
    assert!(settings.print_text);
    assert!(!settings.use_speech);
    assert!(
        parse(&text.replace("dialog_max_dialog_lines=1\n", ""))
            .unwrap_err()
            .to_string()
            .contains("dialog_max_dialog_lines")
    );
    assert!(parse(&text.replace("menu_traverse_delay=0", "menu_traverse_delay=-1")).is_err());
}

#[test]
fn combat_taunt_records_participate_in_identity_and_require_the_complete_actor_list() {
    let mut scripts = cache();
    scripts.actors.insert(
        "TEST".into(),
        Actor {
            header_index: 0,
            code_origin: 0,
            instances: 1,
            namespace: None,
            instance_variables: vec![vec![]],
            location_offsets: vec![],
            program: Program {
                version: 4,
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
            attributes: BTreeMap::from([("attr_num_taunts".into(), 1)]),
            native_hook: None,
            combat_taunts: vec![CombatTaunt {
                trigger: 0,
                label: *b"AbCd",
                animation: 0,
            }],
        },
    );
    scripts.content_id = scripts.content_identity().unwrap();
    scripts.validate().unwrap();
    scripts.actors.get_mut("TEST").unwrap().combat_taunts[0].label = *b"EfGh";
    assert!(
        scripts
            .validate()
            .unwrap_err()
            .to_string()
            .contains("identity mismatch")
    );
    scripts.content_id = scripts.content_identity().unwrap();
    scripts.validate().unwrap();
    scripts
        .actors
        .get_mut("TEST")
        .unwrap()
        .combat_taunts
        .clear();
    scripts.content_id = scripts.content_identity().unwrap();
    assert!(
        scripts
            .validate()
            .unwrap_err()
            .to_string()
            .contains("actor TEST: incomplete combat taunts")
    );
    scripts.version = 11;
    scripts.content_id = scripts.content_identity().unwrap();
    assert!(
        scripts
            .validate()
            .unwrap_err()
            .to_string()
            .contains("script schema 11 unsupported")
    );
}

#[test]
fn fighting_threshold_is_retained_and_part_of_content_identity() {
    let mut scripts = cache();
    scripts.in_combat_threshold = Some(-256);
    scripts.content_id = scripts.content_identity().unwrap();
    let decoded: Scripts = serde_json::from_slice(&serde_json::to_vec(&scripts).unwrap()).unwrap();
    decoded.validate().unwrap();
    assert_eq!(decoded.in_combat_threshold, Some(-256));
    scripts.in_combat_threshold = None;
    assert!(
        scripts
            .validate()
            .unwrap_err()
            .to_string()
            .contains("identity mismatch")
    );
}

fn camera_ini(skip: Option<&str>) -> String {
    let modes = [
        "combat",
        "hang",
        "rope",
        "rope_above",
        "rope_below",
        "debug",
    ];
    let mut keys = vec![
        ("offset_pos_x", 1),
        ("offset_pos_y", 2),
        ("offset_pos_z", 3),
        ("offset_angle_x", 4),
        ("offset_angle_y", 5),
        ("offset_angle_z", 6),
        ("target_offset_x", 7),
        ("target_offset_y", 8),
        ("target_offset_z", 9),
        ("camera_distance", 250),
        ("camera_min_distance", 120),
        ("max_vel", 900),
        ("obstacle_size", 20),
        ("max_acc", 5000),
        ("max_y_vel", 4000),
        ("camera_size", 10),
        ("camera_scape_size", 5),
        ("glide_x", 1),
        ("glide_y", 2),
        ("glide_z", 3),
        ("glide_angle_x", 4),
        ("glide_angle_y", 5),
        ("glide_angle_z", 6),
    ];
    let mut names = Vec::new();
    for (slot, mode) in modes.iter().enumerate() {
        let base = 10 * (slot as i32 + 1);
        for (suffix, value) in [
            ("angle_offset_x", base),
            ("angle_offset_y", base + 1),
            ("angle_offset_z", base + 2),
            ("distance", base * 10),
            ("max_vel", base + 5),
        ] {
            names.push((format!("camera_{mode}_{suffix}"), value));
        }
    }
    let mut text = String::from("[system]\n[camera]\n");
    for (key, value) in keys.drain(..) {
        if Some(key) != skip {
            text += &format!("{key}={value}\n");
        }
    }
    for (key, value) in names {
        if Some(key.as_str()) != skip {
            text += &format!("{key}={value}\n");
        }
    }
    text
}

#[test]
fn camera_settings_read_every_installed_key_and_reject_a_missing_one() {
    use cyrus_assets::script::CameraSettings;
    let parse = |skip| {
        CameraSettings::from_system(
            &rgpre::import::system_ini::SystemIni::parse(&camera_ini(skip)).unwrap(),
        )
    };
    let settings = parse(None).unwrap();
    assert_eq!(settings.offset_pos, [1, 2, 3]);
    assert_eq!(settings.target_offset, [7, 8, 9]);
    assert_eq!(
        (settings.distance, settings.min_distance, settings.max_vel),
        (250, 120, 900)
    );
    assert_eq!(
        (
            settings.limits.obstacle_size,
            settings.limits.max_acc,
            settings.limits.max_y_vel,
            settings.limits.camera_size,
            settings.limits.camera_scape_size
        ),
        (20, 5000, 4000, 10, 5)
    );
    assert_eq!(settings.glide.position, [1.0, 2.0, 3.0]);
    assert_eq!(settings.glide.angles, [4.0, 5.0, 6.0]);
    let error = parse(Some("max_acc")).unwrap_err();
    assert!(format!("{error:#}").contains("max_acc"), "{error:#}");
    let error = parse(Some("glide_angle_y")).unwrap_err();
    assert!(format!("{error:#}").contains("glide_angle_y"), "{error:#}");
    assert_eq!(settings.modes[0].angle_offset, [10, 11, 12]);
    assert_eq!(
        (settings.modes[0].distance, settings.modes[0].max_vel),
        (100, 15)
    );
    assert_eq!(settings.modes[5].angle_offset, [60, 61, 62]);
    // An absent per-mode rate is zero (keep the copied rate); other keys are required.
    assert_eq!(
        parse(Some("camera_hang_max_vel")).unwrap().modes[1].max_vel,
        0
    );
    let error = parse(Some("camera_hang_distance")).unwrap_err();
    assert!(
        format!("{error:#}").contains("camera_hang_distance"),
        "{error:#}"
    );
}

#[test]
fn installed_system_ini_supplies_every_required_camera_setting() {
    use std::path::{Path, PathBuf};
    let root = std::env::var_os("REDGUARD_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../redguard"));
    let path = ["SYSTEM.INI", "system.ini"]
        .into_iter()
        .map(|name| root.join(name))
        .find(|path| path.is_file());
    let Some(path) = path else {
        eprintln!("skipped: no install (camera settings)");
        return;
    };
    let text = String::from_utf8_lossy(&std::fs::read(path).unwrap()).into_owned();
    let system = rgpre::import::system_ini::SystemIni::parse(&text).unwrap();
    let settings = cyrus_assets::script::CameraSettings::from_system(&system)
        .unwrap_or_else(|error| panic!("installed SYSTEM.INI [camera]: {error:#}"));
    assert!(settings.distance > 0 && settings.max_vel > 0);
}

#[test]
fn neighbor_spacing_retains_signed_unscaled_values_and_invalidates_old_identity() {
    let mut scripts = cache();
    let original = scripts.content_id.clone();
    scripts.non_engaged_spacing = Some(-123);
    scripts.content_id = scripts.content_identity().unwrap();
    assert_ne!(scripts.content_id, original);
    let decoded: Scripts = serde_json::from_slice(&serde_json::to_vec(&scripts).unwrap()).unwrap();
    decoded.validate().unwrap();
    assert_eq!(decoded.non_engaged_spacing, Some(-123));
    scripts.non_engaged_spacing = None;
    assert!(
        scripts
            .validate()
            .unwrap_err()
            .to_string()
            .contains("identity mismatch")
    );
}

#[test]
fn defense_metadata_round_trips_and_participates_in_cache_identity() {
    let mut scripts = cache();
    scripts.combat_defenses.push(combat::Defense {
        animation_group: 102,
        min_range: None,
        max_range: 100,
        first_collide_frame: Some(3),
        collide_duration: Some(2),
        arc: None,
    });
    scripts.content_id = scripts.content_identity().unwrap();
    let decoded: Scripts = serde_json::from_slice(&serde_json::to_vec(&scripts).unwrap()).unwrap();
    decoded.validate().unwrap();
    assert_eq!(decoded.combat_defenses, scripts.combat_defenses);
    scripts.combat_defenses[0].animation_group = 103;
    assert!(
        scripts
            .validate()
            .unwrap_err()
            .to_string()
            .contains("identity mismatch")
    );
}

#[test]
fn perception_delay_retains_signed_ticks_and_invalidates_old_exports() {
    let mut scripts = cache();
    let original = scripts.content_id.clone();
    scripts.node_timer = Some(-123);
    scripts.content_id = scripts.content_identity().unwrap();
    assert_ne!(scripts.content_id, original);
    let decoded: Scripts = serde_json::from_slice(&serde_json::to_vec(&scripts).unwrap()).unwrap();
    decoded.validate().unwrap();
    assert_eq!(decoded.node_timer, Some(-123));
    scripts.node_timer = None;
    assert!(
        scripts
            .validate()
            .unwrap_err()
            .to_string()
            .contains("identity mismatch")
    );
    scripts.version = 17;
    assert!(
        scripts
            .validate()
            .unwrap_err()
            .to_string()
            .contains("re-export")
    );
}

#[test]
fn clank_effects_are_required_and_participate_in_cache_identity() {
    let mut scripts = cache();
    let identity = scripts.content_identity().unwrap();
    scripts.combat_clank_effects = [Some(31), Some(32)];
    assert_ne!(identity, scripts.content_identity().unwrap());
    let mut saved = serde_json::to_value(&scripts).unwrap();
    assert_eq!(
        serde_json::from_value::<Scripts>(saved.clone())
            .unwrap()
            .combat_clank_effects,
        [Some(31), Some(32)]
    );
    saved
        .as_object_mut()
        .unwrap()
        .remove("combat_clank_effects");
    assert!(serde_json::from_value::<Scripts>(saved).is_err());
}

#[test]
fn weapon_geometry_round_trips_and_invalidates_changed_lengths() {
    let mut scripts = cache();
    scripts.weapons.insert(
        1,
        weapon::Weapon {
            hand_model: "HAND".into(),
            hilt_model: "HILT".into(),
            length: -17,
        },
    );
    scripts.content_id = scripts.content_identity().unwrap();
    let decoded: Scripts = serde_json::from_slice(&serde_json::to_vec(&scripts).unwrap()).unwrap();
    decoded.validate().unwrap();
    assert_eq!(decoded.weapons[&1].length, -17);
    scripts.weapons.get_mut(&1).unwrap().length = 18;
    assert!(
        scripts
            .validate()
            .unwrap_err()
            .to_string()
            .contains("identity mismatch")
    );
}
