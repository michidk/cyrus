//! A rope's terminal model is a transient collision candidate (preservation: live and transient
//! collision candidates; MPRP rope instancing): type 0x80 face candidate at the terminal's swung
//! pose, with zero collision metadata and no actor or static owner.
use super::*;
use cyrus_collision::actor_models::ActorModelCollision;
use cyrus_collision::candidates::{CandidateOwner, ModelInstance, transient_model};

fn rope_world(
    ropes: serde_json::Value,
    terminal_geometry: bool,
) -> (
    ScriptWorld,
    cyrus_assets::animation::Animations,
    cyrus_script::player::data::PlayerScene,
) {
    use cyrus_assets::collision::{ActorCollisionModels, CollisionGeometry, CollisionSubobject};
    let mut scripts = cache();
    scripts.updates.ropes_load = Some(true);
    add_standalone_receivers(&mut scripts);
    let (mut world, mut data, _) = static_model_fixture_at(scripts, None, [10, 20, 65535]);
    data.metadata["ropes"] = ropes;
    world.prepare_ropes(&data).unwrap();
    let geometry = CollisionGeometry {
        version: Some(*b"v5.0"),
        radius: 30.0,
        vertices: vec![[-20.0, 0.0, -20.0], [20.0, 0.0, -20.0], [0.0, 0.0, 20.0]],
        normals: vec![[0.0, -1.0, 0.0]],
        faces: vec![vec![0, 1, 2]],
        textures: Some(vec![None]),
        subobjects: vec![CollisionSubobject {
            center: [0.0; 3],
            radius: 30.0,
            extents: Some([1.0; 3]),
            faces: vec![0],
        }],
    };
    let mut source = ActorCollisionModels {
        object_count: 2,
        models: BTreeMap::from([("STATIC".into(), clone_geometry(&geometry))]),
        instances: BTreeMap::from([(0, "STATIC".into()), (1, "STATIC".into())]),
        collision_flags: BTreeMap::from([(0, 6), (1, 7)]),
        ..Default::default()
    };
    if terminal_geometry {
        source.models.insert("TERMINAL".into(), geometry);
        source.transients.insert(0, "TERMINAL".into());
    }
    let scene = ordinary_collision_scene(world.world, ActorModelCollision::new(&source).unwrap());
    (world, data, scene)
}

fn clone_geometry(
    geometry: &cyrus_assets::collision::CollisionGeometry,
) -> cyrus_assets::collision::CollisionGeometry {
    serde_json::from_value(serde_json::to_value(geometry).unwrap()).unwrap()
}

fn rope(static_model: &str) -> serde_json::Value {
    serde_json::json!([{
        "position": cyrus_game::rope::world_position([0.0, 0.0, 65535.0]),
        "angle_y": 0, "rope_type": 0, "swing": 0, "speed": 0,
        "length": 3, "static_model": static_model, "rope_model": "ROPELINK",
        "unknown1": [0, 0, 0],
    }])
}

#[test]
fn a_rope_terminal_is_a_transient_candidate_one_link_below_the_last_link() {
    let (mut world, data, scene) = rope_world(rope("TERMINAL"), true);
    let records = world.transient_collision_models(&scene).unwrap();
    assert_eq!(records.len(), 1);
    assert!(records[0].active && records[0].flags & 0x10 != 0);
    let bounds = records[0].model.unwrap();
    assert_eq!(bounds.instance, ModelInstance::Runtime(transient_model(0)));
    assert_eq!(bounds.radius, 30.0);
    // Three 0.8 world-unit links hang from the base, then the terminal one link below the last:
    // 0.8 * 3 = 2.4 world units, 20 map units per world unit.
    assert!(bounds.position[0].abs() < 1e-3, "{:?}", bounds.position);
    assert!(
        (bounds.position[1] - 48.0).abs() < 1e-2,
        "{:?}",
        bounds.position
    );

    // The ordinary collision query of an actor near it retains that candidate in its face list.
    let id = world.id(0).unwrap();
    let query = world
        .prepare_ordinary_collision_query(id, &scene, &data, [10, 65535], &[])
        .unwrap();
    let terminals: Vec<_> = query
        .candidates()
        .faces
        .iter()
        .filter(|face| face.owner == CandidateOwner::Transient)
        .collect();
    assert_eq!(terminals.len(), 1);
    assert_eq!(
        (terminals[0].candidate_type, terminals[0].collision_flags),
        (0x80, 0)
    );
}

#[test]
fn ordinary_rope_collision_uses_the_published_camera_and_the_strict_base_distance() {
    use cyrus_script::script::camera::CameraPose;

    for (distance, loaded, expected) in [(4096 * 256 - 1, true, true), (4096 * 256, false, false)] {
        let (mut world, data, scene) = rope_world(rope("TERMINAL"), loaded);
        let view = [0, 65535];
        // A load-time selection must not prevent the first live selection in this cell.
        world.ordinary_actors(view).unwrap();
        world
            .initialize_camera_pose(CameraPose {
                position: [0, -distance, 65535 * 256],
                angles: [0; 3],
            })
            .unwrap();
        world.ordinary_actors(view).unwrap();

        let query = world
            .prepare_ordinary_collision_query(world.id(0).unwrap(), &scene, &data, view, &[])
            .unwrap();
        assert_eq!(
            query
                .candidates()
                .faces
                .iter()
                .any(|face| face.owner == CandidateOwner::Transient),
            expected,
            "camera-to-base distance {distance} Q8"
        );
    }
}

#[test]
fn rope_admission_is_retained_within_a_window_and_rebuilt_on_exit_and_return() {
    use cyrus_script::script::camera::CameraPose;

    let (mut world, data, scene) = rope_world(rope("TERMINAL"), true);
    let base = [0, 0, 65535 * 256];
    world
        .initialize_player_spatial(world.selected_player().unwrap(), base, [0; 3])
        .unwrap();
    world
        .initialize_camera_pose(CameraPose {
            position: base,
            angles: [0; 3],
        })
        .unwrap();
    let mut placement = world.selected_player_placement().unwrap();

    for (x, y, expected) in [
        (0, 0, true),
        (0, -4096, true),
        (5120, 0, false),
        (0, 0, true),
    ] {
        let view = [x, 65535];
        placement.camera_position = [x * 256, y * 256, base[2]];
        world.restore_selected_player_placement(&placement).unwrap();
        world.ordinary_actors(view).unwrap();
        let query = world
            .prepare_ordinary_collision_query(world.id(0).unwrap(), &scene, &data, view, &[])
            .unwrap();
        assert_eq!(
            query
                .candidates()
                .faces
                .iter()
                .any(|face| face.owner == CandidateOwner::Transient),
            expected,
            "camera at ({x}, {y})"
        );
    }
}

#[test]
fn a_rope_without_a_terminal_model_has_no_transient_candidate() {
    let (world, _, scene) = rope_world(rope(""), true);
    assert!(world.transient_collision_models(&scene).unwrap().is_empty());
}

#[test]
fn a_terminal_without_exported_geometry_asks_for_a_re_export() {
    let (world, _, scene) = rope_world(rope("TERMINAL"), false);
    let error = world.transient_collision_models(&scene).unwrap_err();
    assert!(format!("{error:#}").contains("re-export"), "{error:#}");
    let mut scene = scene;
    scene.actor_collision = None;
    let error = world.transient_collision_models(&scene).unwrap_err();
    assert!(format!("{error:#}").contains("re-export"), "{error:#}");
}

#[test]
fn a_weapon_tip_through_a_rope_terminal_clanks_once_like_any_unowned_model() {
    use cyrus_assets::collision::{ActorCollisionModels, CollisionScene};
    use cyrus_script::script::{
        CombatControlState, LocomotionGeometry, OrdinaryGroundQuery, ScriptInputs,
    };
    // Preservation SOUP.md "Unowned static weapon contact": a face candidate without an actor
    // owner takes the sword clank (one cue unless its type is exactly one), keeps attack intent
    // and continues. The rope's terminal is such a candidate (type 0x80).
    let rope = serde_json::json!([{
        "position": [-51.2, 0.0, -3225.6], "angle_y": 0, "rope_type": 0, "swing": 0, "speed": 0,
        "length": 1, "static_model": "TERMINAL", "rope_model": "ROPELINK", "unknown1": [0, 0, 0],
    }]);
    let record = cyrus_game::rope::RopeRecord::from_metadata(&rope[0]).unwrap();
    let (terminal, _) = cyrus_game::rope::Rope::new(&record, 0.8)
        .end_collision_pose()
        .unwrap();
    let centre = terminal.map(|value| value.round() as i32);
    for (tip_x, base_x, expected_cues) in [(-10, 10, 1), (10, -10, 0)] {
        let (world, scripts, mut data) = movement_fixture(
            18,
            false,
            true,
            |scripts| {
                for instance in &mut scripts.instances {
                    instance.object_type = PlacementType::MODEL;
                }
                scripts.updates.ropes_load = Some(true);
                scripts.combat_clank_effects = [Some(31), None];
                scripts.combat_attacks =
                    vec![combat::Attack {
                        is_combo: false,
                        motions: vec![serde_json::from_value(serde_json::json!({
                        "section":0,"kind":0,"animation_group":18,"min_range":0,"max_range":100,
                        "first_collide_frame":0,"collide_duration":1
                    }))
                    .unwrap()],
                        min_range: 0,
                        max_range: 100,
                    }];
            },
            |saved| {
                saved["actors"][0]["control"]["death_selector"] = 0.into();
                saved["actors"][0]["control"]["combat"] =
                    serde_json::to_value(CombatControlState {
                        action: 0,
                        flags: CombatFlags::from_bits(1),
                        attack_indices: [Some(0); 9],
                        retained_action_indices: [-1, 0],
                        ..Default::default()
                    })
                    .unwrap();
            },
        );
        data.metadata["actors"][0]["model_load_path"] = "none".into();
        data.metadata["actors"][0]["ravc_count"] = 0.into();
        data.metadata["ropes"] = rope.clone();
        data.models.insert(
            "WEAPON".into(),
            cyrus_assets::animation::AnimationModel {
                radius: Some(1.),
                base_positions: vec![],
                base_normals: vec![],
                faces: vec![],
                corner_normals: vec![],
                encoding: "animated_i16".into(),
                positions: vec![vec![]],
                normals: vec![vec![]],
            },
        );
        let mut saved = movement_snapshot(&world, &scripts, &data);
        saved["spatial"]["actors"][0]["local"]["position"] = serde_json::json!([
            i64::from(centre[0]) * 256,
            i64::from(centre[1]) * 256,
            i64::from(centre[2]) * 256
        ]);
        saved["actors"][0]["model"]["weapon"] = serde_json::json!({
            "item":1,"model":"WEAPON","position_bits":[0,0,0],
            "matrix_bits":[[1065353216_u32,0,0],[0,1065353216_u32,0],[0,0,1065353216_u32]],
            "retained_point":[0,0,0],"retained_angles":[0,0,0],
            "endpoints":[
                [centre[0] + base_x, centre[1], centre[2]],
                [centre[0] + tip_x, centre[1], centre[2]]
            ]
        });
        saved["ropes"] = serde_json::json!([{
            "amplitude": 0, "phase": 0, "step": 0, "push": 0, "direction": 0,
            "previous_sum": 0, "attachments": [],
        }]);
        let mut world = restore_navigation_owner(saved, &scripts, &data).unwrap();
        let id = world.id(0).unwrap();
        let (_, _, statics) = static_model_fixture(cache());
        let mut scene = ordinary_collision_scene(world.world, statics);
        let mut source = ActorCollisionModels {
            object_count: 2,
            ..Default::default()
        };
        source
            .models
            .insert("TERMINAL".into(), ordinary_forward_wall_model().geometry);
        source.transients.insert(0, "TERMINAL".into());
        scene.actor_collision = Some(ActorModelCollision::new(&source).unwrap());
        scene.collision = CollisionScene {
            sphere_object_scale: Some(256),
            ..Default::default()
        };
        let prepared = world
            .prepare_ordinary_collision_query(id, &scene, &data, [1024, 1024], &[])
            .unwrap();
        assert!(
            prepared
                .candidates()
                .faces
                .iter()
                .any(|face| face.owner == CandidateOwner::Transient),
            "the terminal must be a candidate of the attacker"
        );
        let mut random = cyrus_game::random::EngineRandom::default();
        let before = random.state();
        let mut cues = Vec::new();
        world
            .maintain_map_combat(
                id,
                1,
                false,
                ScriptInputs {
                    locomotion_geometry: Some(LocomotionGeometry {
                        ground: OrdinaryGroundQuery {
                            scene: &scene,
                            animations: &data,
                            prepared: &prepared,
                            distance: 0,
                            sidedness: false,
                        },
                        environment: cyrus_script::script::ObstacleEnvironment {
                            selected_player: Some(id),
                            auto_grab: false,
                            item_system_ready: true,
                            shell_execution: false,
                            ticks: 1,
                        },
                    }),
                    ..Default::default()
                },
                Some(&mut random),
                &mut |actor, sound| {
                    cues.push((actor, sound));
                    Ok(())
                },
            )
            .unwrap();
        assert_eq!(cues.len(), expected_cues, "tip {tip_x} base {base_x}");
        assert_eq!(random.state() != before, expected_cues == 1);
        for (actor, cue) in cues {
            assert_eq!(actor, id);
            assert!(matches!(
                cue,
                cyrus_script::script::ScriptSound::Positional {
                    effect: 31,
                    volume: 0,
                    ..
                }
            ));
        }
        assert_eq!(
            world.actors()[0]
                .control_state()
                .unwrap()
                .combat
                .as_ref()
                .unwrap()
                .flags
                .bits(),
            0x21,
            "attack intent stays set"
        );
    }
}

#[test]
fn rope_swing_suspends_outside_the_retained_window_and_resumes_on_reentry() {
    use cyrus_script::script::{OrdinaryInputs, ScriptInputs, camera::CameraPose};
    let (world, data, scene) = rope_world(rope("TERMINAL"), true);
    let mut scripts = cache();
    scripts.updates.ropes_load = Some(true);
    add_standalone_receivers(&mut scripts);
    // The fixture rewrites placement identity. Rebuild its matching source for restoration.
    for placement in &mut scripts.instances {
        placement.object_type = PlacementType::MODEL;
        placement.map_position = [10, 20, 65535];
    }
    scripts.content_id = scripts.content_identity().unwrap();
    let mut saved = movement_snapshot(&world, &scripts, &data);
    // A controlled nonzero state exposes updates even with the installed zero-swing maximum.
    saved["ropes"][0]["amplitude"] = 1024.into();
    saved["ropes"][0]["step"] = 200.into();
    let mut world = restore_navigation_owner(saved, &scripts, &data).unwrap();
    world
        .initialize_player_spatial(
            world.selected_player().unwrap(),
            [0, 0, 65535 * 256],
            [0; 3],
        )
        .unwrap();
    world
        .initialize_camera_pose(CameraPose {
            position: [5120 * 256, 0, 65535 * 256],
            angles: [0; 3],
        })
        .unwrap();
    world.ordinary_actors([5120, 65535]).unwrap();
    let frozen = world.ropes()[0].state();
    let advance = |world: &mut ScriptWorld, view| {
        world
            .update_ordinary(
                &data,
                view,
                OrdinaryInputs {
                    scene: Some(&scene),
                    scripts: ScriptInputs {
                        nominal_ticks: Some(1),
                        frame_scale: Some(256),
                        ..Default::default()
                    },
                },
                &mut cyrus_game::random::EngineRandom::default(),
                &mut |_, _| Ok(()),
                100,
            )
            .unwrap();
    };
    advance(&mut world, [5120, 65535]);
    assert_eq!(world.ropes()[0].state(), frozen);
    // Move the camera near the base without changing the retained cell.
    let mut placement = world.selected_player_placement().unwrap();
    placement.camera_position = [0, 0, 65535 * 256];
    world.restore_selected_player_placement(&placement).unwrap();
    advance(&mut world, [5120, 65535]);
    assert_eq!(world.ropes()[0].state(), frozen);
    // The first update in the rebuilt near window must resume the rope immediately.
    advance(&mut world, [0, 65535]);
    assert_eq!(world.ropes()[0].amplitude, 0);
    assert_eq!(world.ropes()[0].step, 0);
    assert_ne!(world.ropes()[0].state(), frozen);
}

#[test]
fn terminal_collision_keeps_the_geometry_published_before_the_oscillator() {
    use cyrus_script::script::{OrdinaryInputs, ScriptInputs};
    let mut records = rope("TERMINAL");
    records[0]["swing"] = 18.into();
    records[0]["length"] = 30.into();
    let (world, data, scene) = rope_world(records, true);
    let mut scripts = cache();
    scripts.updates.ropes_load = Some(true);
    add_standalone_receivers(&mut scripts);
    for placement in &mut scripts.instances {
        placement.object_type = PlacementType::MODEL;
        placement.map_position = [10, 20, 65535];
    }
    scripts.content_id = scripts.content_identity().unwrap();
    let mut saved = movement_snapshot(&world, &scripts, &data);
    saved["ropes"][0]["amplitude"] = 12.into();
    saved["ropes"][0]["step"] = 200.into();
    let mut world = restore_navigation_owner(saved, &scripts, &data).unwrap();
    let expected = world.ropes()[0].end_collision_pose().unwrap();
    assert_eq!(world.ropes()[0].end_pose().unwrap().pitch, 2026);
    world
        .update_ordinary(
            &data,
            [0, 65535],
            OrdinaryInputs {
                scene: Some(&scene),
                scripts: ScriptInputs {
                    nominal_ticks: Some(1),
                    frame_scale: Some(256),
                    ..Default::default()
                },
            },
            &mut cyrus_game::random::EngineRandom::default(),
            &mut |_, _| Ok(()),
            100,
        )
        .unwrap();
    assert_ne!(
        world.ropes()[0].step,
        200,
        "the oscillator must actually advance"
    );
    assert_eq!(
        world.ropes()[0].end_collision_pose().unwrap(),
        expected,
        "later collision reads the model published before the oscillator"
    );
    let bounds = world.transient_collision_models(&scene).unwrap()[0]
        .model
        .unwrap();
    assert_eq!(bounds.position, expected.0);
    let restored =
        restore_navigation_owner(movement_snapshot(&world, &scripts, &data), &scripts, &data)
            .unwrap();
    assert_eq!(
        restored.ropes()[0].end_collision_pose().unwrap(),
        expected,
        "restore must not replace the published pose with the advanced oscillator"
    );
}
