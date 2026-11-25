//! Receiver-owned panic-group stored default uses public property dispatch.
use cyrus_assets::script::*;
use cyrus_script::script::MotionDirection;
use cyrus_script::script::{ScriptInputs, ScriptStage, ScriptWorld};
use cyrus_soup::AnimationGroup;
use cyrus_soup::program::*;

mod support;
use support::advance;
use support::counter_scripts;

#[test]
fn panic_group_routes_signed_words_without_a_playback_allocation() {
    for (value, expected) in [
        (0, 0),
        (5, 5),
        (32767, 32767),
        (32768, -32768),
        (-1, -1),
        (65538, 2),
    ] {
        for object in [Object::Me, Object::Named { string: 0 }] {
            let scripts = counter_scripts(cyrus_soup::Property::PanicGroup, object, value, true);
            let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
            advance(&mut world, 1).unwrap();
            assert_eq!(world.actors()[1].runtime().unwrap().variables(), [expected]);
            assert_eq!(world.flags(), [73]);
            assert_eq!(
                world.actors()[0]
                    .control_state()
                    .unwrap()
                    .motion_classification,
                MotionDirection::NONE
            );
        }
    }
}

#[test]
fn panic_group_allocation_is_zero_and_missing_receiver_stops_continuation() {
    let scripts = counter_scripts(cyrus_soup::Property::PanicGroup, Object::Me, 99, false);
    let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
    advance(&mut world, 0).unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().variables(), [0]);
    for write in [false, true] {
        let scripts = counter_scripts(cyrus_soup::Property::PanicGroup, Object::Player, 5, write);
        let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
        let error = advance(&mut world, 0).unwrap_err();
        assert!(format!("{error:#}").contains("Player property selected player unavailable"));
        assert_eq!(world.flags(), [12]);
        assert_eq!(world.actors()[0].runtime().unwrap().variables(), [4]);
        let failure = world.actors()[0].failure().unwrap().to_owned();
        assert!(advance(&mut world, 0).is_err());
        assert_eq!(world.actors()[0].failure(), Some(failure.as_str()));
    }
}

fn animation_data(scripts: &Scripts, sound: bool) -> cyrus_assets::animation::Animations {
    let mut commands = vec![];
    if sound {
        commands.push(serde_json::json!({"opcode":4,"sound_param":-17,"volume_shift":0}));
    }
    commands.push(serde_json::json!({"opcode":0,"frame_reference":50}));
    commands.push(serde_json::json!({"opcode":2,"value":0}));
    cyrus_assets::animation::Animations {
        version: 2,
        world: scripts.world,
        metadata: serde_json::json!({"script_content_id":scripts.content_id,"actors":[{
            "script_name":"LOGIC","animation_groups":[
                {"group_index":0,"anim_id":1,"anim_type":0,"commands":[{"opcode":0,"frame_reference":10},{"opcode":2,"value":0}]},
                {"group_index":5,"anim_id":1,"anim_type":0,"commands":commands}]}]}),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    }
}

#[test]
fn panic_group_write_is_deferred_until_the_public_animation_stage() {
    for fail_sound in [false, true] {
        let scripts = counter_scripts(cyrus_soup::Property::PanicGroup, Object::Me, 5, true);
        let data = animation_data(&scripts, fail_sound);
        let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
        let id = world.id(0).unwrap();
        world
            .bind_animation(id, &data, AnimationGroup::PANIC)
            .unwrap();
        assert_eq!(world.animation_frame(id).unwrap(), None);
        advance(&mut world, 0).unwrap();
        assert_eq!(world.flags(), [73]);
        assert_eq!(world.actors()[0].runtime().unwrap().variables(), [5]);
        assert_eq!(world.animation_frame(id).unwrap(), None);
        let mut calls = 0;
        let result = world.advance(
            id,
            ScriptStage::Animation,
            ScriptInputs {
                nominal_ticks: Some(1),
                frame_scale: Some(256),
                ..Default::default()
            },
            Some(&mut |_, _| {
                calls += 1;
                anyhow::bail!("playback sound failed")
            }),
            None,
            20,
        );
        if fail_sound {
            assert!(format!("{:#}", result.unwrap_err()).contains("playback sound failed"));
            assert_eq!(calls, 1);
            assert_eq!(world.animation_frame(id).unwrap(), None);
        } else {
            result.unwrap();
            assert_eq!(calls, 0);
            assert_eq!(world.animation_frame(id).unwrap(), Some(50));
        }
        assert_eq!(world.flags(), [73]);
        assert_eq!(world.actors()[0].runtime().unwrap().variables(), [5]);
    }
}

#[test]
fn panic_group_keeps_two_receiver_words_independent() {
    let mut scripts = counter_scripts(cyrus_soup::Property::PanicGroup, Object::Me, 7, true);
    let actor = scripts.actors.get_mut("LOGIC").unwrap();
    actor.program.variables.push(9);
    let property = |object| Place::Property {
        object,
        reference: 294,
    };
    let assign = |destination, first| Operation::Assign {
        destination,
        formula: Formula {
            first,
            rest: vec![],
        },
    };
    let operations = [
        assign(
            property(Object::Named { string: 0 }),
            Value::Integer { value: -2 },
        ),
        assign(property(Object::Me), Value::Integer { value: 7 }),
        assign(
            Place::Local { index: 0 },
            Value::Read {
                place: property(Object::Named { string: 0 }),
            },
        ),
        assign(
            Place::Local { index: 1 },
            Value::Read {
                place: property(Object::Me),
            },
        ),
        Operation::End { target: 4 },
    ];
    actor.program.steps = operations
        .into_iter()
        .enumerate()
        .map(|(i, operation)| {
            (
                i as u32,
                Step {
                    next: i as u32 + 1,
                    operation,
                },
            )
        })
        .collect();
    actor.instance_variables = vec![vec![4, 9], vec![4, 9]];
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
    advance(&mut world, 1).unwrap();
    assert_eq!(world.actors()[1].runtime().unwrap().variables(), [-2, 7]);
}

#[test]
fn stored_default_survives_playback_snapshot_without_dispatching_commands() {
    let scripts = counter_scripts(cyrus_soup::Property::PanicGroup, Object::Me, 5, true);
    let data = animation_data(&scripts, false);
    let mut playback =
        cyrus_world::animation::ActorAnimation::from_asset(&data, "LOGIC", AnimationGroup::PANIC)
            .unwrap();
    playback.set_stored_default(AnimationGroup::IDLE_TORCH);
    let snapshot = playback.snapshot();
    let mut restored =
        cyrus_world::animation::ActorAnimation::from_snapshot(&data, "LOGIC", snapshot).unwrap();
    assert_eq!(restored.current_group(), None);
    assert_eq!(restored.default_group(), AnimationGroup::IDLE_TORCH);
    assert_eq!(
        restored.request(AnimationGroup::IDLE_TORCH, 0, 1).unwrap(),
        Some(50)
    );
}
