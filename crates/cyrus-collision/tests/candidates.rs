use cyrus_collision::candidates::*;

fn model(id: usize, position: [f32; 3]) -> ModelBounds {
    ModelBounds {
        instance: ModelInstance::Runtime(id),
        position,
        radius: 10.0,
    }
}

fn query() -> CandidateQuery {
    CandidateQuery::new([0; 3], Some(model(0, [0.0; 3]))).unwrap()
}

fn static_model(id: usize, flags: u8) -> StaticModel {
    StaticModel {
        placement: id,
        model: Some(model(id, [0.0; 3])),
        collision_flags: flags,
    }
}

fn actor(id: usize, static_model_path: bool) -> LiveActor {
    LiveActor {
        actor: id,
        model: Some(model(id, [0.0; 3])),
        ordinary_excluded: false,
        animation_excluded: false,
        attribute_55: 0,
        static_model_path,
        collision_flags: 2,
    }
}

#[test]
fn candidates_preserve_phase_order_owner_and_metadata() {
    let result = CollisionCandidates::build(
        &query(),
        &[static_model(1, 4), static_model(2, 1)],
        &[actor(3, true), actor(4, false)],
        &[TransientModel {
            active: true,
            flags: 0x10,
            model: Some(model(5, [100_000.0; 3])),
        }],
    )
    .unwrap();
    assert_eq!(
        result.faces,
        vec![
            FaceCandidate {
                model: ModelInstance::Runtime(1),
                owner: CandidateOwner::Static(1),
                collision_flags: 4,
                candidate_type: 0x20,
            },
            FaceCandidate {
                model: ModelInstance::Runtime(3),
                owner: CandidateOwner::Actor(3),
                collision_flags: 2,
                candidate_type: 0x20,
            },
            FaceCandidate {
                model: ModelInstance::Runtime(5),
                owner: CandidateOwner::Transient,
                collision_flags: 0,
                candidate_type: 0x80,
            },
        ]
    );
    assert_eq!(
        result.actors,
        vec![
            ActorCandidate {
                model: Some(ModelInstance::Runtime(2)),
                owner: CandidateOwner::Static(2),
                collision_flags: 1,
            },
            ActorCandidate {
                model: None,
                owner: CandidateOwner::Actor(4),
                collision_flags: 2,
            },
        ]
    );
}

#[test]
fn static_face_saturation_ends_static_phase_but_live_phase_continues() {
    let mut statics: Vec<_> = (1..=65).map(|id| static_model(id, 0)).collect();
    statics.push(static_model(66, 1));
    let result = CollisionCandidates::build(&query(), &statics, &[actor(67, false)], &[]).unwrap();
    assert_eq!(result.faces.len(), 64);
    assert_eq!(result.faces[63].owner, CandidateOwner::Static(64));
    assert_eq!(result.actors.len(), 1);
    assert_eq!(result.actors[0].owner, CandidateOwner::Actor(67));
}

#[test]
fn actor_saturation_skips_that_destination_and_keeps_face_candidates() {
    let mut statics: Vec<_> = (1..=65).map(|id| static_model(id, 1)).collect();
    statics.push(static_model(66, 0));
    let result = CollisionCandidates::build(
        &query(),
        &statics,
        &[actor(67, false), actor(68, true)],
        &[],
    )
    .unwrap();
    assert_eq!(result.actors.len(), 64);
    assert_eq!(result.actors[63].owner, CandidateOwner::Static(64));
    assert_eq!(
        result
            .faces
            .iter()
            .map(|face| face.owner)
            .collect::<Vec<_>>(),
        vec![CandidateOwner::Static(66), CandidateOwner::Actor(68)]
    );
}

#[test]
fn live_exclusions_and_own_model_identity_are_independent() {
    let mut records: Vec<_> = (0..=6).map(|id| actor(id, true)).collect();
    records[1].ordinary_excluded = true;
    records[2].animation_excluded = true;
    records[3].attribute_55 = 1;
    records[4].model = None;
    records[5].model.as_mut().unwrap().position = [31.0, 0.0, 0.0];
    let result = CollisionCandidates::build(
        &query(),
        &[static_model(0, 0), static_model(7, 2)],
        &records,
        &[],
    )
    .unwrap();
    assert_eq!(result.faces.len(), 1);
    assert_eq!(result.faces[0].owner, CandidateOwner::Actor(6));
}

#[test]
fn distance_equality_passes_and_missing_query_model_uses_radius_200() {
    let mut live = actor(1, true);
    live.model.as_mut().unwrap().position = [30.0, 0.0, 0.0];
    assert_eq!(
        CollisionCandidates::build(&query(), &[], &[live], &[])
            .unwrap()
            .faces
            .len(),
        1
    );
    let mut boundary = static_model(2, 0);
    boundary.model.as_mut().unwrap().position = [210.0, 0.0, 0.0];
    let query = CandidateQuery::new([255, -1, 0], None).unwrap();
    // Arithmetic shift retains Y=-1, so the diagonal is outside the radius.
    assert!(
        CollisionCandidates::build(&query, &[boundary], &[], &[])
            .unwrap()
            .faces
            .is_empty()
    );
    let mut boundary = static_model(2, 0);
    boundary.model.as_mut().unwrap().position = [210.0, -1.0, 0.0];
    assert_eq!(
        CollisionCandidates::build(&query, &[boundary], &[], &[])
            .unwrap()
            .faces
            .len(),
        1
    );
}

#[test]
fn transient_admission_retains_flags_and_empty_loaded_model_boundary() {
    let records = [
        TransientModel {
            active: false,
            flags: 0x10,
            model: Some(model(1, [0.0; 3])),
        },
        TransientModel {
            active: true,
            flags: 0,
            model: Some(model(2, [0.0; 3])),
        },
        TransientModel {
            active: true,
            flags: 0x10,
            model: None,
        },
    ];
    assert!(
        CollisionCandidates::build(&query(), &[], &[], &records)
            .unwrap()
            .faces
            .is_empty()
    );
    let invalid = StaticModel {
        placement: 4,
        model: Some(model(4, [f32::NAN, 0.0, 0.0])),
        collision_flags: 0,
    };
    assert!(
        CollisionCandidates::build(&query(), &[invalid], &[], &[])
            .unwrap_err()
            .to_string()
            .contains("Runtime(4)")
    );
}

#[test]
fn both_full_lists_return_before_reading_remaining_live_records() {
    let mut statics: Vec<_> = (1..=64).map(|id| static_model(id, 0)).collect();
    statics.extend((65..=128).map(|id| static_model(id, 1)));
    let mut invalid = actor(129, true);
    invalid.model.as_mut().unwrap().radius = f32::NAN;
    let result = CollisionCandidates::build(&query(), &statics, &[invalid], &[]).unwrap();
    assert_eq!(result.faces.len(), 64);
    assert_eq!(result.actors.len(), 64);
}

#[test]
fn query_radius_truncates_before_model_distance_selection() {
    let mut own = model(0, [0.0; 3]);
    own.radius = 10.75;
    let query = CandidateQuery::new([0; 3], Some(own)).unwrap();
    let mut boundary = static_model(1, 0);
    boundary.model.as_mut().unwrap().position = [31.25, 0.0, 0.0];
    assert!(
        CollisionCandidates::build(&query, &[boundary], &[], &[])
            .unwrap()
            .faces
            .is_empty()
    );
}
