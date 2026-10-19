//! `showObjWorld` reproduces the original handler.
//!
//! Each case gives synthetic receiver attributes, pose and a world position, and the camera state
//! the original left after its first invocation (selection, scripted words, request, offsets,
//! published pose) and after the next invocation. The scene has no obstacles and no terrain.
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
    camera_move: u8,
    attributes: [u8; 6],
    arguments: [i32; 3],
    /// The actor's own position.
    position: [i32; 3],
    /// The position and angles the record binds to.
    bound_position: [i32; 3],
    bound_angles: [u16; 3],
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
        camera_move: 0,
        attributes: [130, 120, 0, 140, 128, 126],
        arguments: [30, -20, 50],
        position: [20000, 30000, 40000],
        bound_position: [20000, 30000, 40000],
        bound_angles: [10, 500, 20],
        selected: 2,
        index: 2,
        active: 1,
        nesting: 1,
        counter: 1,
        pose: [-17920, 2298880, 0],
        pose_angles: [1543, 241, 0],
        scratch: Record {
            position_offset: [6500, 500, -500],
            angle_offset: [30, -20, 5],
            request: [-17920, 2298880, 0],
            target_offset: [1500, -3500, 0],
            distance: 321,
            flags: 0x68,
        },
        record_zero_flags: 0x61,
        later_active: 0,
        later_nesting: 0,
        later_counter: 2,
    },
    Case {
        name: "relative request",
        camera_move: 1,
        attributes: [130, 120, 0, 140, 128, 126],
        arguments: [30, -20, 50],
        position: [20000, 30000, 40000],
        bound_position: [20000, 30000, 40000],
        bound_angles: [10, 500, 20],
        selected: 2,
        index: 2,
        active: 1,
        nesting: 1,
        counter: 1,
        pose: [-17920, 2298880, 0],
        pose_angles: [1543, 241, 0],
        scratch: Record {
            position_offset: [6500, 500, -500],
            angle_offset: [30, -20, 5],
            request: [-37920, 2268880, -40000],
            target_offset: [1500, -3500, 0],
            distance: 321,
            flags: 0x64,
        },
        record_zero_flags: 0x61,
        later_active: 0,
        later_nesting: 0,
        later_counter: 2,
    },
    Case {
        name: "orbit request",
        camera_move: 2,
        attributes: [130, 120, 0, 140, 128, 126],
        arguments: [30, -20, 50],
        position: [20000, 30000, 40000],
        bound_position: [20000, 30000, 40000],
        bound_angles: [10, 500, 20],
        selected: 2,
        index: 2,
        active: 1,
        nesting: 1,
        counter: 1,
        pose: [-12786, 2299830, 11036],
        pose_angles: [1542, 268, 0],
        scratch: Record {
            position_offset: [6500, 500, -500],
            angle_offset: [1553, 747, 20],
            request: [0, 0, 0],
            target_offset: [1500, -3500, 0],
            distance: 8865,
            flags: 0x60,
        },
        record_zero_flags: 0x61,
        later_active: 0,
        later_nesting: 0,
        later_counter: 2,
    },
    Case {
        name: "no request mode",
        camera_move: 3,
        attributes: [130, 120, 0, 140, 128, 126],
        arguments: [30, -20, 50],
        position: [20000, 30000, 40000],
        bound_position: [20000, 30000, 40000],
        bound_angles: [10, 500, 20],
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
        record_zero_flags: 0x61,
        later_active: 0,
        later_nesting: 0,
        later_counter: 2,
    },
    Case {
        name: "relative request on a slave actor",
        camera_move: 1,
        attributes: [130, 120, 0, 140, 128, 126],
        arguments: [30, -20, 50],
        position: [20000, 30000, 40000],
        bound_position: [-7000, 2500, 9100],
        bound_angles: [300, 1700, 40],
        selected: 2,
        index: 2,
        active: 1,
        nesting: 1,
        counter: 1,
        pose: [-17920, 2298880, 0],
        pose_angles: [1537, 325, 0],
        scratch: Record {
            position_offset: [6500, 500, -500],
            angle_offset: [30, -20, 5],
            request: [-10920, 2296380, -9100],
            target_offset: [1500, -3500, 0],
            distance: 321,
            flags: 0x64,
        },
        record_zero_flags: 0x61,
        later_active: 0,
        later_nesting: 0,
        later_counter: 2,
    },
    Case {
        name: "orbit request on a slave actor",
        camera_move: 2,
        attributes: [130, 120, 0, 140, 128, 126],
        arguments: [-300, 40, 700],
        position: [20000, 30000, 40000],
        bound_position: [-7000, 2500, 9100],
        bound_angles: [300, 1700, 40],
        selected: 2,
        index: 2,
        active: 1,
        nesting: 1,
        counter: 1,
        pose: [-120240, 2288416, 125196],
        pose_angles: [1558, 779, 0],
        scratch: Record {
            position_offset: [6500, 500, -500],
            angle_offset: [1860, 426, 40],
            request: [0, 0, 0],
            target_offset: [1500, -3500, 0],
            distance: 8949,
            flags: 0x60,
        },
        record_zero_flags: 0x61,
        later_active: 0,
        later_nesting: 0,
        later_counter: 2,
    },
    Case {
        name: "high attributes",
        camera_move: 0,
        attributes: [255, 1, 255, 1, 255, 127],
        arguments: [-3000, 300, 8000],
        position: [9000, -3000, 800],
        bound_position: [9000, -3000, 800],
        bound_angles: [200, 1500, 64],
        selected: 2,
        index: 2,
        active: 1,
        nesting: 1,
        counter: 1,
        pose: [-793600, 2380800, 2035200],
        pose_angles: [1778, 904, 0],
        scratch: Record {
            position_offset: [-63000, 64000, 0],
            angle_offset: [30, -20, 5],
            request: [-793600, 2380800, 2035200],
            target_offset: [64000, -63000, 64000],
            distance: 321,
            flags: 0x68,
        },
        record_zero_flags: 0x61,
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
fn world_location_reproduces_the_original_handler() {
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
        let inputs = WorldLocationInputs {
            actor: actor(4),
            camera_move: case.camera_move,
            attributes: case.attributes,
            position: case.arguments,
            bound_position: case.bound_position,
            actor_position: case.position,
            bound_angles: case.bound_angles,
        };
        let mut update = |state: &mut CameraState| -> Result<()> {
            let next = state.prepare_pose(
                &hits,
                &CameraUpdateInputs {
                    bound_position: case.bound_position,
                    bound_angles: case.bound_angles,
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
            .show_world_location(&inputs, &mut counter, 0, &mut update)
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
            .show_world_location(&inputs, &mut counter, 1, &mut update)
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
