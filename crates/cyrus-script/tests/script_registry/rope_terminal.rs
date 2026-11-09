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
    let (mut world, mut data, _) = static_model_fixture(scripts);
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
        "position": [0.0, 0.0, 0.0], "angle_y": 0, "rope_type": 0, "swing": 0, "speed": 0,
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
        .prepare_ordinary_collision_query(id, &scene, &data, [10, 30], &[])
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
