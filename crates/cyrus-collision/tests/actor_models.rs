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
        collision_flags: BTreeMap::from([(42, 3)]),
        unsupported: Default::default(),
    })
    .unwrap()
}

#[test]
fn primary_body_validates_shape_even_without_candidates() {
    use bevy::math::Vec3;
    use cyrus_collision::{
        candidates::CollisionCandidates, collision_mesh::BodyProbe,
        collision_world::PrimaryBodyQuery,
    };
    let statics = StaticCollision::new(&CollisionScene::default()).unwrap();
    let models = actors();
    let poses = BTreeMap::new();
    let candidates = CollisionCandidates {
        faces: vec![],
        actors: vec![],
    };
    for spheres in [vec![(Vec3::ZERO, -1.0)], vec![(Vec3::ZERO, 1.0); 25]] {
        assert!(
            statics
                .runtime_primary_body_contacts(
                    &candidates,
                    &models,
                    &poses,
                    PrimaryBodyQuery {
                        probe: BodyProbe {
                            position: [0; 3],
                            rotation: Mat3::IDENTITY,
                            spheres: &spheres,
                            sidedness: false
                        },
                        excluded_candidate_type: None,
                        terrain: None,
                    }
                )
                .is_err()
        );
    }
}

#[test]
fn mixed_primary_body_contacts_retain_owners_integer_normals_and_terrain_order() {
    use bevy::math::Vec3;
    use cyrus_collision::{
        candidates::{CollisionCandidates, FaceCandidate},
        collision_mesh::BodyProbe,
        collision_world::{PrimaryBodyQuery, RuntimeBodyContact},
    };
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
    let candidates = CollisionCandidates {
        faces: vec![
            FaceCandidate {
                model: ModelInstance::StaticPlacement(0),
                owner: CandidateOwner::Static(0),
                collision_flags: 0,
                candidate_type: 0x20,
            },
            FaceCandidate {
                model: ModelInstance::Runtime(42),
                owner: CandidateOwner::Actor(42),
                collision_flags: 3,
                candidate_type: 0x20,
            },
            FaceCandidate {
                model: ModelInstance::Runtime(99),
                owner: CandidateOwner::Transient,
                collision_flags: 0,
                candidate_type: 0x80,
            },
        ],
        actors: vec![],
    };
    let mut poses = BTreeMap::from([(
        42,
        ModelPose::new([600.0, 25.0, 600.0], Mat3::IDENTITY).unwrap(),
    )]);
    let spheres = [(Vec3::ZERO, 2.0)];
    let query = |terrain| PrimaryBodyQuery {
        probe: BodyProbe {
            position: [600 * 256, 24 * 256, 600 * 256],
            rotation: Mat3::IDENTITY,
            spheres: &spheres,
            sidedness: false,
        },
        excluded_candidate_type: Some(0x80),
        terrain,
    };
    let terrain = RuntimeBodyContact {
        source: SupportSource::Terrain,
        point: [600, 30, 600],
        normal: [0, -256, 0],
        classification: 4,
    };
    let contacts = statics
        .runtime_primary_body_contacts(&candidates, &models, &poses, query(Some(terrain)))
        .unwrap();
    assert_eq!(contacts.len(), 3);
    assert_eq!(
        contacts[0].source,
        SupportSource::Model { model: 0, face: 0 }
    );
    assert_eq!(
        contacts[1].source,
        SupportSource::Actor { actor: 42, face: 0 }
    );
    assert_eq!(contacts[2].source, SupportSource::Terrain);
    assert_eq!(contacts[1].point, [600, 25, 600]);
    assert_eq!(contacts[1].normal, [0, -256, 0]);
    assert_eq!(contacts[1].classification, 4);
    // A stored flipped pose changes world classification without normalization.
    poses.insert(
        42,
        ModelPose::new(
            [600.0, 25.0, 600.0],
            Mat3::from_diagonal(Vec3::new(1.0, -1.0, -1.0)),
        )
        .unwrap(),
    );
    let contacts = statics
        .runtime_primary_body_contacts(&candidates, &models, &poses, query(None))
        .unwrap();
    assert_eq!(contacts[1].normal, [0, 256, 0]);
    assert_eq!(contacts[1].classification, 8);
    let mut unsupported = query(None);
    unsupported.excluded_candidate_type = None;
    assert!(
        statics
            .runtime_primary_body_contacts(&candidates, &models, &poses, unsupported)
            .unwrap_err()
            .to_string()
            .contains("transient")
    );
}

#[test]
fn primary_body_limit_returns_first_sixteen_without_later_pose_or_terrain() {
    use bevy::math::Vec3;
    use cyrus_collision::{
        candidates::{CollisionCandidates, FaceCandidate},
        collision_mesh::BodyProbe,
        collision_world::{PrimaryBodyQuery, RuntimeBodyContact},
    };
    let mut deck = geometry();
    deck.faces = vec![vec![0, 1, 2]; 33];
    deck.normals = vec![[0.0, -1.0, 0.0]; 33];
    deck.textures = Some(vec![None; 33]);
    deck.subobjects[0].faces = (0..33).collect();
    let mut scene = CollisionScene::default();
    scene.models.insert(
        "DECK".into(),
        CollisionModel {
            flags: 0,
            geometry: deck,
        },
    );
    scene.placements.push(StaticPlacement {
        id: 1,
        model: "DECK".into(),
        position: [600, 25, 600],
        rotation: [268435456, 0, 0, 0, 268435456, 0, 0, 0, 268435456],
    });
    let statics = StaticCollision::new(&scene).unwrap();
    let candidates = CollisionCandidates {
        faces: vec![
            FaceCandidate {
                model: ModelInstance::StaticPlacement(0),
                owner: CandidateOwner::Static(0),
                collision_flags: 0,
                candidate_type: 0x20,
            },
            FaceCandidate {
                model: ModelInstance::Runtime(42),
                owner: CandidateOwner::Actor(42),
                collision_flags: 0,
                candidate_type: 0x20,
            },
        ],
        actors: vec![],
    };
    let spheres = [(Vec3::ZERO, 2.0)];
    let result = statics
        .runtime_primary_body_contacts(
            &candidates,
            &actors(),
            &BTreeMap::new(),
            PrimaryBodyQuery {
                probe: BodyProbe {
                    position: [600 * 256, 24 * 256, 600 * 256],
                    rotation: Mat3::IDENTITY,
                    spheres: &spheres,
                    sidedness: false,
                },
                excluded_candidate_type: None,
                terrain: Some(RuntimeBodyContact {
                    source: SupportSource::Terrain,
                    point: [0; 3],
                    normal: [0, -256, 0],
                    classification: 4,
                }),
            },
        )
        .unwrap();
    assert_eq!(result.len(), 16);
    assert_eq!(
        result
            .iter()
            .map(|contact| contact.source)
            .collect::<Vec<_>>(),
        (0..16)
            .map(|face| SupportSource::Model { model: 0, face })
            .collect::<Vec<_>>()
    );
    let residual = || cyrus_collision::collision_world::ResidualBodyQuery {
        probe: BodyProbe {
            position: [600 * 256, 24 * 256, 600 * 256],
            rotation: Mat3::IDENTITY,
            spheres: &spheres,
            sidedness: false,
        },
        excluded_candidate_type: None,
    };
    // Residual collection must inspect the later live pose after thirty-two static faces.
    assert!(
        statics
            .runtime_residual_body_contacts(&candidates, &actors(), &BTreeMap::new(), residual())
            .unwrap_err()
            .to_string()
            .contains("stored model pose")
    );
    let poses = BTreeMap::from([(
        42,
        ModelPose::new([600.0, 25.0, 600.0], Mat3::IDENTITY).unwrap(),
    )]);
    let all = statics
        .runtime_residual_body_contacts(&candidates, &actors(), &poses, residual())
        .unwrap();
    assert_eq!(all.len(), 33);
    assert_eq!(all[31].source, SupportSource::Model { model: 0, face: 31 });
    assert_eq!(all[32].source, SupportSource::Actor { actor: 42, face: 0 });
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
        .runtime_ground_contacts(&candidates, &models, &poses, &probe, false, None)
        .unwrap();
    let segment = statics
        .runtime_segment_contacts(
            &candidates,
            &models,
            &poses,
            [600, 0, 600],
            [600, 50, 600],
            false,
            None,
        )
        .unwrap();
    assert_eq!(
        segment
            .iter()
            .map(|(source, _)| *source)
            .collect::<Vec<_>>(),
        vec![
            SupportSource::Model { model: 0, face: 0 },
            SupportSource::Model { model: 0, face: 0 },
            SupportSource::Actor { actor: 42, face: 0 },
            SupportSource::Actor { actor: 42, face: 0 },
        ]
    );
    assert!(
        segment
            .iter()
            .all(|(_, hit)| hit.point == [600.0, 25.0, 600.0])
    );
    let mut filtered = cyrus_collision::candidates::CollisionCandidates {
        faces: candidates.faces.clone(),
        actors: candidates.actors.clone(),
    };
    filtered.faces[1].candidate_type = 128;
    let static_only = statics
        .runtime_segment_contacts(
            &filtered,
            &models,
            &BTreeMap::new(),
            [600, 0, 600],
            [600, 50, 600],
            false,
            Some(128),
        )
        .unwrap();
    assert_eq!(static_only.len(), 2);
    assert!(
        statics
            .runtime_segment_contacts(
                &filtered,
                &models,
                &BTreeMap::new(),
                [600, 0, 600],
                [600, 50, 600],
                false,
                None,
            )
            .is_err()
    );
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
        .runtime_ground_contacts(&candidates, &models, &poses, &probe, false, None)
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
    let mut excluded = cyrus_collision::candidates::CollisionCandidates {
        faces: candidates.faces.clone(),
        actors: Vec::new(),
    };
    for candidate in &mut excluded.faces {
        candidate.candidate_type = 128;
    }
    assert!(
        statics
            .runtime_ground_contacts(
                &excluded,
                &models,
                &BTreeMap::new(),
                &probe,
                false,
                Some(128)
            )
            .unwrap()
            .is_empty()
    );
    poses.clear();
    assert!(
        statics
            .runtime_ground_contacts(&candidates, &models, &poses, &probe, false, None)
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
        collision_flags: BTreeMap::from([(0, 0)]),
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

#[test]
fn initial_collision_metadata_distinguishes_known_zero_missing_and_invalid_actor() {
    let source = ActorCollisionModels {
        object_count: 2,
        collision_flags: BTreeMap::from([(0, 0), (1, 6)]),
        ..Default::default()
    };
    let prepared = ActorModelCollision::new(&source).unwrap();
    assert_eq!(prepared.collision_flags(0).unwrap(), 0);
    assert_eq!(prepared.collision_flags(1).unwrap(), 6);
    assert_eq!(prepared.loaded_radius(0).unwrap(), None);
    assert!(prepared.loaded_radius(2).is_err());
    assert!(prepared.collision_flags(2).is_err());
    let absent = ActorModelCollision::new(&ActorCollisionModels {
        object_count: 1,
        ..Default::default()
    })
    .unwrap();
    assert!(
        absent
            .collision_flags(0)
            .unwrap_err()
            .to_string()
            .contains("re-export")
    );
    let mut invalid = source;
    invalid.collision_flags.insert(2, 3);
    assert!(ActorModelCollision::new(&invalid).is_err());
}
