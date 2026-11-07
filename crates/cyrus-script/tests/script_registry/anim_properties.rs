//! `anim.*` properties are fields of the actor's playback record: scripts read and assign them raw
//! and the production playback advances from the assigned values.
use super::*;
use cyrus_assets::animation::Animations;
use cyrus_script::script::{ScriptInputs, ScriptStage};

fn int(value: i32) -> Value {
    Value::Integer { value }
}

const REFERENCES: [Property; 11] = [
    Property::AnimNext,
    Property::AnimNextSpeed,
    Property::AnimFrame,
    Property::AnimSpeed,
    Property::AnimSpeedCount,
    Property::AnimStopFlag,
    Property::AnimHaltFlag,
    Property::AnimHoldFlag,
    Property::AnimLoopCount,
    Property::AnimRewindFlag,
    Property::AnimRealFrame,
];

fn reference(property: Property) -> u16 {
    REFERENCES.iter().position(|p| *p == property).unwrap() as u16
}

fn me(property: Property) -> Place {
    Place::Property {
        object: Object::Me,
        reference: reference(property),
    }
}

fn assign(destination: Place, first: Value) -> Operation {
    Operation::Assign {
        destination,
        formula: Formula {
            first,
            rest: vec![],
        },
    }
}

fn write(property: Property, value: i32) -> Operation {
    assign(me(property), int(value))
}

/// Property place on the actor whose namespace is `gremlin` (script string 1).
fn gremlin(property: Property) -> Place {
    Place::Property {
        object: Object::Named { string: 1 },
        reference: reference(property),
    }
}

/// Reads each property into the local of the same position, from local zero.
fn reads(properties: &[Property]) -> Vec<Operation> {
    properties
        .iter()
        .enumerate()
        .map(|(local, property)| {
            assign(
                Place::Local { index: local as u8 },
                Value::Read {
                    place: me(*property),
                },
            )
        })
        .collect()
}

/// Group 7 has frame countdown 3 and four frames followed by its end.
fn animation_data(scripts: &Scripts, actor: &str) -> Animations {
    let mut data = Animations {
        version: 2,
        world: scripts.world,
        metadata: serde_json::json!({"script_content_id": scripts.content_id,
            "markers": [], "walk_node_maps": [],
            "mpob_objects": [{"script_name": "LOGIC", "object_type": 1}, {"script_name": "LOGIC", "object_type": 1}],
            "actors": [{
            "script_name": actor,
            "model_load_path": "animation_table",
            "raan_entries": [{"file_path": "LOGIC.3D", "frame_count": 30}],
            "animation_groups": [
                {"group_index": 7, "anim_id": 3, "anim_type": 0, "commands": [
                    {"opcode": 0, "frame_reference": 10}, {"opcode": 0, "frame_reference": 11},
                    {"opcode": 0, "frame_reference": 12}, {"opcode": 0, "frame_reference": 13},
                    {"opcode": 1}]},
                {"group_index": 9, "anim_id": 5, "anim_type": 0, "commands": [
                    {"opcode": 0, "frame_reference": 20}, {"opcode": 1}]},
            ]
        }]}),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    };
    data.models.insert(
        "LOGIC".into(),
        cyrus_assets::animation::AnimationModel {
            radius: Some(17.0),
            base_positions: vec![],
            base_normals: vec![],
            faces: vec![],
            corner_normals: vec![],
            encoding: "animated_i16".into(),
            positions: vec![vec![[0.0, 0.0, 0.0], [0.0, 1.0, 0.0]]; 30],
            normals: vec![vec![]; 30],
        },
    );
    data
}

struct Fixture {
    world: ScriptWorld,
    id: cyrus_script::script::ScriptActorId,
}

impl Fixture {
    /// Actor 0 runs `ops` and owns the playback of group 7, already started by one tick. With
    /// `neighbour` the playback belongs to the standalone `gremlin` actor instead, which actor 0
    /// reaches by name.
    fn build(ops: Vec<Operation>, neighbour: bool) -> Self {
        let mut scripts = wait_cache(ops);
        scripts.definition.references = REFERENCES.to_vec();
        scripts.definition.functions[1] = Function {
            name: Native::WaitAnimFrame,
            param_count: 2,
            kind: CallKind::Task,
        };
        let logic = scripts.actors.get_mut("LOGIC").unwrap();
        logic.program.variables = vec![0; 10];
        logic.instance_variables = vec![vec![0; 10]; 2];
        logic.program.strings = vec!["logic".into(), "gremlin".into()];
        scripts.content_id = scripts.content_identity().unwrap();
        if neighbour {
            add_standalone_receivers(&mut scripts);
            for name in ["CYRUS", "GREMLIN", "CAMERA"] {
                scripts.actors.get_mut(name).unwrap().instance_variables = vec![vec![0; 10]];
            }
            scripts.content_id = scripts.content_identity().unwrap();
        }
        let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
        let owner = if neighbour {
            world
                .actors()
                .iter()
                .position(|actor| actor.name() == "GREMLIN")
                .unwrap()
        } else {
            0
        };
        let id = world.id(owner).unwrap();
        let data = animation_data(&scripts, world.actors()[owner].name());
        world
            .bind_animation(id, &data, AnimationGroup::IDLE_LEDGE)
            .unwrap();
        let mut fixture = Self { world, id };
        fixture.animate(1);
        fixture
    }

    fn new(ops: Vec<Operation>) -> Self {
        Self::build(ops, false)
    }

    fn animate(&mut self, ticks: u32) {
        self.world
            .advance(
                self.id,
                ScriptStage::Animation,
                ScriptInputs {
                    nominal_ticks: Some(ticks),
                    ..Default::default()
                },
                None,
                None,
                0,
            )
            .unwrap();
    }

    fn body(&mut self) {
        let caller = self.world.id(0).unwrap();
        self.world
            .advance(
                caller,
                ScriptStage::Body(Thread::Main),
                ScriptInputs {
                    nominal_ticks: Some(0),
                    frame_scale: Some(256),
                    ..Default::default()
                },
                None,
                None,
                20,
            )
            .unwrap();
    }

    fn locals(&self) -> Vec<i32> {
        self.world.actors()[0]
            .runtime()
            .unwrap()
            .variables()
            .iter()
            .map(|value| value.integer().unwrap())
            .collect()
    }

    fn frame(&self) -> Option<u32> {
        self.world.animation_frame(self.id).unwrap()
    }
}

/// Locals after the fields below are read, in this order.
const READ: [Property; 10] = [
    Property::AnimNext,
    Property::AnimFrame,
    Property::AnimSpeed,
    Property::AnimSpeedCount,
    Property::AnimStopFlag,
    Property::AnimHaltFlag,
    Property::AnimHoldFlag,
    Property::AnimLoopCount,
    Property::AnimRewindFlag,
    Property::AnimRealFrame,
];

/// A script that reads every field, then applies `writes`: the first run shows the state before
/// the writes, a later one the state production playback left them in.
fn script(writes: &[(Property, i32)]) -> Vec<Operation> {
    let mut ops = reads(&READ);
    ops.extend(
        writes
            .iter()
            .map(|(property, value)| write(*property, *value)),
    );
    ops.push(Operation::End { target: 0 });
    ops
}

/// Runs the script once, advances playback by each tick count, and runs it again.
fn after(writes: &[(Property, i32)], ticks: &[u32]) -> (Fixture, Vec<i32>) {
    let mut fixture = Fixture::new(script(writes));
    fixture.body();
    for &count in ticks {
        fixture.animate(count);
    }
    fixture.body();
    let locals = fixture.locals();
    (fixture, locals)
}

#[test]
fn a_started_group_shows_its_countdown_and_cursor_through_the_record() {
    let mut fixture = Fixture::new(script(&[]));
    fixture.body();
    // No pending group (-1), cursor 0, reload 3, countdown 2 after the start tick, the other
    // bytes clear and no frame has been counted.
    assert_eq!(fixture.locals(), [-1, 0, 3, 2, 0, 0, 0, 0, 0, 0]);
    assert_eq!(fixture.frame(), Some(10));
}

#[test]
fn an_expired_countdown_written_by_a_script_advances_one_command_and_counts_the_frame() {
    // Zero countdown with a tick: the cursor steps to the next frame command, the progress byte
    // counts it, the countdown reloads to the speed and drops once.
    let (fixture, locals) = after(&[(Property::AnimSpeedCount, 0)], &[1]);
    assert_eq!(locals, [-1, 1, 3, 2, 0, 0, 0, 0, 0, 1]);
    assert_eq!(fixture.frame(), Some(11));
    // Writing the cursor too moves playback to that command instead.
    let (fixture, locals) = after(
        &[(Property::AnimSpeedCount, 0), (Property::AnimFrame, 2)],
        &[1],
    );
    assert_eq!(locals[1], 3);
    assert_eq!(fixture.frame(), Some(13));
    // Without the write the nonzero countdown only drops.
    let (fixture, locals) = after(&[], &[1]);
    assert_eq!(locals[1..4], [0, 3, 1]);
    assert_eq!(fixture.frame(), Some(10));
}

#[test]
fn the_wait_byte_holds_or_counts_down_before_playback_resumes() {
    // 255 holds indefinitely: no tick moves countdown, cursor or progress.
    let (fixture, locals) = after(&[(Property::AnimHoldFlag, 255)], &[5, 5]);
    assert_eq!(locals[3], 2);
    assert_eq!(locals[6], 255);
    assert_eq!(fixture.frame(), Some(10));
    // A counted wait of 2 spends one tick per update and, once spent, lets the countdown drop.
    let (_, locals) = after(&[(Property::AnimHoldFlag, 2)], &[1]);
    assert_eq!(locals[3..7], [2, 0, 0, 1]);
    let (_, locals) = after(&[(Property::AnimHoldFlag, 2)], &[1, 1]);
    assert_eq!(locals[3..7], [1, 0, 0, 0]);
}

#[test]
fn rewind_steps_the_cursor_and_progress_back() {
    // Reverse playback with an expired countdown: the cursor and the wrapping progress byte step
    // back, so cursor 2 shows command 1.
    let (fixture, locals) = after(
        &[
            (Property::AnimRewindFlag, 1),
            (Property::AnimFrame, 2),
            (Property::AnimSpeedCount, 0),
        ],
        &[1],
    );
    assert_eq!((locals[1], locals[8], locals[9]), (1, 1, 255));
    assert_eq!(fixture.frame(), Some(11));
}

#[test]
fn the_remaining_bytes_and_the_pending_request_read_back_what_was_written() {
    let writes = [
        (Property::AnimStopFlag, 1),
        (Property::AnimHaltFlag, 1),
        (Property::AnimLoopCount, 200),
        (Property::AnimSpeed, 9),
        (Property::AnimRealFrame, 250),
        (Property::AnimNext, 9),
    ];
    let (_, locals) = after(&writes, &[]);
    // Bytes keep eight bits; the pending group is the one named.
    assert_eq!(locals, [9, 0, 9, 2, 1, 1, 0, 200, 0, 250]);
}

fn run_body(fixture: &mut Fixture) -> anyhow::Result<()> {
    let caller = fixture.world.id(0)?;
    fixture.world.advance(
        caller,
        ScriptStage::Body(Thread::Main),
        ScriptInputs {
            nominal_ticks: Some(0),
            frame_scale: Some(256),
            ..Default::default()
        },
        None,
        None,
        20,
    )
}

#[test]
fn a_pending_requests_speed_is_its_modifier_plus_the_groups_own_byte() {
    // Group 9's own byte is 5. Naming it as pending leaves modifier zero; writing the speed stores
    // the modifier that leaves the written value.
    let mut ops = vec![write(Property::AnimNext, 9)];
    ops.extend(reads(&[Property::AnimNextSpeed]));
    ops.extend([
        write(Property::AnimNextSpeed, 12),
        assign(
            Place::Local { index: 1 },
            Value::Read {
                place: me(Property::AnimNextSpeed),
            },
        ),
        Operation::End { target: 0 },
    ]);
    let mut fixture = Fixture::new(ops);
    run_body(&mut fixture).unwrap();
    assert_eq!(fixture.locals()[..2], [5, 12]);
    // Without a pending request the byte is not modelled: an explicit error, never a zero.
    let mut ops = reads(&[Property::AnimNextSpeed]);
    ops.push(Operation::End { target: 0 });
    let mut fixture = Fixture::new(ops);
    let error = run_body(&mut fixture).unwrap_err();
    assert!(
        format!("{error:#}").contains("no pending request"),
        "{error:#}"
    );
}

#[test]
fn the_frame_wait_completes_from_a_progress_byte_a_script_wrote() {
    // WaitAnimFrame(7, 5) completes once the current group's progress byte reaches five.
    let wait_for_five = |progress: i32| {
        let mut fixture = Fixture::new(vec![
            write(Property::AnimRealFrame, progress),
            wait(
                1,
                Native::WaitAnimFrame,
                CallKind::Task,
                vec![int(7), int(5)],
            ),
            set_flag(7),
            Operation::End { target: 0 },
        ]);
        run_body(&mut fixture).unwrap();
        let caller = fixture.world.id(0).unwrap();
        let inputs = ScriptInputs {
            nominal_ticks: Some(0),
            frame_scale: Some(256),
            ..Default::default()
        };
        fixture
            .world
            .advance(caller, ScriptStage::Tasks, inputs, None, None, 0)
            .unwrap();
        fixture.world.actors()[0].runtime().unwrap().active_tasks()
    };
    assert_eq!(wait_for_five(5), 0, "progress five completes the wait");
    assert_eq!(wait_for_five(4), 1, "progress four keeps waiting");
}

#[test]
fn another_actors_playback_record_is_reached_by_name() {
    let mut ops = vec![
        assign(gremlin(Property::AnimRealFrame), int(7)),
        assign(
            Place::Local { index: 0 },
            Value::Read {
                place: gremlin(Property::AnimRealFrame),
            },
        ),
        assign(
            Place::Local { index: 1 },
            Value::Read {
                place: gremlin(Property::AnimSpeed),
            },
        ),
        assign(
            Place::Local { index: 2 },
            Value::Read {
                place: me(Property::AnimRealFrame),
            },
        ),
    ];
    ops.push(Operation::End { target: 0 });
    let mut fixture = Fixture::build(ops, true);
    // Actor 0 has no animation association, so its own fields read zero.
    run_body(&mut fixture).unwrap();
    assert_eq!(fixture.locals()[..3], [7, 3, 0]);
    // The write changed the neighbour's own playback: ticks no longer start from progress zero.
    fixture.animate(1);
    run_body(&mut fixture).unwrap();
    assert_eq!(fixture.locals()[..3], [7, 3, 0]);
    // Writing a field of an actor with no animation is an explicit error.
    let mut fixture = Fixture::build(
        vec![
            write(Property::AnimRealFrame, 1),
            Operation::End { target: 0 },
        ],
        true,
    );
    let error = run_body(&mut fixture).unwrap_err();
    assert!(
        format!("{error:#}").contains("has no animation"),
        "{error:#}"
    );
}
//

#[test]
fn a_cursor_outside_the_group_fails_playback_stickily_and_the_record_stops_reading() {
    let mut fixture = Fixture::new(script(&[
        (Property::AnimFrame, 99),
        (Property::AnimSpeedCount, 0),
    ]));
    fixture.body();
    let error = fixture
        .world
        .advance(
            fixture.id,
            ScriptStage::Animation,
            ScriptInputs {
                nominal_ticks: Some(1),
                ..Default::default()
            },
            None,
            None,
            0,
        )
        .unwrap_err();
    let message = format!("{error:#}");
    assert!(message.contains("command"), "{message}");
    let failure = fixture.world.actors()[fixture.id.index]
        .animation_failure()
        .map(str::to_owned)
        .expect("the failure is retained");
    // The failure is sticky: later reads of the record report it instead of stale values.
    let error = run_body(&mut fixture).unwrap_err();
    assert!(format!("{error:#}").contains(&failure), "{error:#}");
}
