use super::*;

#[test]
fn flat_controls_keep_a_per_actor_record_and_reject_uninstalled_textures() {
    let flat =
        |calls: &[(Native, &[i32])]| run_native_world(calls).unwrap().actors()[0].flat_state();
    assert_eq!(
        flat(&[(Native::FlatOff, &[])]),
        None,
        "no record is created"
    );
    let shown = flat(&[(Native::Flat, &[300]), (Native::FlatOffset, &[1, -2, 3])]).unwrap();
    assert_eq!(shown["enabled"], true);
    assert_eq!(shown["texture"], 300);
    assert_eq!(shown["offset"], serde_json::json!([1, 65534, 3]));
    let hidden = flat(&[(Native::Flat, &[300]), (Native::FlatOff, &[])]).unwrap();
    assert_eq!(hidden["enabled"], false);
    assert_eq!(
        flat(&[(Native::FlatLikeStatic, &[300])]).unwrap()["frame"],
        65535
    );
    let error = run_native_world(&[(Native::FlatAnimate, &[(1 << 7) | 2, 0, 0])])
        .err()
        .unwrap();
    assert!(format!("{error:#}").contains("not installed"), "{error:#}");
    assert!(run_native_world(&[(Native::FlatOffset, &[1])]).is_err());
}

#[test]
fn type_three_placement_starts_with_an_enabled_flat_record_of_its_texture() {
    let mut scripts = random_scripts(Native::Spare0Parm0, vec![], Object::Me);
    scripts.definition.functions[1].param_count = 0;
    scripts.instances.truncate(2);
    scripts.instances[0].object_type = PlacementType::FLAT;
    scripts.instances[0].texture = -20351;
    scripts.content_id = scripts.content_identity().unwrap();
    let world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let flat = world.actors()[0].flat_state().unwrap();
    assert_eq!(flat["enabled"], true);
    assert_eq!(flat["texture"], 0xb081);
    assert_eq!(flat["offset"], serde_json::json!([0, 0, 0]));
    assert!(world.actors()[1].flat_state().is_none());
}

#[test]
fn actor_flat_draws_place_enabled_records_at_the_pose_plus_the_rotated_offset() {
    let mut scripts = random_scripts(Native::Spare0Parm0, vec![], Object::Me);
    scripts.definition.functions[1].param_count = 0;
    scripts.instances.truncate(2);
    scripts.instances[0].object_type = PlacementType::FLAT;
    scripts.instances[0].texture = -20351;
    scripts.instances[0].map_position = [100, -200, 300];
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world.prepare_spatial().unwrap();
    let pose = world.spatial(world.id(0).unwrap()).unwrap().local;
    let draws = world.actor_flat_draws().unwrap();
    assert_eq!(draws.len(), 1);
    // The placement record starts static (frame word zero, no offset): explicit frame zero.
    assert_eq!(draws[0].descriptor, 0xb081 | 0x20000);
    assert_eq!(draws[0].frame, 0);
    assert_eq!(draws[0].position, pose.position.map(|axis| axis >> 8));
    assert!(world.actors()[1].flat_state().is_none());
}

#[test]
fn ordinary_flat_script_creation_animation_and_disable_wait_for_next_maintenance() {
    let mut scripts = light_maintenance_scripts(
        PlacementType::LIGHT,
        1,
        &[(Native::FlatAnimate, &[129, 1, 1])],
    );
    scripts.texture_frames.insert(1, BTreeMap::from([(1, 4)]));
    let (_, mut world, data) = light_maintenance_world(scripts);
    let mut random = cyrus_game::random::EngineRandom::default();
    light_maintenance_update(&mut world, &data, false, 1, &mut random);
    let actor = &world.actors()[0];
    assert!(actor.failure().is_none(), "{:?}", actor.failure());
    assert_eq!(actor.flat_state().unwrap()["frame"], 0);
    light_maintenance_update(&mut world, &data, false, 1, &mut random);
    assert_eq!(world.actors()[0].flat_state().unwrap()["frame"], 1);

    let scripts = light_maintenance_scripts(
        PlacementType::LIGHT,
        1,
        &[(Native::Flat, &[129]), (Native::FlatOff, &[])],
    );
    let (_, mut world, data) = light_maintenance_world(scripts);
    light_maintenance_update(&mut world, &data, false, 33, &mut random);
    let actor = &world.actors()[0];
    assert!(actor.failure().is_none(), "{:?}", actor.failure());
    assert_eq!(actor.flat_state().unwrap()["enabled"], false);
    assert_eq!(actor.flat_state().unwrap()["pending"], 0);
    light_maintenance_update(&mut world, &data, false, 33, &mut random);
    assert!(world.actors()[0].flat_state().is_none());
}
