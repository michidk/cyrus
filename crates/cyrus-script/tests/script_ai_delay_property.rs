//! Receiver-owned AI delay retains unsigned property values through public snapshots.
use cyrus_assets::script::PlacementType;
use cyrus_script::script::ScriptWorld;
use cyrus_soup::program::*;
use std::collections::BTreeMap;

mod support;
use support::advance;
use support::counter_scripts;

#[test]
fn ai_delay_routes_unsigned_low_bytes_without_changing_adjacent_or_other_control() {
    for (value, expected) in [
        (0, 0),
        (6, 6),
        (8, 8),
        (128, 128),
        (207, 207),
        (-1, 255),
        (264, 8),
        (256, 0),
    ] {
        for (object, receiver) in [(Object::Me, 1), (Object::Named { string: 0 }, 0)] {
            let scripts = counter_scripts(cyrus_soup::Property::AiDelay, object, value, true);
            let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
            let mut before =
                serde_json::to_value(world.actors()[receiver].control_state().unwrap()).unwrap();
            advance(&mut world, 1).unwrap();
            assert_eq!(world.actors()[1].runtime().unwrap().variables(), [expected]);
            assert_eq!(world.flags(), [73]);
            before["ai_delay"] = expected.into();
            assert_eq!(
                serde_json::to_value(world.actors()[receiver].control_state().unwrap()).unwrap(),
                before
            );
            assert_eq!(
                world.actors()[1 - receiver]
                    .control_state()
                    .unwrap()
                    .ai_delay,
                0
            );
        }
    }
}

#[test]
fn ai_delay_allocation_zero_and_missing_owner_receiver_failures_are_distinct() {
    let scripts = counter_scripts(cyrus_soup::Property::AiDelay, Object::Me, 99, false);
    let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
    advance(&mut world, 0).unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().variables(), [0]);
    for write in [false, true] {
        for missing_receiver in [false, true] {
            let mut scripts = counter_scripts(
                cyrus_soup::Property::AiDelay,
                if missing_receiver {
                    Object::Player
                } else {
                    Object::Me
                },
                207,
                write,
            );
            if !missing_receiver {
                scripts.instances[0].object_type = PlacementType::SCRIPT_ONLY;
            }
            scripts.content_id = scripts.content_identity().unwrap();
            let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
            let error = advance(&mut world, 0).unwrap_err();
            assert!(format!("{error:#}").contains(if missing_receiver {
                "selected player unavailable"
            } else {
                "ai_delay control owner unavailable"
            }));
            assert_eq!(world.flags(), [12]);
            assert_eq!(world.actors()[0].runtime().unwrap().variables(), [4]);
            let failure = world.actors()[0].failure().unwrap().to_owned();
            assert!(advance(&mut world, 0).is_err());
            assert_eq!(world.actors()[0].failure(), Some(failure.as_str()));
        }
    }
}

#[test]
fn property_delay_is_consumed_by_public_save_restore_without_aliasing_ai_mode() {
    use cyrus_script::script::{WorldRestoreContext, WorldSnapshotSources};
    for (value, expected) in [(64, 64), (128, 128), (255, 255), (300, 44), (-1, 255)] {
        let mut scripts = counter_scripts(
            cyrus_soup::Property::AiDelay,
            Object::Named { string: 0 },
            value,
            true,
        );
        scripts.definition.references[293] = cyrus_soup::Property::AiMode;
        scripts.definition.references[292] = cyrus_soup::Property::ExtraTime;
        let program = &mut scripts.actors.get_mut("LOGIC").unwrap().program;
        let old = std::mem::take(&mut program.steps);
        for (offset, mut step) in old {
            step.next += 2;
            if let Operation::End { target } = &mut step.operation {
                *target = 3;
            }
            program.steps.insert(offset + 2, step);
        }
        for (offset, reference, value) in [(0, 292, 23), (1, 293, 11)] {
            program.steps.insert(
                offset,
                Step {
                    next: offset + 1,
                    operation: Operation::Assign {
                        destination: Place::Property {
                            object: Object::Named { string: 0 },
                            reference,
                        },
                        formula: Formula {
                            first: Value::Integer { value },
                            rest: vec![],
                        },
                    },
                },
            );
        }
        scripts.content_id = scripts.content_identity().unwrap();
        let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
        advance(&mut world, 1).unwrap();
        let data = cyrus_assets::animation::Animations {
            version: 2,
            world: 7,
            metadata: serde_json::json!({"script_content_id": scripts.content_id, "actors": []}),
            sections: Default::default(),
            models: Default::default(),
            unsupported: Default::default(),
        };
        let collision = Default::default();
        let sources = WorldSnapshotSources {
            identity_memo: None,
            effects: None,
            player: None,
            scripts: &scripts,
            animations: &data,
            collision: &collision,
        };
        let saved = world.snapshot(&sources).unwrap();
        let serialized = serde_json::to_vec(&saved).unwrap();
        let mut restored = ScriptWorld::from_snapshot(
            serde_json::from_slice(&serialized).unwrap(),
            &sources,
            WorldRestoreContext {
                generation: 9,
                globals: std::mem::take(world.globals_mut()),
                samples: &BTreeMap::new(),
            },
        )
        .unwrap();
        let control = restored.actors()[0].control_state().unwrap();
        assert_eq!(control.ai_delay, expected);
        assert_eq!(control.ai_mode, 11);
        assert_eq!(control.ai_type.byte(), 0);
        assert_eq!(control.extra_time, 23);
        assert_eq!(restored.actors()[1].control_state().unwrap().ai_delay, 0);
        advance(&mut restored, 1).unwrap();
        assert_eq!(
            restored.actors()[1].runtime().unwrap().variables(),
            [i32::from(expected)]
        );
        assert_eq!(restored.flags(), [73]);
    }
}
