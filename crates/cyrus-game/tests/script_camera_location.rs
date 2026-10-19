//! The location camera's first and later invocations reproduce the original handler.
//!
//! Each case gives synthetic receiver attributes, pose and task arguments, and the camera state the
//! original left after its first invocation (selection, scripted words, request, offsets, published
//! pose) and after the next invocation. The scene has no obstacles and no terrain.
use anyhow::Result;
use cyrus_game::script::{ScriptActorId, camera::*};

struct NoGeometry;
impl CameraGeometry for NoGeometry {
    fn segment(
        &mut self,
        _: [i32; 3],
        _: [i32; 3],
        _: ScriptActorId,
    ) -> Result<Vec<cyrus_collision::collision_mesh::IntegerSegmentContact>> {
        Ok(vec![])
    }
    fn contacts(&mut self, _: [i32; 3], _: ScriptActorId) -> Result<Vec<[i32; 3]>> {
        Ok(vec![])
    }
    fn terrain_height(&mut self, _: [i32; 2]) -> Result<f32> {
        anyhow::bail!("terrain is disabled in these cases")
    }
}

#[derive(Debug, PartialEq)]
struct Record {
    position_offset: [i32; 3],
    angle_offset: [i32; 3],
    request: [i32; 3],
    target_offset: [i32; 3],
    distance: i32,
    flags: u8,
}

struct Case {
    name: &'static str,
    frame: VectorFrame,
    camera_move: u8,
    attributes: [u8; 6],
    arguments: [i32; 3],
    position: [i32; 3],
    angles: [u16; 3],
    // After the first invocation.
    selected: usize,
    index: usize,
    active: u16,
    nesting: u32,
    counter: u32,
    pose: [i32; 3],
    pose_angles: [i32; 3],
    scratch: Record,
    record_zero_flags: u8,
    // After the second invocation (elapsed one).
    later_active: u16,
    later_nesting: u32,
    later_counter: u32,
}

const CASES: &[Case] = &[
    Case {
        name: "absolute request",
        frame: VectorFrame::Actor,
        camera_move: 0,
        attributes: [130, 120, 0, 140, 128, 126],
        arguments: [3, -2, 5],
        position: [20000, 30000, 40000],
        angles: [10, 500, 20],
        selected: 2,
        index: 2,
        active: 1,
        nesting: 1,
        counter: 1,
        pose: [19475, 30410, 47279],
        pose_angles: [1854, 985, 0],
        scratch: Record {
            position_offset: [6500, 500, -500],
            angle_offset: [30, -20, 5],
            request: [19475, 30410, 47279],
            target_offset: [1500, -3500, 0],
            distance: 321,
            flags: 0x68,
        },
        record_zero_flags: 0x41,
        later_active: 0,
        later_nesting: 0,
        later_counter: 2,
    },
    Case {
        name: "relative request",
        frame: VectorFrame::Actor,
        camera_move: 1,
        attributes: [130, 120, 0, 140, 128, 126],
        arguments: [3, -2, 5],
        position: [20000, 30000, 40000],
        angles: [10, 500, 20],
        selected: 2,
        index: 2,
        active: 1,
        nesting: 1,
        counter: 1,
        pose: [19475, 30410, 47279],
        pose_angles: [1854, 985, 0],
        scratch: Record {
            position_offset: [6500, 500, -500],
            angle_offset: [30, -20, 5],
            request: [-525, 410, 7279],
            target_offset: [1500, -3500, 0],
            distance: 321,
            flags: 0x64,
        },
        record_zero_flags: 0x41,
        later_active: 0,
        later_nesting: 0,
        later_counter: 2,
    },
    Case {
        name: "orbit request",
        frame: VectorFrame::Actor,
        camera_move: 2,
        attributes: [130, 120, 0, 140, 128, 126],
        arguments: [3, -2, 5],
        position: [20000, 30000, 40000],
        angles: [10, 500, 20],
        selected: 2,
        index: 2,
        active: 1,
        nesting: 1,
        counter: 1,
        pose: [19506, 30427, 47256],
        pose_angles: [1853, 987, 0],
        scratch: Record {
            position_offset: [6500, 500, -500],
            angle_offset: [115, 1207, 20],
            request: [0, 0, 0],
            target_offset: [1500, -3500, 0],
            distance: 6,
            flags: 0x60,
        },
        record_zero_flags: 0x41,
        later_active: 0,
        later_nesting: 0,
        later_counter: 2,
    },
    Case {
        name: "no request mode",
        frame: VectorFrame::Actor,
        camera_move: 3,
        attributes: [130, 120, 0, 140, 128, 126],
        arguments: [3, -2, 5],
        position: [20000, 30000, 40000],
        angles: [10, 500, 20],
        selected: 2,
        index: 2,
        active: 1,
        nesting: 1,
        counter: 1,
        pose: [102707, 25874, 48448],
        pose_angles: [2, 1510, 0],
        scratch: Record {
            position_offset: [6500, 500, -500],
            angle_offset: [30, -20, 5],
            request: [0, 0, 0],
            target_offset: [1500, -3500, 0],
            distance: 321,
            flags: 0x60,
        },
        record_zero_flags: 0x41,
        later_active: 0,
        later_nesting: 0,
        later_counter: 2,
    },
    Case {
        name: "zero attributes",
        frame: VectorFrame::Actor,
        camera_move: 0,
        attributes: [0, 0, 0, 0, 0, 0],
        arguments: [-100, 40, 7],
        position: [-5000, 7000, 9000],
        angles: [1800, 300, 2000],
        selected: 2,
        index: 2,
        active: 1,
        nesting: 1,
        counter: 1,
        pose: [-12837, 18292, -14969],
        pose_angles: [1911, 102, 0],
        scratch: Record {
            position_offset: [0, 0, 0],
            angle_offset: [30, -20, 5],
            request: [-12837, 18292, -14969],
            target_offset: [0, 0, 0],
            distance: 321,
            flags: 0x68,
        },
        record_zero_flags: 0x41,
        later_active: 0,
        later_nesting: 0,
        later_counter: 2,
    },
    Case {
        name: "high attributes",
        frame: VectorFrame::Actor,
        camera_move: 1,
        attributes: [255, 1, 255, 1, 255, 127],
        arguments: [30, 30, -30],
        position: [9000, -3000, 800],
        angles: [200, 1500, 64],
        selected: 2,
        index: 2,
        active: 1,
        nesting: 1,
        counter: 1,
        pose: [44338, 50077, 65541],
        pose_angles: [1808, 1068, 0],
        scratch: Record {
            position_offset: [-63000, 64000, 0],
            angle_offset: [30, -20, 5],
            request: [35338, 53077, 64741],
            target_offset: [64000, -63000, 64000],
            distance: 321,
            flags: 0x64,
        },
        record_zero_flags: 0x41,
        later_active: 0,
        later_nesting: 0,
        later_counter: 2,
    },
    Case {
        name: "large orbit",
        frame: VectorFrame::Actor,
        camera_move: 2,
        attributes: [140, 100, 127, 90, 150, 127],
        arguments: [2000, -1500, 3000],
        position: [12000, 12000, -12000],
        angles: [600, 1900, 5],
        selected: 2,
        index: 2,
        active: 1,
        nesting: 1,
        counter: 1,
        pose: [214101, -629911, -732530],
        pose_angles: [235, 1959, 0],
        scratch: Record {
            position_offset: [-18500, 11500, 0],
            angle_offset: [826, 1808, 5],
            request: [0, 0, 0],
            target_offset: [6500, -13500, 0],
            distance: 3905,
            flags: 0x60,
        },
        record_zero_flags: 0x41,
        later_active: 0,
        later_nesting: 0,
        later_counter: 2,
    },
    Case {
        name: "orbit zero vector",
        frame: VectorFrame::Actor,
        camera_move: 2,
        attributes: [130, 120, 0, 140, 128, 126],
        arguments: [0, 0, 0],
        position: [20000, 30000, 40000],
        angles: [10, 500, 20],
        selected: 2,
        index: 2,
        active: 1,
        nesting: 1,
        counter: 1,
        pose: [20710, 50625, 46435],
        pose_angles: [1598, 1060, 0],
        scratch: Record {
            position_offset: [6500, 500, -500],
            angle_offset: [1546, 1524, 20],
            request: [0, 0, 0],
            target_offset: [1500, -3500, 0],
            distance: 77,
            flags: 0x60,
        },
        record_zero_flags: 0x41,
        later_active: 0,
        later_nesting: 0,
        later_counter: 2,
    },
    Case {
        name: "world absolute request",
        frame: VectorFrame::World,
        camera_move: 0,
        attributes: [130, 120, 0, 140, 128, 126],
        arguments: [3, -2, 5],
        position: [20000, 30000, 40000],
        angles: [10, 500, 20],
        selected: 2,
        index: 2,
        active: 1,
        nesting: 1,
        counter: 1,
        pose: [21478, 30401, 47715],
        pose_angles: [1868, 1092, 0],
        scratch: Record {
            position_offset: [6500, 500, -500],
            angle_offset: [30, -20, 5],
            request: [21478, 30401, 47715],
            target_offset: [1500, -3500, 0],
            distance: 321,
            flags: 0x68,
        },
        record_zero_flags: 0x41,
        later_active: 0,
        later_nesting: 0,
        later_counter: 2,
    },
    Case {
        name: "world relative request",
        frame: VectorFrame::World,
        camera_move: 1,
        attributes: [130, 120, 0, 140, 128, 126],
        arguments: [3, -2, 5],
        position: [20000, 30000, 40000],
        angles: [10, 500, 20],
        selected: 2,
        index: 2,
        active: 1,
        nesting: 1,
        counter: 1,
        pose: [21478, 30401, 47715],
        pose_angles: [1868, 1092, 0],
        scratch: Record {
            position_offset: [6500, 500, -500],
            angle_offset: [30, -20, 5],
            request: [1478, 401, 7715],
            target_offset: [1500, -3500, 0],
            distance: 321,
            flags: 0x64,
        },
        record_zero_flags: 0x41,
        later_active: 0,
        later_nesting: 0,
        later_counter: 2,
    },
    Case {
        name: "world orbit request",
        frame: VectorFrame::World,
        camera_move: 2,
        attributes: [130, 120, 0, 140, 128, 126],
        arguments: [30, -20, 50],
        position: [20000, 30000, 40000],
        angles: [10, 500, 20],
        selected: 2,
        index: 2,
        active: 1,
        nesting: 1,
        counter: 1,
        pose: [28310, 25878, 59114],
        pose_angles: [12, 1165, 0],
        scratch: Record {
            position_offset: [6500, 500, -500],
            angle_offset: [117, 1700, 20],
            request: [0, 0, 0],
            target_offset: [1500, -3500, 0],
            distance: 61,
            flags: 0x60,
        },
        record_zero_flags: 0x41,
        later_active: 0,
        later_nesting: 0,
        later_counter: 2,
    },
    Case {
        name: "world no request mode",
        frame: VectorFrame::World,
        camera_move: 3,
        attributes: [130, 120, 0, 140, 128, 126],
        arguments: [3, -2, 5],
        position: [20000, 30000, 40000],
        angles: [10, 500, 20],
        selected: 2,
        index: 2,
        active: 1,
        nesting: 1,
        counter: 1,
        pose: [102707, 25874, 48448],
        pose_angles: [2, 1510, 0],
        scratch: Record {
            position_offset: [6500, 500, -500],
            angle_offset: [30, -20, 5],
            request: [0, 0, 0],
            target_offset: [1500, -3500, 0],
            distance: 321,
            flags: 0x60,
        },
        record_zero_flags: 0x41,
        later_active: 0,
        later_nesting: 0,
        later_counter: 2,
    },
    Case {
        name: "world high attributes",
        frame: VectorFrame::World,
        camera_move: 1,
        attributes: [255, 1, 255, 1, 255, 127],
        arguments: [30, 30, -30],
        position: [9000, -3000, 800],
        angles: [200, 1500, 64],
        selected: 2,
        index: 2,
        active: 1,
        nesting: 1,
        counter: 1,
        pose: [53756, 45951, 63739],
        pose_angles: [1813, 1090, 0],
        scratch: Record {
            position_offset: [-63000, 64000, 0],
            angle_offset: [30, -20, 5],
            request: [44756, 48951, 62939],
            target_offset: [64000, -63000, 64000],
            distance: 321,
            flags: 0x64,
        },
        record_zero_flags: 0x41,
        later_active: 0,
        later_nesting: 0,
        later_counter: 2,
    },
];

fn actor(index: usize) -> ScriptActorId {
    ScriptActorId {
        generation: 1,
        index,
    }
}

#[test]
fn location_camera_reproduces_the_original_handler() {
    let hits = CameraHitSelection {
        minimum_position: [0; 3],
        maximum_position: [0; 3],
        minimum_normal: [0; 3],
        maximum_normal: [0; 3],
    };
    for case in CASES {
        let mut state =
            CameraState::new(&CameraSettings::default(), actor(2), actor(3), 0).unwrap();
        let record = &mut state.records[0];
        record.position_offset = [100, -9000, 50];
        record.angle_offset = [30, -20, 5];
        record.target_offset = [-5000, -12000, 7];
        record.distance = 321;
        record.minimum_distance = 77;
        record.maximum_movement = 800;
        record.flags = 0x40;
        let zero = state.records[0].clone();
        for slot in 1..9 {
            state.records[slot] = zero.clone();
        }
        state.current = CameraPose {
            position: [4300, 12000, -9500],
            angles: [3, 2000, 0],
        };
        state.previous = CameraPose {
            position: [4200, 11900, -9400],
            angles: [3, 1990, 0],
        };
        let inputs = LocationInputs {
            frame: case.frame,
            actor: actor(4),
            camera_move: case.camera_move,
            attributes: case.attributes,
            arguments: case.arguments,
            bound_position: case.position,
            bound_angles: case.angles,
        };
        let mut update = |state: &mut CameraState| -> Result<()> {
            let next = state.prepare_pose(
                &hits,
                &CameraUpdateInputs {
                    bound_position: case.position,
                    bound_angles: case.angles,
                    movement: MovementLimits {
                        budget_increment: 5000,
                        vertical: -4000,
                        frame_scale: 256,
                        selected_player_special_state: false,
                    },
                    body_radius: 10.0,
                    body_clearance: 5.0,
                    collision_inhibited: false,
                    terrain_enabled: false,
                },
                &mut NoGeometry,
            )?;
            *state = next.state;
            Ok(())
        };
        let mut counter = 0u32;
        let running = state
            .show_location(&inputs, &mut counter, 0, &mut update)
            .unwrap_or_else(|error| panic!("{}: {error:#}", case.name));
        assert!(running, "{}: first invocation stays active", case.name);
        assert_eq!(
            (
                state.selected.index(),
                state.index.index(),
                state.scripted_active,
                state.scripted_nesting,
                counter
            ),
            (
                case.selected,
                case.index,
                case.active,
                case.nesting,
                case.counter
            ),
            "{}: selection and scripted words",
            case.name
        );
        assert_eq!(
            (state.current.position, state.current.angles),
            (case.pose, case.pose_angles),
            "{}: published pose",
            case.name
        );
        let scratch = &state.records[state.selected.index()];
        assert_eq!(
            Record {
                position_offset: scratch.position_offset,
                angle_offset: scratch.angle_offset,
                request: scratch.request,
                target_offset: scratch.target_offset,
                distance: scratch.distance,
                flags: scratch.flags,
            },
            case.scratch,
            "{}: bound record",
            case.name
        );
        assert_eq!(scratch.actor, actor(4), "{}: bound actor", case.name);
        assert_eq!(
            state.records[0].flags, case.record_zero_flags,
            "{}: record zero flags",
            case.name
        );
        let running = state
            .show_location(&inputs, &mut counter, 1, &mut update)
            .unwrap_or_else(|error| panic!("{}: {error:#}", case.name));
        assert!(!running, "{}: later invocation completes", case.name);
        assert_eq!(
            (state.scripted_active, state.scripted_nesting, counter),
            (case.later_active, case.later_nesting, case.later_counter),
            "{}: release",
            case.name
        );
    }
}

#[test]
fn location_camera_leaves_the_state_unchanged_when_the_update_fails() {
    let mut state = CameraState::new(&CameraSettings::default(), actor(2), actor(3), 0).unwrap();
    let before = state.clone();
    let inputs = LocationInputs {
        frame: VectorFrame::Actor,
        actor: actor(4),
        camera_move: 0,
        attributes: [130, 120, 0, 140, 128, 126],
        arguments: [3, -2, 5],
        bound_position: [20000, 30000, 40000],
        bound_angles: [10, 500, 20],
    };
    let mut counter = 0u32;
    let error = state
        .show_location(&inputs, &mut counter, 0, &mut |_: &mut CameraState| {
            anyhow::bail!("camera update service failed")
        })
        .unwrap_err();
    assert!(format!("{error:#}").contains("camera update service failed"));
    assert_eq!(counter, 0);
    assert_eq!(state.selected, before.selected);
    assert_eq!(state.scripted_active, 0);
    assert_eq!(state.records[0].flags, before.records[0].flags);
}
