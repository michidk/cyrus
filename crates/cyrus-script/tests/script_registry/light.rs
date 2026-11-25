use super::*;

#[test]
fn light_controls_create_a_disabled_record_then_update_it_with_low_word_stores() {
    use cyrus_script::script::{ScriptInputs, ScriptStage};
    let light = |calls: &[(Native, &[i32])]| {
        run_native_world(calls).unwrap().actors()[0]
            .light_state()
            .unwrap()
    };
    let fresh = light(&[(Native::LightOffset, &[1, 2, 3])]);
    assert_eq!(fresh["enabled"], false);
    assert_eq!(
        (fresh["intensity"].clone(), fresh["radius"].clone()),
        (256.into(), 100.into())
    );
    assert_eq!(fresh["offset"], serde_json::json!([1, 2, 3]));
    let lit = light(&[(Native::Light, &[50, 200])]);
    assert_eq!(lit["enabled"], true);
    assert_eq!(
        (lit["radius"].clone(), lit["intensity"].clone()),
        (50.into(), 200.into())
    );
    assert_eq!(lit["starting_radius"], 50);
    assert_eq!(
        light(&[(Native::LightIntensity, &[65537])])["intensity"],
        1,
        "low word kept"
    );
    assert_eq!(light(&[(Native::LightRadius, &[70])])["enabled"], true);
    assert_eq!(
        light(&[(Native::LightFlicker, &[7, 9])])["flicker"],
        serde_json::json!([7, 0, 9])
    );
    assert_eq!(
        light(&[(Native::FlickerLight, &[7])])["flicker"],
        serde_json::json!([7, 0, 1])
    );
    assert_eq!(
        light(&[
            (Native::LightFlicker, &[7, 9]),
            (Native::LightFlickerOff, &[])
        ])["flicker"],
        serde_json::json!([0, 0, 9])
    );
    assert_eq!(
        light(&[(Native::LightSize, &[7, 9])])["size"],
        serde_json::json!([7, 0, 9])
    );
    assert_eq!(
        light(&[(Native::LightSize, &[7, 9]), (Native::LightSizeOff, &[])])["size"],
        serde_json::json!([0, 0, 9]),
        "the size effect has its own words, apart from the flicker's"
    );
    let mut malformed =
        random_scripts(Native::Light, vec![Value::Integer { value: 1 }], Object::Me);
    malformed.definition.functions[1].param_count = 1;
    let mut world = ScriptWorld::prepare(&malformed, 1).unwrap();
    let error = world
        .advance(
            world.id(0).unwrap(),
            ScriptStage::Body(Thread::Main),
            ScriptInputs::default(),
            None,
            None,
            20,
        )
        .unwrap_err();
    assert!(format!("{error:#}").contains("wrong argument count"));
    assert!(world.actors()[0].light_state().is_none());
}

#[test]
fn a_type_four_object_starts_with_its_placement_intensity_and_radius_and_no_colour() {
    use cyrus_script::script::{ScriptInputs, ScriptStage};
    let mut scripts = random_scripts(
        Native::LightFlicker,
        vec![Value::Integer { value: 7 }, Value::Integer { value: 9 }],
        Object::Me,
    );
    scripts.definition.functions[1].param_count = 2;
    scripts.instances.truncate(1);
    let placement = &mut scripts.instances[0];
    placement.object_type = PlacementType::LIGHT;
    placement.light_intensity = 211;
    placement.light_radius = 900;
    placement.light_rgb = [-12, 34, 56];
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let light = world.actors()[0].light_state().unwrap();
    assert_eq!(
        (
            &light["enabled"],
            &light["locked"],
            &light["intensity"],
            &light["radius"],
            &light["rgb"]
        ),
        (
            &true.into(),
            &true.into(),
            &211.into(),
            &900.into(),
            &serde_json::json!([0, 0, 0])
        )
    );
    let id = world.id(0).unwrap();
    world
        .advance(
            id,
            ScriptStage::Body(Thread::Main),
            ScriptInputs::default(),
            None,
            None,
            20,
        )
        .unwrap();
    assert_eq!(
        world.actors()[0].light_state().unwrap()["flicker"],
        serde_json::json!([7, 0, 9])
    );
    assert!(
        world.object_lights().is_empty(),
        "type-four lights are not projected as map lights"
    );
}

#[test]
fn snapshot_marks_only_type_six_placements_as_map_lights() {
    use cyrus_script::script::WorldSnapshotSources;
    let mut scripts = random_scripts(Native::Spare0Parm0, vec![], Object::Me);
    scripts.definition.functions[1].param_count = 0;
    scripts.instances.truncate(2);
    for (placement, object_type) in scripts
        .instances
        .iter_mut()
        .zip([PlacementType::COLOURED_LIGHT, PlacementType::LIGHT])
    {
        placement.object_type = object_type;
        placement.light_intensity = 211;
        placement.light_radius = 900;
        placement.light_rgb = [12, 34, 56];
    }
    scripts.content_id = scripts.content_identity().unwrap();
    let (_, _, animations) = revisit_test_app(&scripts);
    let collision = cyrus_assets::collision::CollisionScene::default();
    let sources = WorldSnapshotSources {
        identity_memo: None,
        effects: None,
        player: None,
        scripts: &scripts,
        animations: &animations,
        collision: &collision,
    };
    let world = ScriptWorld::prepare(&scripts, 1).unwrap();
    assert_eq!(
        world.object_lights().len(),
        1,
        "only the type-six placement is a map light"
    );
    let saved = serde_json::to_value(world.snapshot(&sources).unwrap()).unwrap();
    assert_eq!(saved["actors"][0]["map_light"], true);
    assert_eq!(saved["actors"][1]["map_light"], false);
    for index in 0..2 {
        let mut mismatched = saved.clone();
        mismatched["actors"][index]["map_light"] = serde_json::json!(index != 0);
        let error = ScriptWorld::from_snapshot(
            serde_json::from_value(mismatched).unwrap(),
            &sources,
            cyrus_script::script::WorldRestoreContext {
                generation: 2,
                globals: cyrus_script::script::GameplayGlobals::default(),
                samples: &BTreeMap::new(),
            },
        )
        .err()
        .expect("installed map-light eligibility must not be changed by a save");
        assert!(
            format!("{error:#}").contains("map-light eligibility differs"),
            "{error:#}"
        );
        assert_eq!(
            world.object_lights().len(),
            1,
            "rejected restore leaves the live light owner unchanged"
        );
    }
}

#[test]
fn authored_light_off_survives_world_replacement_without_reenabling() {
    use cyrus_script::script::{
        GameplayGlobals, ScriptInputs, ScriptStage, WorldRestoreContext, WorldSnapshotSources,
    };
    let mut scripts = random_scripts(Native::LightOff, vec![], Object::Me);
    scripts.definition.functions[1].param_count = 0;
    scripts.instances.truncate(1);
    let placement = &mut scripts.instances[0];
    placement.object_type = PlacementType::COLOURED_LIGHT;
    placement.light_intensity = 211;
    placement.light_radius = 900;
    placement.light_rgb = [-12, 34, 56];
    placement.map_position = [100, -200, 300];
    scripts.content_id = scripts.content_identity().unwrap();
    let (_, _, animations) = revisit_test_app(&scripts);
    let collision = cyrus_assets::collision::CollisionScene::default();
    let sources = WorldSnapshotSources {
        identity_memo: None,
        effects: None,
        player: None,
        scripts: &scripts,
        animations: &animations,
        collision: &collision,
    };
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let lights = world.object_lights();
    assert_eq!(lights.len(), 1);
    assert_eq!(lights[0].position, [100, -200, 300]);
    assert_eq!(lights[0].radius, 900);
    assert_eq!(lights[0].rgb, [-12, 34, 56]);
    let id = world.id(0).unwrap();
    world
        .advance(
            id,
            ScriptStage::Body(Thread::Main),
            ScriptInputs::default(),
            None,
            None,
            20,
        )
        .unwrap();
    assert!(world.object_lights().is_empty());
    assert_eq!(world.actors()[0].runtime().unwrap().variables(), [0]);
    let snapshot = world.snapshot(&sources).unwrap();
    let snapshot = serde_json::from_slice(&serde_json::to_vec(&snapshot).unwrap()).unwrap();
    let replacement = ScriptWorld::from_snapshot(
        snapshot,
        &sources,
        WorldRestoreContext {
            generation: 2,
            globals: GameplayGlobals::default(),
            samples: &BTreeMap::new(),
        },
    )
    .unwrap();
    assert!(replacement.object_lights().is_empty());
    assert_eq!(replacement.actors()[0].runtime().unwrap().variables(), [0]);
    let mut saved = serde_json::to_value(replacement.snapshot(&sources).unwrap()).unwrap();
    saved["actors"][0]["light"]["position"][0] = 101.into();
    assert!(
        ScriptWorld::from_snapshot(
            serde_json::from_value(saved).unwrap(),
            &sources,
            WorldRestoreContext {
                generation: 3,
                globals: GameplayGlobals::default(),
                samples: &BTreeMap::new()
            },
        )
        .is_err()
    );
}

#[test]
fn type_four_object_light_survives_world_replacement() {
    use cyrus_script::script::{GameplayGlobals, WorldRestoreContext, WorldSnapshotSources};
    let mut scripts = random_scripts(Native::Spare0Parm0, vec![], Object::Me);
    scripts.definition.functions[1].param_count = 0;
    scripts.instances.truncate(1);
    let placement = &mut scripts.instances[0];
    placement.object_type = PlacementType::LIGHT;
    placement.light_intensity = 56;
    placement.light_radius = 200;
    placement.map_position = [100, -200, 300];
    scripts.content_id = scripts.content_identity().unwrap();
    let (_, _, animations) = revisit_test_app(&scripts);
    let collision = cyrus_assets::collision::CollisionScene::default();
    let sources = WorldSnapshotSources {
        identity_memo: None,
        effects: None,
        player: None,
        scripts: &scripts,
        animations: &animations,
        collision: &collision,
    };
    let world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let snapshot = world.snapshot(&sources).unwrap();
    let snapshot = serde_json::from_slice(&serde_json::to_vec(&snapshot).unwrap()).unwrap();
    ScriptWorld::from_snapshot(
        snapshot,
        &sources,
        WorldRestoreContext {
            generation: 2,
            globals: GameplayGlobals::default(),
            samples: &BTreeMap::new(),
        },
    )
    .unwrap();
}

#[test]
fn light_off_without_a_record_returns_zero_without_creating_a_light() {
    use cyrus_script::script::{ScriptInputs, ScriptStage};
    let mut scripts = random_scripts(Native::LightOff, vec![], Object::Me);
    scripts.definition.functions[1].param_count = 0;
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let id = world.id(0).unwrap();
    world
        .advance(
            id,
            ScriptStage::Body(Thread::Main),
            ScriptInputs::default(),
            None,
            None,
            20,
        )
        .unwrap();
    assert!(world.object_lights().is_empty());
    assert_eq!(world.actors()[0].runtime().unwrap().variables(), [0]);
}

#[test]
fn separate_light_cleanup_runs_with_tasks_disabled_and_uses_wrapping_byte_ticks() {
    use cyrus_script::script::{ScriptInputs, ScriptStage, WorldSnapshotSources};
    for (ticks, retained) in [(32, true), (33, false), (256, true)] {
        let mut scripts = random_scripts(Native::LightOff, vec![], Object::Me);
        scripts.definition.functions[1].param_count = 0;
        scripts.updates.task_system = false;
        scripts.instances.truncate(1);
        scripts.instances[0].object_type = PlacementType::COLOURED_LIGHT;
        scripts.instances[0].map_position = [600, 0, 600];
        scripts.content_id = scripts.content_identity().unwrap();
        let (_, _, animations) = revisit_test_app(&scripts);
        let collision = cyrus_assets::collision::CollisionScene::default();
        let sources = WorldSnapshotSources {
            identity_memo: None,
            effects: None,
            player: None,
            scripts: &scripts,
            animations: &animations,
            collision: &collision,
        };
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        let id = world.id(0).unwrap();
        world
            .advance(
                id,
                ScriptStage::Body(Thread::Main),
                ScriptInputs::default(),
                None,
                None,
                20,
            )
            .unwrap();
        world
            .update_separate(
                [600, 600],
                ScriptInputs {
                    nominal_ticks: Some(ticks),
                    frame_scale: Some(256),
                    ..Default::default()
                },
                &mut cyrus_game::random::EngineRandom::default(),
                &mut |_, _| Ok(()),
                100,
            )
            .unwrap();
        assert!(world.actors()[0].failure().is_none());
        let saved = serde_json::to_value(world.snapshot(&sources).unwrap()).unwrap();
        assert_eq!(
            !saved["actors"][0]["light"].is_null(),
            retained,
            "ticks {ticks}"
        );
        assert!(world.object_lights().is_empty());
    }
}

#[test]
fn installed_authored_lights_reach_logical_render_projection_in_map_order() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../redguard");
    if !root.join("RGFX.EXE").exists() {
        eprintln!("skipped: no install (authored logical lights)");
        return;
    }
    let mut count = 0;
    for entry in std::fs::read_dir(root.join("maps")).unwrap() {
        let path = entry.unwrap().path();
        if !path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("rgm"))
        {
            continue;
        }
        let map = rgpre::import::rgm::parse_rgm_file(&std::fs::read(&path).unwrap()).unwrap();
        let expected: Vec<_> = map
            .sections
            .iter()
            .filter_map(|section| match section {
                rgpre::import::rgm::RgmSection::MpobParsed(_, records) => Some(records),
                _ => None,
            })
            .flatten()
            .filter(|record| {
                PlacementType::from_byte(record.object_type) == PlacementType::COLOURED_LIGHT
            })
            .collect();
        let scripts = cyrus_assets::script_export::prepare(&map, &root, 0).unwrap();
        let world = ScriptWorld::prepare(&scripts, 1).unwrap();
        let lights = world.object_lights();
        assert_eq!(
            lights.len(),
            expected.len(),
            "{} light census",
            path.display()
        );
        for (light, record) in lights.iter().zip(expected) {
            assert_eq!(
                light.radius,
                record.radius,
                "{} light radius",
                path.display()
            );
            assert_eq!(
                light.rgb,
                [record.red, record.green, record.blue],
                "{} light RGB",
                path.display()
            );
        }
        count += lights.len();
    }
    assert!(count > 0, "installed maps must exercise authored lights");
}

#[test]
fn moved_authored_light_uses_current_pose_after_snapshot_replacement() {
    use cyrus_script::script::{
        GameplayGlobals, ScriptInputs, ScriptStage, WorldRestoreContext, WorldSnapshotSources,
    };
    let mut scripts = cache();
    scripts.definition.functions.push(Function {
        name: Native::MoveObjectAxis,
        param_count: 3,
        kind: CallKind::Task,
    });
    scripts.definition.functions.push(Function {
        name: Native::LightOffset,
        param_count: 3,
        kind: CallKind::Function,
    });
    scripts.instances.truncate(1);
    scripts.instances[0].object_type = PlacementType::COLOURED_LIGHT;
    scripts.instances[0].map_position = [100, -200, 300];
    scripts
        .actors
        .get_mut("LOGIC")
        .unwrap()
        .program
        .steps
        .get_mut(&1)
        .unwrap()
        .operation = wait(
        1,
        Native::MoveObjectAxis,
        CallKind::Task,
        vec![
            Value::Integer { value: 0 },
            Value::Integer { value: 512 },
            Value::Integer { value: 2 },
        ],
    );
    scripts
        .actors
        .get_mut("LOGIC")
        .unwrap()
        .program
        .steps
        .insert(0, assign_call(1, 2, Native::LightOffset, &[256, -128, 512]));
    scripts.content_id = scripts.content_identity().unwrap();
    let (_, _, animations) = revisit_test_app(&scripts);
    let collision = cyrus_assets::collision::CollisionScene::default();
    let sources = WorldSnapshotSources {
        identity_memo: None,
        effects: None,
        player: None,
        scripts: &scripts,
        animations: &animations,
        collision: &collision,
    };
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world.prepare_spatial().unwrap();
    let id = world.id(0).unwrap();
    world
        .advance(
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
        )
        .unwrap();
    world.refresh_families().unwrap();
    world
        .advance(
            id,
            ScriptStage::Tasks,
            ScriptInputs {
                frame_scale: Some(256),
                nominal_ticks: Some(1),
                ..Default::default()
            },
            None,
            None,
            20,
        )
        .unwrap();
    world.refresh_families().unwrap();
    assert_eq!(world.object_lights()[0].position, [101, -201, 302]);
    let snapshot = world.snapshot(&sources).unwrap();
    let replacement = ScriptWorld::from_snapshot(
        snapshot,
        &sources,
        WorldRestoreContext {
            generation: 2,
            globals: GameplayGlobals::default(),
            samples: &BTreeMap::new(),
        },
    )
    .unwrap();
    assert_eq!(replacement.object_lights()[0].position, [101, -201, 302]);
}

pub(super) fn light_maintenance_scripts(
    kind: PlacementType,
    count: usize,
    calls: &[(Native, &[i32])],
) -> Scripts {
    let mut scripts = cache();
    scripts.instances.truncate(count);
    let actor = scripts.actors.get_mut("LOGIC").unwrap();
    actor.instances = count as u16;
    actor.instance_variables.truncate(count);
    actor.program.steps.clear();
    for (index, (name, arguments)) in calls.iter().enumerate() {
        let function = scripts.definition.functions.len() as u16;
        scripts.definition.functions.push(Function {
            name: *name,
            param_count: arguments.len() as u8,
            kind: CallKind::Function,
        });
        actor.program.steps.insert(
            index as u32,
            assign_call(index as u32 + 1, function, *name, arguments),
        );
    }
    let end = calls.len() as u32;
    actor.program.steps.insert(
        end,
        Step {
            next: end + 1,
            operation: Operation::End { target: end },
        },
    );
    for instance in &mut scripts.instances {
        instance.object_type = kind;
        instance.map_position = [600, 0, 600];
        instance.light_intensity = 256;
        instance.light_radius = 100;
    }
    scripts
}

pub(super) fn light_maintenance_world(
    mut scripts: Scripts,
) -> (Scripts, ScriptWorld, cyrus_assets::animation::Animations) {
    scripts.content_id = scripts.content_identity().unwrap();
    let (_, _, data) = revisit_test_app(&scripts);
    let world = ScriptWorld::prepare(&scripts, 1).unwrap();
    (scripts, world, data)
}

pub(super) fn light_maintenance_update(
    world: &mut ScriptWorld,
    data: &cyrus_assets::animation::Animations,
    separate: bool,
    ticks: u32,
    random: &mut cyrus_game::random::EngineRandom,
) {
    let inputs = cyrus_script::script::ScriptInputs {
        nominal_ticks: Some(ticks),
        frame_scale: Some(256),
        ..Default::default()
    };
    if separate {
        world
            .update_separate([600, 600], inputs, random, &mut |_, _| Ok(()), 100)
            .unwrap();
    } else {
        world
            .update_ordinary(
                data,
                [600, 600],
                cyrus_script::script::OrdinaryInputs {
                    scripts: inputs,
                    scene: None,
                },
                random,
                &mut |_, _| Ok(()),
                100,
            )
            .unwrap();
    }
}

#[test]
fn light_maintenance_precedes_ordinary_scripts_but_follows_separate_scripts() {
    for (kind, separate, first_intensity, first_radius, first_random) in [
        (PlacementType::LIGHT, false, 256, 100, 1103527590),
        (PlacementType::COLOURED_LIGHT, true, 259, 97, 662824084),
    ] {
        let mut scripts = light_maintenance_scripts(
            kind,
            1,
            &[
                (Native::LightFlicker, &[18, 1]),
                (Native::LightSize, &[8, 1]),
                (Native::Rnd, &[255]),
            ],
        );
        scripts
            .actors
            .get_mut("LOGIC")
            .unwrap()
            .program
            .steps
            .get_mut(&3)
            .unwrap()
            .operation = Operation::End { target: 2 };
        let (_, mut world, data) = light_maintenance_world(scripts);
        let mut random = cyrus_game::random::EngineRandom::default();
        light_maintenance_update(&mut world, &data, separate, 1, &mut random);
        let actor = &world.actors()[0];
        assert!(actor.failure().is_none(), "{:?}", actor.failure());
        let light = actor.light_state().unwrap();
        assert_eq!(light["intensity"], first_intensity);
        assert_eq!(light["radius"], first_radius);
        assert_eq!(actor.runtime().unwrap().variables(), [198]);
        assert_eq!(random.state(), first_random);
        if !separate {
            light_maintenance_update(&mut world, &data, false, 1, &mut random);
            let actor = &world.actors()[0];
            assert!(actor.failure().is_none(), "{:?}", actor.failure());
            let light = actor.light_state().unwrap();
            assert_eq!(light["intensity"], 259);
            assert_eq!(light["radius"], 97);
            assert_eq!(actor.runtime().unwrap().variables(), [107]);
            assert_eq!(random.state(), 3295386429);
        }
    }
}

#[test]
fn light_maintenance_consumes_intensity_then_radius_in_shared_actor_order() {
    let scripts = light_maintenance_scripts(
        PlacementType::COLOURED_LIGHT,
        2,
        &[
            (Native::LightFlicker, &[18, 1]),
            (Native::LightSize, &[8, 1]),
        ],
    );
    let (_, mut world, data) = light_maintenance_world(scripts);
    let mut random = cyrus_game::random::EngineRandom::default();
    assert_eq!(
        world
            .separate_actors([600, 600])
            .unwrap()
            .iter()
            .map(|id| id.index)
            .collect::<Vec<_>>(),
        [1, 0]
    );
    light_maintenance_update(&mut world, &data, true, 1, &mut random);
    for (index, intensity, radius) in [(1, 253, 102), (0, 248, 99)] {
        let actor = &world.actors()[index];
        assert!(actor.failure().is_none(), "{:?}", actor.failure());
        let light = actor.light_state().unwrap();
        assert_eq!(light["intensity"], intensity);
        assert_eq!(light["radius"], radius);
    }
    assert_eq!(random.state(), 3295386429);
}

#[test]
fn light_maintenance_uses_signed_thresholds_one_draw_and_word_clamps() {
    // Literal outcomes from original-instruction cases, including a wrapped input word.
    for (
        flicker,
        size,
        ticks,
        start_intensity,
        start_radius,
        seed,
        intensity,
        radius,
        phase,
        state,
    ) in [
        ([18, 2], [8, 2], 1, 256, 100, 1, 256, 100, 1, 1),
        ([18, 3], [8, 7], 100, 256, 100, 1, 253, 102, 0, 2524885223),
        ([18, 0], [8, 0], 0, 256, 100, 1, 253, 102, 0, 2524885223),
        (
            [18, 1],
            [0, 1],
            1,
            32767,
            100,
            2,
            -32766,
            100,
            0,
            2207042835,
        ),
        ([0, 1], [18, 1], 1, 256, 32767, 2, 256, 1, 0, 2207042835),
        ([0, 1], [0, 1], 1, 256, -9, 1, 256, 1, 0, 1),
        ([65536, 1], [0, 1], 1, 256, 100, 1, 256, 100, 0, 1),
    ] {
        let mut scripts = light_maintenance_scripts(
            PlacementType::COLOURED_LIGHT,
            1,
            &[(Native::LightFlicker, &flicker), (Native::LightSize, &size)],
        );
        scripts.instances[0].light_intensity = start_intensity;
        scripts.instances[0].light_radius = start_radius;
        let (_, mut world, data) = light_maintenance_world(scripts);
        let mut random = cyrus_game::random::EngineRandom::from_state(seed);
        light_maintenance_update(&mut world, &data, true, ticks, &mut random);
        let actor = &world.actors()[0];
        assert!(actor.failure().is_none(), "{:?}", actor.failure());
        let light = actor.light_state().unwrap();
        assert_eq!(light["intensity"], intensity);
        assert_eq!(light["radius"], radius);
        assert_eq!(light["flicker"][1], phase);
        assert_eq!(light["size"][1], phase);
        assert_eq!(random.state(), state);
    }
}

#[test]
fn light_maintenance_signed_phase_and_shared_random_continue_after_snapshot() {
    use cyrus_script::script::{GameplayGlobals, WorldRestoreContext, WorldSnapshotSources};
    let scripts = light_maintenance_scripts(
        PlacementType::COLOURED_LIGHT,
        1,
        &[
            (Native::LightFlicker, &[18, 1]),
            (Native::LightSize, &[8, 1]),
        ],
    );
    let (scripts, mut world, data) = light_maintenance_world(scripts);
    let mut random = cyrus_game::random::EngineRandom::default();
    light_maintenance_update(&mut world, &data, true, 32768, &mut random);
    let light = world.actors()[0].light_state().unwrap();
    assert_eq!(light["flicker"][1], 32768);
    assert_eq!(light["size"][1], 32768);
    assert_eq!(random.state(), 1);
    let collision = cyrus_assets::collision::CollisionScene::default();
    let sources = WorldSnapshotSources {
        identity_memo: None,
        effects: None,
        player: None,
        scripts: &scripts,
        animations: &data,
        collision: &collision,
    };
    let snapshot =
        serde_json::from_value(serde_json::to_value(world.snapshot(&sources).unwrap()).unwrap())
            .unwrap();
    let mut restored = ScriptWorld::from_snapshot(
        snapshot,
        &sources,
        WorldRestoreContext {
            generation: 2,
            globals: GameplayGlobals::default(),
            samples: &BTreeMap::new(),
        },
    )
    .unwrap();
    let mut restored_random = cyrus_game::random::EngineRandom::from_state(random.state());
    for ticks in [32768, 1] {
        light_maintenance_update(&mut world, &data, true, ticks, &mut random);
        light_maintenance_update(&mut restored, &data, true, ticks, &mut restored_random);
        assert_eq!(
            world.actors()[0].light_state(),
            restored.actors()[0].light_state()
        );
        assert_eq!(random.state(), restored_random.state());
    }
    let light = restored.actors()[0].light_state().unwrap();
    assert_eq!(light["intensity"], 253);
    assert_eq!(light["radius"], 102);
    assert_eq!(random.state(), 2524885223);
}

#[test]
fn light_maintenance_disabled_record_does_not_consume_random_or_modulate() {
    let scripts = light_maintenance_scripts(
        PlacementType::COLOURED_LIGHT,
        1,
        &[
            (Native::LightFlicker, &[18, 0]),
            (Native::LightSize, &[8, 0]),
            (Native::LightOff, &[]),
        ],
    );
    let (_, mut world, data) = light_maintenance_world(scripts);
    let mut random = cyrus_game::random::EngineRandom::default();
    light_maintenance_update(&mut world, &data, true, 1, &mut random);
    let actor = &world.actors()[0];
    assert!(actor.failure().is_none(), "{:?}", actor.failure());
    let light = actor.light_state().unwrap();
    assert_eq!(light["enabled"], false);
    assert_eq!(light["inactive_ticks"], 1);
    assert_eq!(light["intensity"], 256);
    assert_eq!(light["radius"], 100);
    assert_eq!(light["flicker"][1], 0);
    assert_eq!(light["size"][1], 0);
    assert_eq!(random.state(), 1);
}

#[test]
fn light_maintenance_rejects_negative_wrapped_amplitude_before_mutation_or_draws() {
    for amplitude in [-1, 65535] {
        let scripts = light_maintenance_scripts(
            PlacementType::COLOURED_LIGHT,
            1,
            &[
                (Native::LightFlicker, &[18, 0]),
                (Native::LightSize, &[amplitude, 0]),
            ],
        );
        let (_, mut world, data) = light_maintenance_world(scripts);
        world
            .advance(
                world.id(0).unwrap(),
                ScriptStage::Body(Thread::Main),
                Default::default(),
                None,
                None,
                100,
            )
            .unwrap();
        let before = world.actors()[0].light_state();
        let mut random = cyrus_game::random::EngineRandom::default();
        light_maintenance_update(&mut world, &data, true, 1, &mut random);
        let actor = &world.actors()[0];
        let error = actor.failure().unwrap();
        assert!(
            error.contains("world 7 instance 0 actor LOGIC separate update"),
            "{error}"
        );
        assert!(
            error.contains("LightSize negative amplitude -1 unsupported"),
            "{error}"
        );
        assert_eq!(actor.light_state(), before);
        assert_eq!(random.state(), 1);
    }
}

#[test]
fn light_maintenance_keeps_hidden_ordinary_actor_phases_and_random_unchanged() {
    let scripts = light_maintenance_scripts(
        PlacementType::LIGHT,
        1,
        &[
            (Native::LightFlicker, &[18, 0]),
            (Native::LightSize, &[8, 0]),
            (Native::HideMe, &[]),
        ],
    );
    let (_, mut world, data) = light_maintenance_world(scripts);
    world
        .advance(
            world.id(0).unwrap(),
            ScriptStage::Body(Thread::Main),
            Default::default(),
            None,
            None,
            100,
        )
        .unwrap();
    let before = world.actors()[0].light_state();
    let mut random = cyrus_game::random::EngineRandom::default();
    light_maintenance_update(&mut world, &data, false, 100, &mut random);
    let actor = &world.actors()[0];
    assert!(actor.failure().is_none(), "{:?}", actor.failure());
    assert_eq!(actor.light_state(), before);
    assert_eq!(random.state(), 1);
}

#[test]
fn fx_light_size_configures_the_receiver_and_finishes_without_a_task_slot() {
    use cyrus_script::script::ScriptInputs;
    for authored in [false, true] {
        let mut scripts = random_scripts(Native::FxLightSize, vec![], Object::Me);
        scripts.definition.functions[1].kind = CallKind::Multitask;
        scripts.definition.functions[1].param_count = 2;
        if authored {
            scripts.instances[0].object_type = PlacementType::LIGHT;
            scripts.instances[0].light_intensity = 211;
            scripts.instances[0].light_radius = 900;
        }
        let actor = scripts.actors.get_mut("LOGIC").unwrap();
        actor.program.steps.get_mut(&0).unwrap().operation = wait(
            1,
            Native::FxLightSize,
            CallKind::Multitask,
            vec![
                Value::Integer { value: 65543 },
                Value::Integer { value: -1 },
            ],
        );
        scripts.content_id = scripts.content_identity().unwrap();
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        let id = world.id(0).unwrap();
        world
            .advance(
                id,
                ScriptStage::OrdinaryMain,
                ScriptInputs {
                    frame_scale: Some(256),
                    ..Default::default()
                },
                None,
                None,
                20,
            )
            .unwrap();
        let actor = &world.actors()[0];
        let light = actor.light_state().unwrap();
        assert_eq!(light["size"], serde_json::json!([7, 0, 65535]));
        assert_eq!(light["flicker"], serde_json::json!([0, 0, 0]));
        assert_eq!(light["enabled"], authored);
        assert_eq!(light["locked"], authored);
        assert_eq!(light["radius"], if authored { 900 } else { 100 });
        assert_eq!(light["intensity"], if authored { 211 } else { 256 });
        assert_eq!(actor.runtime().unwrap().active_tasks(), 0);
        assert_eq!(
            actor.runtime().unwrap().program_counter(Thread::Main),
            Some(0)
        );
    }
}

#[test]
fn fx_light_size_multitask_modulates_the_authored_spatial_light_after_dispatch() {
    let mut scripts = light_maintenance_scripts(
        PlacementType::COLOURED_LIGHT,
        1,
        &[(Native::FxLightSize, &[7, 1])],
    );
    scripts.definition.functions[1].kind = CallKind::Multitask;
    scripts
        .actors
        .get_mut("LOGIC")
        .unwrap()
        .program
        .steps
        .get_mut(&0)
        .unwrap()
        .operation = wait(
        1,
        Native::FxLightSize,
        CallKind::Multitask,
        vec![Value::Integer { value: 7 }, Value::Integer { value: 1 }],
    );
    scripts.instances[0].light_rgb = [12, -34, 56];
    let (_, mut world, data) = light_maintenance_world(scripts);
    let mut random = cyrus_game::random::EngineRandom::from_state(1);
    light_maintenance_update(&mut world, &data, true, 1, &mut random);
    assert!(
        world.actors()[0].failure().is_none(),
        "{:?}",
        world.actors()[0].failure()
    );
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
    let lights = world.object_lights();
    assert_eq!(lights.len(), 1);
    assert_eq!(lights[0].position, [600, 0, 600]);
    assert_eq!(lights[0].radius, 103);
    assert_eq!(lights[0].rgb, [12, -34, 56]);
    assert_eq!(random.state(), 1103527590);
    light_maintenance_update(&mut world, &data, true, 1, &mut random);
    assert_eq!(world.object_lights()[0].radius, 103);
    assert_eq!(random.state(), 2524885223);
}

#[test]
fn fx_flicker_on_off_toggles_radius_on_each_retained_multitask_poll() {
    use cyrus_script::script::ScriptInputs;
    for (authored, bound) in [(false, 1), (true, 1), (false, 3), (true, 3)] {
        let mut scripts = random_scripts(Native::FxFlickerOnOff, vec![], Object::Me);
        scripts.definition.functions[1].kind = CallKind::Multitask;
        if authored {
            scripts.instances[0].object_type = PlacementType::LIGHT;
            scripts.instances[0].light_radius = 900;
        }
        scripts
            .actors
            .get_mut("LOGIC")
            .unwrap()
            .program
            .steps
            .get_mut(&0)
            .unwrap()
            .operation = wait(
            1,
            Native::FxFlickerOnOff,
            CallKind::Multitask,
            vec![Value::Integer { value: bound }],
        );
        scripts.content_id = scripts.content_identity().unwrap();
        let (_, _, animations) = revisit_test_app(&scripts);
        let collision = Default::default();
        let sources = cyrus_script::script::WorldSnapshotSources {
            identity_memo: None,
            effects: None,
            player: None,
            scripts: &scripts,
            animations: &animations,
            collision: &collision,
        };
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        let mut id = world.id(0).unwrap();
        let mut random = cyrus_game::random::EngineRandom::from_state(1);
        let starting = if authored { 900 } else { 100 };
        let toggled = if bound == 1 { 0 } else { starting };
        for (stage, radius, seed) in [
            (ScriptStage::Body(Thread::Main), toggled, 1103527590),
            (
                ScriptStage::Tasks,
                if authored { 900 } else { 100 },
                2524885223,
            ),
            (ScriptStage::Tasks, toggled, 662824084),
        ] {
            world
                .advance(
                    id,
                    stage,
                    ScriptInputs {
                        frame_scale: Some(256),
                        ..Default::default()
                    },
                    None,
                    Some(&mut random),
                    20,
                )
                .unwrap();
            let actor = &world.actors()[0];
            let light = actor.light_state().unwrap();
            assert_eq!(light["radius"], radius);
            assert_eq!(light["starting_radius"], if authored { 900 } else { 100 });
            assert_eq!(light["enabled"], authored);
            assert_eq!(light["locked"], authored);
            assert_eq!(actor.runtime().unwrap().active_tasks(), 1);
            assert_eq!(random.state(), seed);
            if seed == 1103527590 {
                let snapshot = world.snapshot(&sources).unwrap();
                world = ScriptWorld::from_snapshot(
                    snapshot,
                    &sources,
                    cyrus_script::script::WorldRestoreContext {
                        generation: 2,
                        globals: Default::default(),
                        samples: &Default::default(),
                    },
                )
                .unwrap();
                id = world.id(0).unwrap();
                random = cyrus_game::random::EngineRandom::from_state(seed);
            }
        }
    }
}

#[test]
fn fx_flicker_on_off_rejects_nonpositive_bounds_before_allocating_or_drawing() {
    for bound in [0, -1, i32::MIN] {
        let mut scripts = random_scripts(Native::FxFlickerOnOff, vec![], Object::Me);
        scripts.definition.functions[1].kind = CallKind::Multitask;
        scripts
            .actors
            .get_mut("LOGIC")
            .unwrap()
            .program
            .steps
            .get_mut(&0)
            .unwrap()
            .operation = wait(
            1,
            Native::FxFlickerOnOff,
            CallKind::Multitask,
            vec![Value::Integer { value: bound }],
        );
        scripts.content_id = scripts.content_identity().unwrap();
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        let id = world.id(0).unwrap();
        let mut random = cyrus_game::random::EngineRandom::from_state(1);
        let error = world
            .advance(
                id,
                ScriptStage::Body(Thread::Main),
                Default::default(),
                None,
                Some(&mut random),
                20,
            )
            .unwrap_err();
        assert!(format!("{error:#}").contains("FxFlickerOnOff requires a positive bound"));
        assert!(world.actors()[0].light_state().is_none());
        assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
        assert_eq!(random.state(), 1);
    }
}

fn phase_scripts(values: [i32; 7]) -> Scripts {
    let mut scripts = random_scripts(Native::FxPhase, vec![], Object::Me);
    scripts.definition.functions[1].kind = CallKind::Multitask;
    scripts.definition.functions[1].param_count = 7;
    scripts
        .actors
        .get_mut("LOGIC")
        .unwrap()
        .program
        .steps
        .get_mut(&0)
        .unwrap()
        .operation = wait(
        1,
        Native::FxPhase,
        CallKind::Multitask,
        values.map(|value| Value::Integer { value }).to_vec(),
    );
    scripts.content_id = scripts.content_identity().unwrap();
    scripts
}

#[test]
fn fx_phase_tasks_keep_independent_colour_progress_and_resume_after_snapshot() {
    use cyrus_script::script::{ScriptInputs, WorldRestoreContext, WorldSnapshotSources};
    let scripts = phase_scripts([0, 100, -100, 80, 20, 60, 4]);
    let (_, _, animations) = revisit_test_app(&scripts);
    let collision = Default::default();
    let sources = WorldSnapshotSources {
        identity_memo: None,
        effects: None,
        player: None,
        scripts: &scripts,
        animations: &animations,
        collision: &collision,
    };
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    for (index, stage, ticks, rgb) in [
        (0, ScriptStage::Body(Thread::Main), 1, [60, 40, 20]),
        (1, ScriptStage::Body(Thread::Main), 2, [40, 60, -20]),
        (0, ScriptStage::Tasks, 1, [40, 60, -20]),
        (1, ScriptStage::Tasks, 1, [20, 80, -60]),
        (0, ScriptStage::Tasks, 2, [0, 100, -100]),
        (0, ScriptStage::Tasks, 1, [0, 100, -100]),
        (0, ScriptStage::Tasks, 1, [20, 80, -60]),
    ] {
        world
            .advance(
                world.id(index).unwrap(),
                stage,
                ScriptInputs {
                    frame_scale: Some(256),
                    nominal_ticks: Some(ticks),
                    ..Default::default()
                },
                None,
                None,
                20,
            )
            .unwrap();
        let actor = &world.actors()[index];
        let light = actor.light_state().unwrap();
        assert_eq!(light["rgb"], serde_json::json!(rgb));
        assert_eq!(light["enabled"], false);
        assert_eq!(light["locked"], false);
        assert_eq!(actor.runtime().unwrap().active_tasks(), 1);
        let snapshot = world.snapshot(&sources).unwrap();
        world = ScriptWorld::from_snapshot(
            snapshot,
            &sources,
            WorldRestoreContext {
                generation: world.generation() + 1,
                globals: Default::default(),
                samples: &Default::default(),
            },
        )
        .unwrap();
    }
}

#[test]
fn fx_phase_updates_the_authored_light_projection_through_world_dispatch() {
    let mut scripts = phase_scripts([0, 100, -100, 80, 20, 60, 4]);
    scripts.instances.truncate(1);
    let actor = scripts.actors.get_mut("LOGIC").unwrap();
    actor.instances = 1;
    actor.instance_variables.truncate(1);
    scripts.instances[0].object_type = PlacementType::COLOURED_LIGHT;
    scripts.instances[0].map_position = [600, 0, 600];
    scripts.instances[0].light_radius = 100;
    scripts.instances[0].light_intensity = 256;
    scripts.instances[0].light_rgb = [12, -34, 56];
    let (_, mut world, data) = light_maintenance_world(scripts);
    let mut random = cyrus_game::random::EngineRandom::from_state(1);
    light_maintenance_update(&mut world, &data, true, 1, &mut random);
    assert!(
        world.actors()[0].failure().is_none(),
        "{:?}",
        world.actors()[0].failure()
    );
    let lights = world.object_lights();
    assert_eq!(lights.len(), 1);
    assert_eq!(lights[0].position, [600, 0, 600]);
    assert_eq!(lights[0].radius, 100);
    assert_eq!(lights[0].rgb, [60, 40, 20]);
    assert_eq!(
        world.actors()[0].light_state().unwrap()["starting_rgb"],
        serde_json::json!([12, -34, 56])
    );
    light_maintenance_update(&mut world, &data, true, 1, &mut random);
    assert_eq!(world.object_lights()[0].rgb, [40, 60, -20]);
    assert_eq!(random.state(), 1);
}

#[test]
fn fx_phase_zero_span_creates_a_record_and_completes_without_timing() {
    for authored in [false, true] {
        let mut scripts = phase_scripts([1, 2, 3, 4, 5, 6, 0]);
        if authored {
            scripts.instances[0].object_type = PlacementType::COLOURED_LIGHT;
            scripts.instances[0].light_rgb = [9, -7, 5];
        }
        scripts.content_id = scripts.content_identity().unwrap();
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        world.step(world.id(0).unwrap(), Thread::Main, 20).unwrap();
        let actor = &world.actors()[0];
        assert!(actor.light_state().is_some());
        if authored {
            assert_eq!(
                actor.light_state().unwrap()["rgb"],
                serde_json::json!([9, -7, 5])
            );
        }
        assert_eq!(actor.runtime().unwrap().active_tasks(), 1);
        assert_eq!(*world.globals().native_command(), Default::default());
        world.step_tasks(world.id(0).unwrap(), Some(256)).unwrap();
        assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
        if authored {
            assert_eq!(
                world.actors()[0].light_state().unwrap()["rgb"],
                serde_json::json!([9, -7, 5])
            );
        }
    }
}

#[test]
fn fx_phase_keeps_signed_word_channels_and_integer_interpolation() {
    use cyrus_script::script::ScriptInputs;
    for (values, expected) in [
        ([0, 0, 0, 100, -100, 0, 3], [66, -67, 0]),
        (
            [65537, -65537, 32767, -32768, 17, 256, 9],
            [-21846, -7267, 3868],
        ),
    ] {
        let scripts = phase_scripts(values);
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        world
            .advance(
                world.id(0).unwrap(),
                ScriptStage::Body(Thread::Main),
                ScriptInputs {
                    frame_scale: Some(256),
                    nominal_ticks: Some(1),
                    ..Default::default()
                },
                None,
                None,
                20,
            )
            .unwrap();
        assert_eq!(
            world.actors()[0].light_state().unwrap()["rgb"],
            serde_json::json!(expected)
        );
    }
}

#[test]
fn fx_phase_reports_missing_timing_and_signed_division_overflow() {
    use cyrus_script::script::ScriptInputs;
    for (values, ticks, expected) in [
        (
            [0, 0, 0, 80, 20, 60, 4],
            None,
            "FxPhase nominal ticks unavailable",
        ),
        (
            [0, 0, 0, 8_388_608, 0, 0, -1],
            Some(1),
            "FxPhase signed division overflow unsupported",
        ),
    ] {
        let scripts = phase_scripts(values);
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        let id = world.id(0).unwrap();
        let error = world
            .advance(
                id,
                ScriptStage::Body(Thread::Main),
                ScriptInputs {
                    nominal_ticks: ticks,
                    ..Default::default()
                },
                None,
                None,
                20,
            )
            .unwrap_err();
        assert!(format!("{error:#}").contains(expected));
        assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
        assert!(world.actors()[0].light_state().is_some());
        let later = world.step(id, Thread::Main, 20).unwrap_err();
        assert!(format!("{later:#}").contains(expected));
    }
}

fn dim_scripts(kind: PlacementType, amount: i32, base: [i16; 3]) -> Scripts {
    let mut scripts = light_maintenance_scripts(kind, 2, &[(Native::FxFlickerDim, &[amount])]);
    scripts.definition.functions[1].kind = CallKind::Multitask;
    scripts
        .actors
        .get_mut("LOGIC")
        .unwrap()
        .program
        .steps
        .get_mut(&0)
        .unwrap()
        .operation = wait(
        1,
        Native::FxFlickerDim,
        CallKind::Multitask,
        vec![Value::Integer { value: amount }],
    );
    for placement in &mut scripts.instances {
        placement.light_rgb = base;
    }
    scripts.content_id = scripts.content_identity().unwrap();
    scripts
}

#[test]
fn fx_flicker_dim_uses_starting_colour_and_shared_random_after_snapshot() {
    use cyrus_script::script::{ScriptInputs, WorldRestoreContext, WorldSnapshotSources};
    let scripts = dim_scripts(PlacementType::COLOURED_LIGHT, 257, [-10, 300, -32768]);
    let (_, _, animations) = revisit_test_app(&scripts);
    let collision = Default::default();
    let sources = WorldSnapshotSources {
        identity_memo: None,
        effects: None,
        player: None,
        scripts: &scripts,
        animations: &animations,
        collision: &collision,
    };
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let mut random = cyrus_game::random::EngineRandom::from_state(1);
    for (index, stage, rgb, seed) in [
        (
            0,
            ScriptStage::Body(Thread::Main),
            [-207, 103, 32571],
            1103527590,
        ),
        (
            1,
            ScriptStage::Body(Thread::Main),
            [-136, 174, 32642],
            2524885223,
        ),
        (0, ScriptStage::Tasks, [-138, 172, 32640], 662824084),
    ] {
        world
            .advance(
                world.id(index).unwrap(),
                stage,
                ScriptInputs {
                    frame_scale: Some(256),
                    ..Default::default()
                },
                None,
                Some(&mut random),
                20,
            )
            .unwrap();
        let actor = &world.actors()[index];
        let light = actor.light_state().unwrap();
        assert_eq!(light["rgb"], serde_json::json!(rgb));
        assert_eq!(light["starting_rgb"], serde_json::json!([-10, 300, -32768]));
        assert_eq!(light["enabled"], true);
        assert_eq!(light["locked"], true);
        assert_eq!(actor.runtime().unwrap().active_tasks(), 1);
        assert_eq!(random.state(), seed);
        world = ScriptWorld::from_snapshot(
            world.snapshot(&sources).unwrap(),
            &sources,
            WorldRestoreContext {
                generation: world.generation() + 1,
                globals: Default::default(),
                samples: &Default::default(),
            },
        )
        .unwrap();
        random = cyrus_game::random::EngineRandom::from_state(seed);
    }
    let lights = world.object_lights();
    assert_eq!(lights[0].rgb, [-138, 172, 32640]);
    assert_eq!(lights[1].rgb, [-136, 174, 32642]);
    assert_eq!(lights[0].radius, 100);
}

#[test]
fn fx_flicker_dim_matches_original_absolute_bound_word_wrap_and_clamp() {
    use cyrus_script::script::ScriptInputs;
    for (amount, base, expected) in [
        (257, [-10, 300, -32768], [-207, 103, 32571]),
        (-257, [-10, 300, -32768], [-207, 103, 32571]),
        (257, [0, -255, 255], [-197, -255, 58]),
        (1, [32767, -32768, -32767], [32767, -255, -255]),
    ] {
        let scripts = dim_scripts(PlacementType::COLOURED_LIGHT, amount, base);
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        let mut random = cyrus_game::random::EngineRandom::from_state(1);
        world
            .advance(
                world.id(0).unwrap(),
                ScriptStage::Body(Thread::Main),
                ScriptInputs::default(),
                None,
                Some(&mut random),
                20,
            )
            .unwrap();
        assert_eq!(world.object_lights()[0].rgb, expected);
        assert_eq!(random.state(), 1103527590);
    }
}

#[test]
fn fx_flicker_dim_rejects_unknown_base_and_nonreturning_bounds_without_random_draws() {
    use cyrus_script::script::ScriptInputs;
    for (kind, amount, message) in [
        (PlacementType::default(), 257, "starting RGB unavailable"),
        (PlacementType::LIGHT, 257, "starting RGB unavailable"),
        (
            PlacementType::COLOURED_LIGHT,
            0,
            "nonzero representable absolute bound",
        ),
        (
            PlacementType::COLOURED_LIGHT,
            i32::MIN,
            "nonzero representable absolute bound",
        ),
    ] {
        let scripts = dim_scripts(kind, amount, [10, 20, 30]);
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        let id = world.id(0).unwrap();
        let mut random = cyrus_game::random::EngineRandom::from_state(1);
        let error = world
            .advance(
                id,
                ScriptStage::Body(Thread::Main),
                ScriptInputs::default(),
                None,
                Some(&mut random),
                20,
            )
            .unwrap_err();
        assert!(format!("{error:#}").contains(message));
        assert_eq!(random.state(), 1);
        assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
        let error = world.step(id, Thread::Main, 20).unwrap_err();
        assert!(format!("{error:#}").contains(message));
    }
    let scripts = dim_scripts(PlacementType::COLOURED_LIGHT, 257, [10, 20, 30]);
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let error = world
        .step(world.id(0).unwrap(), Thread::Main, 20)
        .unwrap_err();
    assert!(format!("{error:#}").contains("shared random unavailable"));
    assert_eq!(
        world.actors()[0].light_state().unwrap()["rgb"],
        serde_json::json!([10, 20, 30])
    );
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
}

#[test]
fn fx_flicker_dim_restore_validates_base_source_and_does_not_guess_legacy_colour() {
    use cyrus_script::script::{ScriptInputs, WorldRestoreContext, WorldSnapshotSources};
    let scripts = dim_scripts(PlacementType::COLOURED_LIGHT, 257, [-10, 300, -32768]);
    let (_, _, animations) = revisit_test_app(&scripts);
    let collision = Default::default();
    let sources = WorldSnapshotSources {
        identity_memo: None,
        effects: None,
        player: None,
        scripts: &scripts,
        animations: &animations,
        collision: &collision,
    };
    let world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let snapshot = serde_json::to_value(world.snapshot(&sources).unwrap()).unwrap();
    let samples = Default::default();
    let context = || WorldRestoreContext {
        generation: 2,
        globals: Default::default(),
        samples: &samples,
    };
    let mut wrong = snapshot.clone();
    wrong["actors"][0]["light"]["starting_rgb"] = serde_json::json!([1, 2, 3]);
    let error =
        ScriptWorld::from_snapshot(serde_json::from_value(wrong).unwrap(), &sources, context())
            .err()
            .unwrap();
    assert!(format!("{error:#}").contains("starting RGB source differs"));
    let mut legacy = snapshot;
    legacy["actors"][0]["light"]
        .as_object_mut()
        .unwrap()
        .remove("starting_rgb");
    let mut world =
        ScriptWorld::from_snapshot(serde_json::from_value(legacy).unwrap(), &sources, context())
            .unwrap();
    let mut random = cyrus_game::random::EngineRandom::from_state(1);
    let error = world
        .advance(
            world.id(0).unwrap(),
            ScriptStage::Body(Thread::Main),
            ScriptInputs::default(),
            None,
            Some(&mut random),
            20,
        )
        .unwrap_err();
    assert!(format!("{error:#}").contains("starting RGB unavailable"));
    assert_eq!(random.state(), 1);
}
