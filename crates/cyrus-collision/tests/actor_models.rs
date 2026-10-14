use bevy::math::Mat3;
use cyrus_assets::collision::{
    ActorCollisionModels, CollisionGeometry, CollisionModel, CollisionScene, CollisionSubobject,
    StaticPlacement,
};
use cyrus_collision::actor_models::ActorModelCollision;
use cyrus_collision::candidates::{
    CandidateOwner, CandidateQuery, LiveActor, ModelBounds, ModelInstance,
};
use cyrus_collision::collision_mesh::ModelPose;
use cyrus_collision::collision_world::StaticCollision;
use cyrus_collision::ground::{GroundProbe, SupportSource};
use std::collections::BTreeMap;

fn geometry() -> CollisionGeometry {
    CollisionGeometry {
        version: Some(*b"v5.0"),
        radius: 100.0,
        vertices: vec![[-50.0, 0.0, -50.0], [50.0, 0.0, -50.0], [0.0, 0.0, 50.0]],
        normals: vec![[0.0, -1.0, 0.0]],
        faces: vec![vec![0, 1, 2]],
        textures: Some(vec![Some([302, 25])]),
        subobjects: vec![CollisionSubobject {
            center: [0.0; 3],
            radius: 100.0,
            extents: Some([1.0; 3]),
            faces: vec![0, 0],
        }],
    }
}

fn actors() -> ActorModelCollision {
    ActorModelCollision::new(&ActorCollisionModels {
        object_count: 43,
        models: BTreeMap::from([("DECK".into(), geometry())]),
        instances: BTreeMap::from([(42, "DECK".into())]),
        unsupported: Default::default(),
    })
    .unwrap()
}

#[test]
fn live_support_retains_owner_and_duplicate_order_and_follows_supplied_pose() {
    let mut scene = CollisionScene::default();
    scene.models.insert(
        "DECK".into(),
        CollisionModel {
            flags: 0,
            geometry: geometry(),
        },
    );
    scene.placements.push(StaticPlacement {
        id: 1,
        model: "DECK".into(),
        position: [600, 25, 600],
        rotation: [268435456, 0, 0, 0, 268435456, 0, 0, 0, 268435456],
    });
    let statics = StaticCollision::new(&scene).unwrap();
    let models = actors();
    let query = CandidateQuery::new([600 * 256, 0, 600 * 256], None).unwrap();
    let candidates = statics
        .runtime_candidates(
            [5000, 5000],
            &query,
            &[LiveActor {
                actor: 42,
                model: Some(ModelBounds {
                    instance: ModelInstance::Runtime(42),
                    position: [600.0, 25.0, 600.0],
                    radius: 100.0,
                }),
                ordinary_excluded: false,
                animation_excluded: false,
                attribute_55: 0,
                static_model_path: true,
                collision_flags: 4,
            }],
            &[],
        )
        .unwrap();
    assert_eq!(
        candidates
            .faces
            .iter()
            .map(|candidate| candidate.owner)
            .collect::<Vec<_>>(),
        vec![CandidateOwner::Static(0), CandidateOwner::Actor(42)]
    );
    let probe = GroundProbe::new([600 * 256, 0, 600 * 256], 0, 50).unwrap();
    let mut poses = BTreeMap::from([(
        42,
        ModelPose::new([600.0, 25.0, 600.0], Mat3::IDENTITY).unwrap(),
    )]);
    let contacts = statics
        .runtime_ground_contacts(&candidates, &models, &poses, &probe, false)
        .unwrap();
    assert_eq!(contacts.len(), 4);
    assert_eq!(
        contacts[0].source,
        SupportSource::Model { model: 0, face: 0 }
    );
    assert_eq!(
        contacts[1].source,
        SupportSource::Model { model: 0, face: 0 }
    );
    assert_eq!(
        contacts[2].source,
        SupportSource::Actor { actor: 42, face: 0 }
    );
    assert_eq!(
        contacts[3].source,
        SupportSource::Actor { actor: 42, face: 0 }
    );
    assert!(contacts.iter().all(|contact| contact.rank == 24576
        && contact.support.height == 25.0
        && contact.support.normal == [0, -256, 0]));
    assert_eq!(
        probe.select(None, &contacts).unwrap().source(),
        SupportSource::Actor { actor: 42, face: 0 }
    );
    poses.insert(
        42,
        ModelPose::new([600.0, 10.0, 600.0], Mat3::IDENTITY).unwrap(),
    );
    let contacts = statics
        .runtime_ground_contacts(&candidates, &models, &poses, &probe, false)
        .unwrap();
    let support = probe.select(None, &contacts).unwrap();
    assert_eq!(
        support.source(),
        SupportSource::Actor { actor: 42, face: 0 }
    );
    assert_eq!(support.actor_height(62, -2), -13824);
    assert_eq!(models.radius(42).unwrap(), 100.0);
    assert_eq!(
        models
            .face_surface(42, 0, Some(&BTreeMap::from([((302 << 7) | 25, 6)])))
            .unwrap(),
        6
    );
    assert!(
        statics
            .support_metadata(support.source())
            .unwrap_err()
            .to_string()
            .contains("runtime collision metadata owner")
    );
    poses.clear();
    assert!(
        statics
            .runtime_ground_contacts(&candidates, &models, &poses, &probe, false)
            .err()
            .unwrap()
            .to_string()
            .contains("stored model pose unavailable")
    );
}

#[test]
fn live_model_rotation_uses_original_axes_without_renderer_conversion() {
    let models = actors();
    // Rotate the horizontal polygon into a vertical plane in original axes.
    let rotation = Mat3::from_cols_array(&[1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, -1.0, 0.0]);
    let pose = ModelPose::new([600.0, 25.0, 600.0], rotation).unwrap();
    let contacts = models
        .segment_contacts(42, &pose, [600.0, 25.0, 590.0], [600.0, 25.0, 610.0], false)
        .unwrap();
    assert_eq!(contacts.len(), 2);
    assert_eq!(contacts[0].point, [600.0, 25.0, 600.0]);
    assert_eq!(contacts[0].normal, [0.0, 0.0, -1.0]);
    assert_eq!(contacts[0].rank, 16384);
}

#[test]
fn live_cache_missing_geometry_or_material_is_an_error_not_clearance() {
    let invalid = ActorCollisionModels {
        object_count: 1,
        instances: BTreeMap::from([(0, "MISSING".into())]),
        ..Default::default()
    };
    assert!(
        ActorModelCollision::new(&invalid)
            .err()
            .unwrap()
            .to_string()
            .contains("missing geometry")
    );
    let unsupported = ActorCollisionModels {
        object_count: 1,
        unsupported: BTreeMap::from([(0, "installed model absent".into())]),
        ..Default::default()
    };
    assert!(
        ActorModelCollision::new(&unsupported)
            .unwrap()
            .radius(0)
            .unwrap_err()
            .to_string()
            .contains("installed model absent")
    );
    assert!(
        actors()
            .face_surface(42, 0, None)
            .unwrap_err()
            .to_string()
            .contains("MPOB 42")
    );
    let mut solid = geometry();
    solid.textures = Some(vec![None]);
    let solid = ActorCollisionModels {
        object_count: 1,
        models: BTreeMap::from([("SOLID".into(), solid)]),
        instances: BTreeMap::from([(0, "SOLID".into())]),
        unsupported: Default::default(),
    };
    assert_eq!(
        ActorModelCollision::new(&solid)
            .unwrap()
            .face_surface(0, 0, None)
            .unwrap(),
        0
    );
}
