//! PlayerTilt runs through routed VM calls and persistent world task slots.
use super::*;
use cyrus_assets::script::PlacementType;
use cyrus_game::script::{
    ModelSupportOwners, ScriptInputs, WorldRestoreContext, WorldSnapshotSources,
};

fn tilt_fixture(
    arguments: [i32; 3],
    angles: [u16; 3],
    associated: bool,
    target: Object,
) -> (ScriptWorld, Scripts, cyrus_assets::animation::Animations) {
    tilt_fixture_for(Native::PlayerTilt, arguments, angles, associated, target)
}

fn tilt_fixture_for(
    native: Native,
    arguments: [i32; 3],
    angles: [u16; 3],
    associated: bool,
    target: Object,
) -> (ScriptWorld, Scripts, cyrus_assets::animation::Animations) {
    let call = Call {
        function_id: 4,
        function: native,
        kind: CallKind::Multitask,
        target: target.clone(),
        arguments: arguments
            .into_iter()
            .map(|value| Value::Integer { value })
            .collect(),
    };
    let operation = if matches!(target, Object::Me) {
        Operation::Call { call }
    } else {
        Operation::ObjectCall { call }
    };
    let mut scripts = wait_cache(vec![
        operation,
        set_flag(91),
        Operation::Yield,
        Operation::End { target: 0 },
    ]);
    scripts.definition.functions.push(Function {
        name: native,
        param_count: 3,
        kind: CallKind::Multitask,
    });
    add_standalone_receivers(&mut scripts);
    scripts.actors.get_mut("LOGIC").unwrap().program.strings = vec!["logic".into()];
    for instance in &mut scripts.instances {
        instance.object_type = PlacementType::MODEL;
    }
    scripts.instances[0].angles = angles.map(u32::from);
    scripts.instances[0].map_position = [0; 3];
    scripts.instances[0].object_type = PlacementType::MODEL;
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
    world.prepare_spatial().unwrap();
    let player = world.id(2).unwrap();
    world
        .initialize_player_spatial(player, [0, 0, -1536], [0; 3])
        .unwrap();
    let data = cyrus_assets::animation::Animations {
        version: 2,
        world: scripts.world,
        metadata: serde_json::json!({"script_content_id": scripts.content_id,
            "actors": scripts.actors.keys().map(|name| serde_json::json!({"script_name":name,"model_load_path":"none","mpsz_bounds_primary":-1})).collect::<Vec<_>>() }),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    };
    let models =
        cyrus_collision::actor_models::ActorModelCollision::new(&Default::default()).unwrap();
    let scene = ordinary_collision_scene(world.world, models);
    world
        .prepare_ordinary_collision_shape(world.id(0).unwrap(), &data, &scene)
        .unwrap();
    if associated {
        world
            .set_model_support(
                player,
                ModelSupportOwners {
                    actor: Some(world.id(0).unwrap()),
                    static_placement: None,
                },
            )
            .unwrap();
    }
    (world, scripts, data)
}

fn tilt_step(
    world: &mut ScriptWorld,
    stage: ScriptStage,
    ticks: Option<u32>,
) -> anyhow::Result<()> {
    world.advance(
        world.id(0).unwrap(),
        stage,
        ScriptInputs {
            nominal_ticks: ticks,
            frame_scale: Some(256),
            ..Default::default()
        },
        None,
        None,
        20,
    )
}

#[test]
fn player_tilt_public_call_scales_total_extent_and_persists_after_caller_continues() {
    for (args, expected) in [
        ([0, 256, 0], [1794, 0, 0]),
        ([512, 256, 0], [0, 0, 254]),
        ([1024, 256, 0], [254, 0, 0]),
        ([1536, 256, 0], [0, 0, 1794]),
        ([512, 512, 0], [0, 0, 508]),
        ([512, -256, 0], [0, 0, 1794]),
        ([512, 256, -4], [0, 0, 1]),
        ([512, i32::MAX, 0], [0, 0, 2046]),
        ([512, 256, 4], [0, 0, 63]),
    ] {
        let (mut world, _, _) = tilt_fixture(args, [0; 3], true, Object::Me);
        tilt_step(&mut world, ScriptStage::Body(Thread::Main), Some(1)).unwrap();
        assert_eq!(
            world.spatial(world.id(0).unwrap()).unwrap().local.angles,
            expected,
            "{args:?}"
        );
        assert_eq!(world.flags()[0], 91);
        assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 1);
        world.flush_map_updates([0; 3], false).unwrap();
    }
}

#[test]
fn player_tilt_unassociated_relaxes_signed_axes_and_uses_whole_ticks() {
    for (angles, divisor, ticks, expected) in [
        ([100, 37, 2040], 4, 0, [0, 37, 2040]),
        ([100, 37, 2040], 4, 1, [0, 37, 2042]),
        ([100, 37, 2040], 4, 2, [0, 37, 2044]),
        ([1024, 37, 1024], 4, 1, [0, 37, 0]),
        ([2040, 37, 2040], 0, 1, [0, 37, 0]),
    ] {
        let (mut world, _, _) = tilt_fixture([0, 256, divisor], angles, false, Object::Me);
        tilt_step(&mut world, ScriptStage::Body(Thread::Main), Some(ticks)).unwrap();
        assert_eq!(
            world.spatial(world.id(0).unwrap()).unwrap().local.angles,
            expected
        );
    }
    // Equal targets do not request the otherwise unsupported alternate matrix service.
    let (world, scripts, data) = tilt_fixture([0, 256, 4], [0; 3], false, Object::Me);
    let mut world = tilt_restore_changed(&world, &scripts, &data, |saved| {
        saved["spatial"]["actors"][0]["rotation_mode"] = 1.into();
    });
    let before = world.inspect(0, 1)["actors"][0]["spatial"].clone();
    tilt_step(&mut world, ScriptStage::Body(Thread::Main), Some(0)).unwrap();
    assert_eq!(world.inspect(0, 1)["actors"][0]["spatial"], before);
}

#[test]
fn player_tilt_retains_task_and_rebinds_live_support_on_restore() {
    let (mut world, scripts, data) = tilt_fixture([512, 256, 4], [0; 3], true, Object::Me);
    tilt_step(&mut world, ScriptStage::Body(Thread::Main), Some(1)).unwrap();
    let collision = Default::default();
    let sources = WorldSnapshotSources {
        identity_memo: None,
        effects: None,
        player: None,
        scripts: &scripts,
        animations: &data,
        collision: &collision,
    };
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
    tilt_step(&mut world, ScriptStage::Tasks, Some(2)).unwrap();
    assert_eq!(
        world.spatial(world.id(0).unwrap()).unwrap().local.angles,
        [0, 0, 157]
    );
    world
        .set_model_support(world.id(2).unwrap(), ModelSupportOwners::default())
        .unwrap();
    tilt_step(&mut world, ScriptStage::Tasks, Some(1)).unwrap();
    assert_eq!(
        world.spatial(world.id(0).unwrap()).unwrap().local.angles,
        [0; 3]
    );
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 1);
}

#[test]
fn player_tilt_failures_preserve_caller_and_pose() {
    for missing_shape in [false, true] {
        let (world, scripts, data) = tilt_fixture([512, 256, 4], [0; 3], true, Object::Me);
        let mut world = tilt_restore_changed(&world, &scripts, &data, |saved| {
            if missing_shape {
                saved["actors"][0]["collision"] = serde_json::Value::Null;
            }
        });
        world
            .set_actor_attribute(0, Attribute::Slave as usize, 1)
            .unwrap();
        let before = world.inspect(0, 10);
        let error = tilt_step(&mut world, ScriptStage::Body(Thread::Main), Some(1))
            .unwrap_err()
            .to_string();
        assert!(error.contains("original diagnostic 137"), "{error}");
        let after = world.inspect(0, 10);
        assert_eq!(
            after["actors"][0]["spatial"],
            before["actors"][0]["spatial"]
        );
        assert_eq!(after["actors"][0]["main"], before["actors"][0]["main"]);
        assert_eq!(world.flags()[0], 12);
        assert_eq!(
            tilt_step(&mut world, ScriptStage::Body(Thread::Main), Some(1))
                .unwrap_err()
                .to_string(),
            error
        );
    }
}

fn tilt_restore_changed(
    world: &ScriptWorld,
    scripts: &Scripts,
    data: &cyrus_assets::animation::Animations,
    edit: impl FnOnce(&mut serde_json::Value),
) -> ScriptWorld {
    let collision = Default::default();
    let sources = WorldSnapshotSources {
        identity_memo: None,
        effects: None,
        player: None,
        scripts,
        animations: data,
        collision: &collision,
    };
    let mut saved = serde_json::to_value(world.snapshot(&sources).unwrap()).unwrap();
    edit(&mut saved);
    ScriptWorld::from_snapshot(
        serde_json::from_value(saved).unwrap(),
        &sources,
        WorldRestoreContext {
            generation: world.generation() + 1,
            globals: Default::default(),
            samples: &Default::default(),
        },
    )
    .unwrap()
}

#[test]
fn player_tilt_linked_bearing_is_independent_of_local_strength_distance() {
    let (world, scripts, data) = tilt_fixture([0, 256, 0], [0; 3], true, Object::Me);
    let mut world = tilt_restore_changed(&world, &scripts, &data, |saved| {
        saved["spatial"]["actors"][0]["linked_position"] = serde_json::json!([1536, 0, -1536]);
    });
    tilt_step(&mut world, ScriptStage::Body(Thread::Main), Some(1)).unwrap();
    let spatial = world.spatial(world.id(0).unwrap()).unwrap();
    assert_eq!(spatial.local.angles, [0, 0, 254]);
    assert_eq!(spatial.local.position, [0; 3]);
    assert_eq!(spatial.linked_position(), Some([1536, 0, -1536]));
    assert_eq!(
        world.inspect(0, 1)["actors"][0]["spatial"]["angle_history"],
        serde_json::json!([0, 0, 254])
    );
}

#[test]
fn player_tilt_division_fault_does_not_publish_partial_angles() {
    let (world, scripts, data) = tilt_fixture([512, 256, 4], [0; 3], true, Object::Me);
    let mut world = tilt_restore_changed(&world, &scripts, &data, |saved| {
        saved["actors"][0]["collision"]["positive_extent"] = serde_json::json!([0, 0, 0]);
        saved["actors"][0]["collision"]["negative_extent"] = serde_json::json!([0, 0, 0]);
    });
    let before = world.inspect(0, 10)["actors"][0]["spatial"].clone();
    let error = tilt_step(&mut world, ScriptStage::Body(Thread::Main), Some(1)).unwrap_err();
    assert!(
        format!("{error:#}").contains("original division fault"),
        "{error:#}"
    );
    assert_eq!(world.inspect(0, 10)["actors"][0]["spatial"], before);
    assert_eq!(world.flags()[0], 12);
}

#[test]
fn player_tilt_routed_call_changes_receiver_and_leaves_callers_pose() {
    let (mut world, _, _) = tilt_fixture([512, 256, 4], [0; 3], true, Object::Named { string: 0 });
    world
        .advance(
            world.id(1).unwrap(),
            ScriptStage::Body(Thread::Main),
            ScriptInputs {
                nominal_ticks: Some(1),
                frame_scale: Some(256),
                ..Default::default()
            },
            None,
            None,
            20,
        )
        .unwrap();
    assert_eq!(
        world.spatial(world.id(0).unwrap()).unwrap().local.angles,
        [0, 0, 63]
    );
    assert_eq!(
        world.spatial(world.id(1).unwrap()).unwrap().local.angles,
        [0; 3]
    );
    world
        .advance(
            world.id(0).unwrap(),
            ScriptStage::Tasks,
            ScriptInputs {
                nominal_ticks: Some(1),
                frame_scale: Some(256),
                ..Default::default()
            },
            None,
            None,
            20,
        )
        .unwrap();
    assert_eq!(
        world.spatial(world.id(0).unwrap()).unwrap().local.angles,
        [0, 0, 110]
    );
    assert_eq!(
        world.spatial(world.id(1).unwrap()).unwrap().local.angles,
        [0; 3]
    );
}

#[test]
fn player_tilt_alternate_rotation_matches_original_numerical_matrix() {
    let (world, scripts, data) = tilt_fixture([512, 256, 4], [0; 3], true, Object::Me);
    let mut world = tilt_restore_changed(&world, &scripts, &data, |saved| {
        saved["spatial"]["actors"][0]["rotation_mode"] = 1.into();
    });
    tilt_step(&mut world, ScriptStage::Body(Thread::Main), Some(1)).unwrap();
    let pose = world.spatial(world.id(0).unwrap()).unwrap().local;
    assert_eq!(pose.angles, [0, 0, 63]);
    // Verified original numerical builder with the initialized rotation table.
    assert_eq!(
        pose.matrix
            .0
            .into_iter()
            .flatten()
            .map(f32::to_bits)
            .collect::<Vec<_>>(),
        vec![
            1065040811, 3192172729, 0, 1044689081, 1065040811, 2147483648, 0, 0, 1065353216
        ]
    );
}

#[test]
fn player_tilt_retains_unmasked_angle_sums_and_history_across_restore() {
    let (mut world, scripts, data) = tilt_fixture([512, 256, 4], [0, 0, 2047], true, Object::Me);
    tilt_step(&mut world, ScriptStage::Body(Thread::Main), Some(1)).unwrap();
    assert_eq!(
        world.spatial(world.id(0).unwrap()).unwrap().local.angles,
        [0, 0, 2110]
    );
    assert_eq!(
        world.inspect(0, 1)["actors"][0]["spatial"]["angle_history"],
        serde_json::json!([0, 0, 2110])
    );
    assert_eq!(world.flags()[0], 91);
    let mut world = tilt_restore_changed(&world, &scripts, &data, |_| {});
    assert_eq!(
        world.spatial(world.id(0).unwrap()).unwrap().local.angles,
        [0, 0, 2110]
    );
    tilt_step(&mut world, ScriptStage::Tasks, Some(1)).unwrap();
    assert_eq!(
        world.spatial(world.id(0).unwrap()).unwrap().local.angles,
        [0, 0, 2158]
    );
    assert_eq!(
        world
            .spatial(world.id(0).unwrap())
            .unwrap()
            .local
            .matrix
            .0
            .map(|row| row.map(f32::to_bits)),
        cyrus_world::transform::Matrix::orientation([0, 0, 110])
            .0
            .map(|row| row.map(f32::to_bits))
    );
}

#[test]
fn player_tilt_signed_storage_matches_original_full_width_result_stores() {
    // Literal results from original instruction execution with the public fixture's geometry.
    for (initial, expected) in [
        (i32::MIN, -2147483585),
        (-4097, -4034),
        (-1, 62),
        (2047, 2110),
        (i32::MAX, -2147483586),
    ] {
        let (world, scripts, data) = tilt_fixture([512, 256, 4], [0; 3], true, Object::Me);
        let mut world = tilt_restore_changed(&world, &scripts, &data, |saved| {
            saved["spatial"]["actors"][0]["local"]["angles"] = serde_json::json!([0, 0, initial]);
        });
        tilt_step(&mut world, ScriptStage::Body(Thread::Main), Some(1)).unwrap();
        assert_eq!(
            world.spatial(world.id(0).unwrap()).unwrap().local.angles,
            [0, 0, expected]
        );
        assert_eq!(
            world.inspect(0, 1)["actors"][0]["spatial"]["angle_history"],
            serde_json::json!([0, 0, expected])
        );
        assert_eq!(world.flags()[0], 91);
        let world = tilt_restore_changed(&world, &scripts, &data, |_| {});
        assert_eq!(
            world.spatial(world.id(0).unwrap()).unwrap().local.angles,
            [0, 0, expected]
        );
    }
}

/// `PlayerTiltRevx` uses the bearing and angle without the receiver heading and the negated
/// `PlayerTilt` targets (pitch `512 - a` / `a - 1536`, roll `-a` / `a - 1024` / `2048 - a`).
#[test]
fn player_tilt_revx_public_call_negates_the_targets_and_ignores_the_receiver_heading() {
    for (angle, expected) in [
        (0, [254, 0, 0]),
        (512, [0, 0, 1794]),
        (1024, [1794, 0, 0]),
        (1536, [0, 0, 254]),
    ] {
        for heading in [0, 300] {
            let (mut world, _, _) = tilt_fixture_for(
                Native::PlayerTiltRevx,
                [angle, 256, 0],
                [0, heading, 0],
                true,
                Object::Me,
            );
            tilt_step(&mut world, ScriptStage::Body(Thread::Main), Some(1)).unwrap();
            let angles = world.spatial(world.id(0).unwrap()).unwrap().local.angles;
            assert_eq!(
                [angles[0], angles[2]],
                [expected[0], expected[2]],
                "angle {angle} heading {heading}"
            );
            assert_eq!(world.flags()[0], 91, "the caller continues");
            assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 1);
        }
    }
}

#[test]
fn player_tilt_revx_unassociated_receiver_relaxes_to_zero_like_player_tilt() {
    let run = |native| {
        let (mut world, _, _) =
            tilt_fixture_for(native, [0, 256, 4], [100, 37, 2040], false, Object::Me);
        tilt_step(&mut world, ScriptStage::Body(Thread::Main), Some(1)).unwrap();
        world.spatial(world.id(0).unwrap()).unwrap().local.angles
    };
    assert_eq!(run(Native::PlayerTiltRevx), run(Native::PlayerTilt));
}
