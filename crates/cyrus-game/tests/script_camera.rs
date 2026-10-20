//! Logical camera replacements preserve stage order and reject unavailable terrain sources.
use anyhow::Result;
use cyrus_collision::collision_mesh::IntegerSegmentContact;
use cyrus_game::script::{ScriptActorId, camera::*};
use std::cell::RefCell;

fn state() -> CameraState {
    let actor = ScriptActorId {
        generation: 7,
        index: 1,
    };
    let record = CameraRecord {
        actor,
        position_offset: [0; 3],
        angle_offset: [0; 3],
        request: [100, 200, 300],
        target_offset: [0; 3],
        bound_position_history: [0; 3],
        basis: [[0; 3]; 2],
        distance: 0,
        minimum_distance: 0,
        maximum_movement: 1000,
        flags: 0x4b,
    };
    CameraState {
        records: std::array::from_fn(|_| record.clone()),
        selected: CameraSlot::new(1).unwrap(),
        index: CameraSlot::new(1).unwrap(),
        transition: 2,
        scripted_active: 0,
        scripted_nesting: 0,
        reselect_pending: false,
        current: CameraPose {
            position: [7, 8, 9],
            angles: [11, 12, 13],
        },
        previous: CameraPose {
            position: [0; 3],
            angles: [0; 3],
        },
        transformed_basis: [[0; 3]; 2],
        camera_actor: actor,
        alternate_actor: ScriptActorId {
            generation: 7,
            index: 2,
        },
        marks: CameraMarks::default(),
        render: RenderPose {
            position: [0.0; 3],
            angles: [0; 3],
        },
    }
}

struct Geometry {
    calls: Vec<&'static str>,
    hit: bool,
    bad_height: bool,
}
impl CameraGeometry for Geometry {
    fn segment(
        &mut self,
        _: [i32; 3],
        _: [i32; 3],
        _: ScriptActorId,
    ) -> Result<Vec<IntegerSegmentContact>> {
        self.calls.push("segment");
        Ok(if self.hit {
            vec![IntegerSegmentContact {
                face: 0,
                rank: 32767,
                point: [90, 80, 70],
                normal: [0, 256, 0],
            }]
        } else {
            vec![]
        })
    }
    fn contacts(&mut self, _: [i32; 3], _: ScriptActorId) -> Result<Vec<[i32; 3]>> {
        self.calls.push("contacts");
        Ok(vec![])
    }
    fn terrain_height(&mut self, _: [i32; 2]) -> Result<f32> {
        self.calls.push("terrain");
        Ok(if self.bad_height { f32::NAN } else { 1000.0 })
    }
}

#[test]
fn pose_preparation_orders_corrections_and_preserves_live_state_on_rejection() {
    let state = state();
    let hits = CameraHitSelection {
        minimum_position: [11, 22, 33],
        maximum_position: [44, 55, 66],
        minimum_normal: [0; 3],
        maximum_normal: [0; 3],
    };
    let inputs = CameraUpdateInputs {
        bound_position: [1000, 2000, 3000],
        bound_angles: [0; 3],
        movement: MovementLimits {
            budget_increment: 100,
            vertical: -100,
            frame_scale: 256,
            selected_player_special_state: false,
        },
        body_radius: 1.0,
        body_clearance: 0.0,
        terrain_enabled: true,
    };
    let mut geometry = Geometry {
        calls: vec![],
        hit: false,
        bad_height: false,
    };
    let update = state.prepare_pose(&hits, &inputs, &mut geometry).unwrap();
    assert_eq!(
        geometry.calls,
        ["segment", "segment", "contacts", "terrain"]
    );
    assert_eq!(update.state.current.position, [100, 200, 300]);
    assert_eq!(update.state.current.angles, [11, 12, 13]);
    assert_eq!(update.state.previous, state.current);
    assert_eq!(update.state.transition, 2);
    assert_eq!(
        update.actor_positions,
        [
            (state.camera_actor, [100, 200, 300]),
            (state.alternate_actor, [100, 200, 300])
        ]
    );

    geometry.hit = true;
    geometry.calls.clear();
    let update = state.prepare_pose(&hits, &inputs, &mut geometry).unwrap();
    assert_eq!(
        geometry.calls,
        [
            "segment", "contacts", "terrain", "segment", "contacts", "terrain"
        ]
    );
    // Equal minimum rank retains the earlier endpoint; the maximum still changes.
    assert_eq!(update.state.current.position, [11, 22, 33]);
    assert_eq!(update.hits.maximum_position, [23040, 20480, 17920]);
    assert_eq!(update.state.transition, 0);

    geometry.bad_height = true;
    assert!(state.prepare_pose(&hits, &inputs, &mut geometry).is_err());
    assert_eq!(state.current.position, [7, 8, 9]);
    assert_eq!(state.transition, 2);
    assert_eq!(hits.maximum_position, [44, 55, 66]);
}

#[test]
fn selection_and_show_player_retain_transition_and_task_counter_rules() {
    let mut state = state();
    state.records[1].flags = 0;
    state.select(3).unwrap();
    assert_eq!(state.selected, CameraSlot::new(1).unwrap());
    state.records[1].flags = 1;
    state.select(3).unwrap();
    assert_eq!(state.selected, CameraSlot::new(3).unwrap());
    let mut counter = 0;
    assert!(state.show_player(Some(3), &mut counter, 0).unwrap());
    assert_eq!(counter, 1);
    state.records[3].flags |= 0x20;
    assert!(state.reselection_inhibited());
    assert!(!state.show_player(Some(3), &mut counter, 1).unwrap());
    assert_eq!(state.records[3].flags & 0x21, 0);
    assert!(state.show_player(Some(9), &mut counter, 1).is_err());
    assert_eq!(counter, 2);
}

struct Terrain {
    reads: RefCell<Vec<(&'static str, [i32; 2])>>,
    unavailable: bool,
}
impl CameraTerrainHeights for Terrain {
    fn retained_center(&self) -> [i32; 2] {
        [0, 0]
    }
    fn retained_height(&self, cell: [i32; 2]) -> Result<f32> {
        self.reads.borrow_mut().push(("retained", cell));
        anyhow::ensure!(!self.unavailable, "retained terrain unavailable");
        Ok(12.0)
    }
    fn map_height(&self, cell: [i32; 2]) -> Result<f32> {
        self.reads.borrow_mut().push(("map", cell));
        Ok(20.0)
    }
}

#[test]
fn terrain_uses_one_source_for_the_triangle_without_fallback() {
    let mut terrain = Terrain {
        reads: RefCell::new(vec![]),
        unavailable: false,
    };
    assert_eq!(
        sample_terrain([0, 0], &terrain).unwrap(),
        (12.0, [0, -256, 0])
    );
    assert_eq!(
        *terrain.reads.borrow(),
        [
            ("retained", [0, 0]),
            ("retained", [1, -1]),
            ("retained", [0, -1])
        ]
    );
    terrain.reads.borrow_mut().clear();
    assert_eq!(sample_terrain([4096, 0], &terrain).unwrap().0, 20.0);
    assert!(
        terrain
            .reads
            .borrow()
            .iter()
            .all(|(source, _)| *source == "map")
    );
    terrain.unavailable = true;
    terrain.reads.borrow_mut().clear();
    assert!(sample_terrain([0, 0], &terrain).is_err());
    assert_eq!(*terrain.reads.borrow(), [("retained", [0, 0])]);
}

#[test]
fn model_collection_preserves_eligible_order_and_excludes_only_requested_identity() {
    use cyrus_collision::candidates::{ModelBounds, ModelInstance};
    let first = ModelBounds {
        instance: ModelInstance::StaticPlacement(3),
        position: [0.0; 3],
        radius: 100.0,
    };
    let second = ModelBounds {
        instance: ModelInstance::Runtime(7),
        ..first
    };
    assert_eq!(
        collect_segment_models([0; 3], [0; 3], 99 * 256, None, &[second, first, second]).unwrap(),
        [second.instance, first.instance, second.instance]
    );
    assert_eq!(
        collect_segment_models([0; 3], [0; 3], 0, Some(first.instance), &[first, second]).unwrap(),
        [second.instance]
    );
    assert!(
        collect_segment_models([0; 3], [0; 3], 100 * 256, None, &[first])
            .unwrap()
            .is_empty()
    );
    assert!(collect_segment_models([0; 3], [0; 3], 0, None, &[first; 65]).is_err());
    assert!(collect_segment_models([0; 3], [i32::MAX; 3], 0, None, &[first]).is_err());
}

#[test]
fn request_precedence_and_distance_update_precede_geometry() {
    let mut record = state().records[0].clone();
    record.distance = 0;
    record.minimum_distance = 17;
    record.flags = 0x0c;
    assert_eq!(
        record.candidate([1000, 2000, 3000], [0; 3]).unwrap(),
        [1100, 2200, 3300]
    );
    assert_eq!(record.distance, 17);
    record.flags = 0x08;
    assert_eq!(
        record.candidate([1000, 2000, 3000], [0; 3]).unwrap(),
        [100, 200, 300]
    );
    record.flags = 0;
    record.minimum_distance = 0;
    record.distance = 10;
    assert_eq!(
        record.candidate([1000, 2000, 3000], [0; 3]).unwrap(),
        [1000, 2000, 440]
    );
}

#[test]
fn initial_records_follow_the_documented_installed_setting_mapping() {
    let player = ScriptActorId {
        generation: 1,
        index: 2,
    };
    let mode = |base: i32, max_vel| CameraModeSettings {
        angle_offset: [base, base + 1, base + 2],
        distance: base * 10,
        max_vel,
    };
    let settings = CameraSettings {
        offset_pos: [1, 2, 3],
        offset_angle: [4, 5, 6],
        target_offset: [7, 8, 9],
        distance: 250,
        min_distance: 120,
        max_vel: 900,
        modes: [
            mode(10, 0),
            mode(20, 5),
            mode(30, 0),
            mode(40, 0),
            mode(50, 0),
            mode(60, 7),
        ],
        limits: Default::default(),
    };
    let records = CameraState::initial_records(&settings, player).unwrap();
    assert_eq!(records[0].target_offset, [7, 8, 9]);
    assert_eq!(records[3].target_offset, [7, 8, 9]);
    assert_eq!(records[1].target_offset, [0; 3]);
    assert_eq!(records[2].position_offset, [1, 2, 3]);
    assert_eq!(
        (records[3].angle_offset, records[3].distance),
        ([10, 11, 12], 100)
    );
    assert_eq!(
        records[3].maximum_movement, 900,
        "zero keeps the record-zero rate"
    );
    assert_eq!(records[4].maximum_movement, 5);
    assert_eq!(records[4].flags, 0x40);
    assert_eq!(records[5].flags, 0xc0, "rope adds the angle-override bit");
    assert_eq!(records[6].flags, 0x40);
    assert_eq!(
        records[8].angle_offset,
        [62, 5, 6],
        "debug stores land in the first field"
    );
    assert_eq!(records[8].maximum_movement, 7);
    let negative = CameraSettings {
        max_vel: -1,
        ..settings
    };
    assert!(CameraState::initial_records(&negative, player).is_err());
}

#[test]
fn location_offsets_convert_attribute_bytes_for_the_selected_record_only() {
    let mut camera = state();
    camera.set_location_offsets([0, 127, 255, 1, 128, 0]);
    let record = &camera.records[1];
    assert_eq!(record.target_offset, [0, 0, 64_000]);
    assert_eq!(record.position_offset, [-63_000, 500, 0]);
    assert_eq!(camera.records[0].target_offset, [0; 3]);
}

#[test]
fn player_slot_requests_follow_the_selector_priority() {
    // (holding weapon, debug override, living state, rope) -> slot
    for (inputs, expected) in [
        ((true, true, 15, true), Some(3)),
        ((false, true, 15, true), Some(8)),
        ((false, false, 15, true), Some(4)),
        ((false, false, 14, true), Some(5)),
        ((false, false, 14, false), None),
    ] {
        assert_eq!(
            player_slot(inputs.0, inputs.1, inputs.2, inputs.3),
            expected,
            "{inputs:?}"
        );
    }
}

#[test]
fn movement_limits_negate_the_installed_vertical_velocity() {
    let limits = CameraLimits {
        max_acc: 5000,
        max_y_vel: 4000,
        ..Default::default()
    };
    let movement = MovementLimits::from_installed(&limits, 256, true);
    assert_eq!(
        (
            movement.budget_increment,
            movement.vertical,
            movement.frame_scale,
            movement.selected_player_special_state
        ),
        (5000, -4000, 256, true)
    );
}

/// Every step of the original `showPlayerPan` for synthetic settings, recorded by running
/// the original routine over controlled inputs: (elapsed, still active, selected record,
/// selected index, scripted active word, nesting, private counter, angle offset, distance).
#[test]
fn show_player_pan_reproduces_the_original_step_by_step() {
    type Step = (i32, bool, usize, usize, u16, u32, u32, [i32; 3], i32);
    struct Case {
        name: &'static str,
        position: [i32; 3],
        angles: [u16; 3],
        published: [i32; 3],
        duration: i32,
        requested_slot: Option<u8>,
        steps: Vec<Step>,
    }
    let case = |name, duration, requested_slot, steps: Vec<Step>| Case {
        name,
        position: [4000, 9000, -6000],
        angles: [100, 1900, 20],
        published: [4300, 12000, -9500],
        duration,
        requested_slot,
        steps,
    };
    let cases = [
        case(
            "slot zero",
            5,
            None,
            vec![
                (0, true, 2, 0, 1, 1, 1, [1729, 3931, 20], 48),
                (1, true, 2, 0, 1, 1, 2, [1816, 3456, 17], 116),
                (2, true, 2, 0, 1, 1, 3, [1903, 2980, 13], 184),
                (3, true, 2, 0, 1, 1, 4, [1990, 2504, 9], 252),
                (4, false, 0, 0, 0, 0, 6, [30, -20, 5], 321),
            ],
        ),
        case(
            "combat slot",
            4,
            Some(3),
            vec![
                (0, true, 2, 3, 1, 1, 1, [1729, 3931, 20], 48),
                (1, true, 2, 3, 1, 1, 2, [1837, 3306, 17], 82),
                (2, true, 2, 3, 1, 1, 3, [1946, 2681, 13], 116),
                (3, false, 3, 3, 0, 0, 5, [7, 8, 9], 150),
            ],
        ),
        case(
            "duration one",
            1,
            None,
            vec![(0, false, 0, 0, 0, 0, 2, [30, -20, 5], 321)],
        ),
        Case {
            position: [-800, 5000, 3300],
            angles: [2000, 40, 1000],
            published: [300, -4000, 9000],
            ..case(
                "wrapped differences",
                7,
                None,
                vec![
                    (0, true, 2, 0, 1, 1, 1, [2000, 1120, 1000], 22),
                    (1, true, 2, 0, 1, 1, 2, [2013, 1271, 835], 71),
                    (2, true, 2, 0, 1, 1, 3, [2026, 1422, 669], 121),
                    (3, true, 2, 0, 1, 1, 4, [2039, 1574, 503], 171),
                    (4, true, 2, 0, 1, 1, 5, [2052, 1725, 337], 221),
                    (5, true, 2, 0, 1, 1, 6, [2065, 1876, 171], 271),
                    (6, false, 0, 0, 0, 0, 8, [30, -20, 5], 321),
                ],
            )
        },
    ];
    let mode = |angle_offset, distance| CameraModeSettings {
        angle_offset,
        distance,
        max_vel: 0,
    };
    let settings = CameraSettings {
        offset_pos: [100, -9000, 50],
        offset_angle: [30, -20, 5],
        target_offset: [-5000, -12000, 7],
        distance: 321,
        min_distance: 77,
        max_vel: 800,
        modes: std::array::from_fn(|slot| {
            if slot == 0 {
                mode([7, 8, 9], 150)
            } else {
                CameraModeSettings::default()
            }
        }),
        limits: Default::default(),
    };
    let actor = |index| ScriptActorId {
        generation: 1,
        index,
    };
    for case in cases {
        let mut state = CameraState::new(&settings, actor(2), actor(3), 0).unwrap();
        state.current = CameraPose {
            position: case.published,
            angles: [0; 3],
        };
        let (mut counter, mut pan) = (0u32, None);
        for (
            elapsed,
            active,
            record,
            index,
            scripted,
            nesting,
            expected_counter,
            angle,
            distance,
        ) in case.steps
        {
            let running = state
                .show_player_pan(
                    &PanInputs {
                        duration: case.duration,
                        requested_slot: case.requested_slot,
                        bound_position: case.position,
                        bound_angles: case.angles,
                    },
                    &mut pan,
                    &mut counter,
                    elapsed,
                )
                .unwrap();
            let context = format!("{} elapsed {elapsed}", case.name);
            assert_eq!(running, active, "{context}");
            assert_eq!(
                (state.selected.index(), state.index.index()),
                (record, index),
                "{context}"
            );
            assert_eq!(
                (state.scripted_active, state.scripted_nesting, counter),
                (scripted, nesting, expected_counter),
                "{context}"
            );
            let selected = &state.records[record];
            assert_eq!(
                (selected.angle_offset, selected.distance),
                (angle, distance),
                "{context}"
            );
        }
        // The real record survives the pan untouched.
        assert_eq!(state.records[0].angle_offset, [30, -20, 5], "{}", case.name);
        assert_eq!(
            state.records[0].position_offset,
            [100, -9000, 50],
            "{}",
            case.name
        );
    }
    // A negative duration completes at once and changes nothing, including the private word.
    let mut state = CameraState::new(&settings, actor(2), actor(3), 0).unwrap();
    let before = state.records[0].clone();
    let mut counter = 7;
    let running = state
        .show_player_pan(
            &PanInputs {
                duration: -1,
                requested_slot: None,
                bound_position: [0; 3],
                bound_angles: [0; 3],
            },
            &mut None,
            &mut counter,
            0,
        )
        .unwrap();
    assert!(!running);
    assert_eq!(counter, 7);
    assert_eq!(state.selected.index(), 0);
    assert_eq!(state.scripted_active, 0);
    assert_eq!(state.records[0].angle_offset, before.angle_offset);
}
