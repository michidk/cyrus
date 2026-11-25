use super::*;

#[test]
fn ordinary_support_follow_carries_position_and_heading_before_axis_locks() {
    use cyrus_script::script::{ModelSupportOwners, ScriptInputs, ScriptStage};
    for enabled in [false, true] {
        let mut scripts = cache();
        for (name, kind) in [
            (Native::MoveByAxis, CallKind::Multitask),
            (Native::RotateByAxis, CallKind::Multitask),
        ] {
            scripts.definition.functions.push(Function {
                name,
                param_count: 3,
                kind,
            });
        }
        let actor = scripts.actors.get_mut("LOGIC").unwrap();
        actor.attributes[Attribute::PlayerAlign] = u8::from(enabled);
        // Body contacts enabled make the selected rider need its destination admission.
        actor.attributes[Attribute::Collide] = 1;
        actor.program.steps = BTreeMap::from([
            (
                0,
                Step {
                    next: 1,
                    operation: wait(
                        1,
                        Native::MoveByAxis,
                        CallKind::Multitask,
                        vec![
                            Value::Integer { value: 0 },
                            Value::Integer { value: 256 },
                            Value::Integer { value: 1 },
                        ],
                    ),
                },
            ),
            (
                1,
                Step {
                    next: 2,
                    operation: wait(
                        2,
                        Native::RotateByAxis,
                        CallKind::Multitask,
                        vec![
                            Value::Integer { value: 1 },
                            Value::Integer { value: 512 },
                            Value::Integer { value: 1 },
                        ],
                    ),
                },
            ),
            (
                2,
                Step {
                    next: 3,
                    operation: Operation::End { target: 0 },
                },
            ),
        ]);
        for instance in &mut scripts.instances {
            instance.object_type = PlacementType::MODEL;
        }
        scripts.instances[0].map_position = [20, 20, 30];
        scripts.instances[0].angles = [128, 2000, 64];
        scripts.instances[1].map_position = [10, 20, 30];
        scripts.content_id = scripts.content_identity().unwrap();
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        world.prepare_spatial().unwrap();
        let rider = world.id(0).unwrap();
        let owner = world.id(1).unwrap();
        assert!(!world.follow_ordinary_support(rider, None, None).unwrap());
        world.snapshot_spatial(owner).unwrap();
        let inputs = ScriptInputs {
            nominal_ticks: Some(0),
            frame_scale: Some(256),
            ..Default::default()
        };
        world
            .advance(
                owner,
                ScriptStage::Body(Thread::Main),
                inputs,
                None,
                None,
                20,
            )
            .unwrap();
        world
            .advance(owner, ScriptStage::Tasks, inputs, None, None, 20)
            .unwrap();
        assert_eq!(
            world.spatial(owner).unwrap().local.position,
            [2816, 5120, 7680]
        );
        assert_eq!(world.spatial(owner).unwrap().local.angles, [0, 512, 0]);
        world
            .set_model_support(
                rider,
                ModelSupportOwners {
                    actor: Some(owner),
                    static_placement: None,
                },
            )
            .unwrap();
        let initial = world.spatial(rider).unwrap().local;
        if enabled {
            assert!(
                world
                    .follow_ordinary_support(rider, Some(rider), None)
                    .is_err()
            );
            assert_eq!(
                world.spatial(rider).unwrap().local.position,
                initial.position
            );
        }
        assert_eq!(
            world.follow_ordinary_support(rider, None, None).unwrap(),
            enabled
        );
        assert_eq!(
            world.spatial(rider).unwrap().local.position,
            if enabled {
                [2816, 5120, 10240]
            } else {
                [5120, 5120, 7680]
            }
        );
        assert_eq!(
            world.spatial(rider).unwrap().local.angles,
            if enabled {
                [128, 464, 64]
            } else {
                [128, 2000, 64]
            }
        );
    }
}
