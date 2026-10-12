use bevy::math::Vec3;
use cyrus_collision::geometry::CollisionFace;
use cyrus_collision::ground::GroundProbe;
use cyrus_collision::ground::GroundSupport;
use cyrus_collision::ground::ModelSupport;
use cyrus_collision::ground::SupportSource;

#[test]
fn support_history_keeps_probe_changes_and_miss_metadata_across_actors() {
    use cyrus_collision::ground::SupportHistory;
    let mut history = SupportHistory::new(0, 0);
    history.finish_query(Some(4), true);
    assert_eq!(history.ticks(), 12);
    history.begin_player_update(5);
    assert_eq!(history.ticks(), 7);
    history.finish_query(None, true);
    assert_eq!(history.ticks(), 12);
    history.begin_player_update(0);
    assert_eq!(history.ticks(), 12);
    history.begin_player_update(20);
    assert_eq!(history.ticks(), 0);
    history.finish_query(Some(4), false);
    assert_eq!(history.ticks(), 0);
    history.finish_query(None, true);
    assert_eq!(history.ticks(), 12);
    history.finish_query(Some(0), false);
    history.begin_player_update(5);
    history.finish_query(None, true);
    assert_eq!(history.ticks(), 7);
}

fn floor() -> CollisionFace {
    CollisionFace::new(
        vec![
            Vec3::new(100.0, -32.0, 100.0),
            Vec3::new(-100.0, -32.0, 100.0),
            Vec3::new(-100.0, -32.0, -100.0),
            Vec3::new(100.0, -32.0, -100.0),
        ],
        Vec3::NEG_Y,
    )
    .unwrap()
}

#[test]
fn original_ground_segment_fraction_uses_32768_scale() {
    let contact = floor()
        .segment_contact(
            Vec3::new(34.0, -84.0, 22.0),
            Vec3::new(34.0, 16.0, 22.0),
            true,
        )
        .unwrap();
    assert_eq!(contact.point, Vec3::new(34.0, -32.0, 22.0));
    assert_eq!(contact.normal, Vec3::NEG_Y);
    assert_eq!(contact.rank, 17_039);
}

#[test]
fn segment_requires_plane_crossing_and_polygon_containment() {
    let face = floor();
    let point = |x, y| Vec3::new(x, y, 0.0);
    assert!(
        face.segment_contact(point(0.0, -40.0), point(0.0, -32.0), true)
            .is_some()
    );
    assert!(
        face.segment_contact(point(100.0, -40.0), point(100.0, -20.0), true)
            .is_some()
    );
    assert!(
        face.segment_contact(point(101.0, -40.0), point(101.0, -20.0), true)
            .is_none()
    );
    assert!(
        face.segment_contact(point(0.0, -40.0), point(0.0, -33.0), true)
            .is_none()
    );
    assert!(
        face.segment_contact(point(0.0, -32.0), point(1.0, -32.0), true)
            .is_none()
    );
    assert!(
        face.segment_contact(point(0.0, -27.0), point(0.0, -40.0), true)
            .is_none()
    );
    assert!(
        face.segment_contact(point(0.0, -27.0), point(0.0, -40.0), false)
            .is_some()
    );
    assert!(
        face.segment_contact(point(0.0, -28.0), point(0.0, -40.0), true)
            .is_some()
    );
}

fn support(height: f64) -> GroundSupport {
    GroundSupport {
        height,
        normal: [0, -256, 0],
    }
}

#[test]
fn original_model_support_replaces_terrain_and_preserves_actor_height() {
    let probe =
        GroundProbe::new([36_965 * 256 + 255, -388 * 256, 40_065 * 256 + 1], 61, 50).unwrap();
    assert_eq!(
        probe.endpoints(),
        ([36_965, -377, 40_065], [36_965, -277, 40_065])
    );
    let selected = probe
        .select(
            Some(support(-287.5)),
            &[ModelSupport {
                face: 3,
                model: 7,
                support: support(-325.0),
                rank: 17_039,
            }],
        )
        .unwrap();
    assert_eq!(
        selected.source(),
        SupportSource::Model { model: 7, face: 3 }
    );
    assert_eq!(selected.actor_height(61, -2), -99_328);
}

#[test]
fn terrain_rank_truncates_height_and_equal_model_ranks_keep_last_contact() {
    let probe = GroundProbe::new([0, -327 * 256, 0], 0, 50).unwrap();
    let candidates = [
        ModelSupport {
            face: 3,
            model: 1,
            support: support(-323.0),
            rank: 17_650,
        },
        ModelSupport {
            face: 3,
            model: 2,
            support: support(-323.0),
            rank: 17_650,
        },
        ModelSupport {
            face: 3,
            model: 3,
            support: GroundSupport {
                height: -350.0,
                normal: [256, 0, 0],
            },
            rank: 1,
        },
    ];
    assert_eq!(
        probe
            .select(Some(support(-350.5)), &candidates)
            .unwrap()
            .source(),
        SupportSource::Model { model: 2, face: 3 },
    );
    assert!(probe.select(Some(support(-276.0)), &[]).is_none());
    assert_eq!(
        probe.select(Some(support(-400.0)), &[]).unwrap().source(),
        SupportSource::Terrain
    );
    assert!(GroundProbe::new([0; 3], 0, 0).is_err());
    assert!(GroundProbe::new([i64::MAX; 3], 0, 50).is_err());
}
