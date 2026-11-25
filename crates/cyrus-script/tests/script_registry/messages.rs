use super::*;

fn message_sender_scripts() -> Scripts {
    message_query_scripts(Native::GetMessageSender)
}

pub(super) fn message_query_scripts(native: Native) -> Scripts {
    let mut scripts = actor_reference_scripts(Object::Me);
    let function_id = scripts.definition.functions.len() as u16;
    scripts.definition.functions.push(Function {
        name: native,
        kind: CallKind::Function,
        param_count: 0,
    });
    let actor = scripts.actors.get_mut("LOGIC").unwrap();
    actor.program.steps.get_mut(&0).unwrap().operation = Operation::Assign {
        destination: Place::Local { index: 0 },
        formula: Formula {
            first: Value::Call {
                call: Box::new(Call {
                    function_id,
                    function: native,
                    kind: CallKind::Function,
                    target: Object::Me,
                    arguments: vec![],
                }),
            },
            rest: vec![],
        },
    };
    scripts.content_id = scripts.content_identity().unwrap();
    scripts
}

#[test]
fn get_message_sender_returns_the_selected_entrys_sender_as_a_typed_reference() {
    use cyrus_script::script::CombatMessageCode;
    use cyrus_soup::{ActorReference, ScriptValue};
    let scripts = message_sender_scripts();
    // Nothing selected, a consumed message, a message and a message for another receiver.
    for (send, consume, expected) in [
        (false, false, None),
        (true, true, None),
        (true, false, Some(0)),
    ] {
        let mut world = ScriptWorld::prepare(&scripts, 17).unwrap();
        let (sender, receiver) = (world.id(0).unwrap(), world.id(1).unwrap());
        if send {
            world
                .send_combat_message(sender, receiver, CombatMessageCode::HIT)
                .unwrap();
        }
        if consume {
            world.consume_combat_message(receiver).unwrap().unwrap();
        }
        actor_reference_step(&mut world, receiver).unwrap();
        let first = world.actors()[1].runtime().unwrap().variables()[0];
        assert_eq!(
            first,
            expected.map_or(ScriptValue::Integer(0), |index| ScriptValue::Actor(
                ActorReference {
                    generation: 17,
                    index
                }
            )),
            "send {send} consume {consume}"
        );
        // The query consumes nothing and changes nothing.
        if send && !consume {
            assert!(world.peek_combat_message(receiver).unwrap().is_some());
        }
    }
}

#[test]
fn get_message_sender_returns_a_retained_queue_entry_unchanged_after_world_replacement() {
    use cyrus_script::script::{CombatMessageCode, WorldRestoreContext, WorldSnapshotSources};
    use cyrus_soup::{ActorReference, ScriptValue};
    let scripts = message_sender_scripts();
    let data = cyrus_assets::animation::Animations {
        version: 2,
        world: scripts.world,
        metadata: serde_json::json!({"script_content_id": scripts.content_id,"actors":[]}),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    };
    let mut world = ScriptWorld::prepare(&scripts, 17).unwrap();
    let (sender, receiver) = (world.id(0).unwrap(), world.id(1).unwrap());
    world
        .send_combat_message(sender, receiver, CombatMessageCode::HIT)
        .unwrap();
    let snapshot = world
        .snapshot(&WorldSnapshotSources {
            identity_memo: None,
            effects: None,
            player: None,
            scripts: &scripts,
            animations: &data,
            collision: &Default::default(),
        })
        .unwrap();
    // The queue belongs to the host's retained globals: it carries the entry across the
    // replacement of the logical world, and the stored reference comes back as stored.
    let mut world = ScriptWorld::from_snapshot(
        snapshot,
        &WorldSnapshotSources {
            identity_memo: None,
            effects: None,
            player: None,
            scripts: &scripts,
            animations: &data,
            collision: &Default::default(),
        },
        WorldRestoreContext {
            generation: 18,
            globals: std::mem::take(world.globals_mut()),
            samples: &BTreeMap::new(),
        },
    )
    .unwrap();
    let receiver = world.id(1).unwrap();
    actor_reference_step(&mut world, receiver).unwrap();
    assert_eq!(
        world.actors()[1].runtime().unwrap().variables()[0],
        ScriptValue::Actor(ActorReference {
            generation: 17,
            index: 0
        })
    );
}

#[test]
fn get_message_sender_id_returns_the_selected_senders_runtime_identifier() {
    use cyrus_script::script::{CombatMessageCode, WorldRestoreContext, WorldSnapshotSources};
    use cyrus_soup::ScriptValue;
    let scripts = message_query_scripts(Native::GetMessageSenderId);
    for (send, consume, expected) in [(false, false, 0), (true, true, 0), (true, false, 36)] {
        let mut world = ScriptWorld::prepare(&scripts, 17).unwrap();
        let (sender, receiver) = (world.id(0).unwrap(), world.id(1).unwrap());
        if send {
            world
                .send_combat_message(sender, receiver, CombatMessageCode::HIT)
                .unwrap();
        }
        if consume {
            world.consume_combat_message(receiver).unwrap().unwrap();
        }
        actor_reference_step(&mut world, receiver).unwrap();
        assert_eq!(
            world.actors()[1].runtime().unwrap().variables()[0],
            ScriptValue::Integer(expected),
            "send {send} consume {consume}"
        );
        if send && !consume {
            assert!(world.peek_combat_message(receiver).unwrap().is_some());
        }
    }
    // A retained entry whose sender belongs to a replaced world cannot be read: an explicit
    // error, with the receiver's local unchanged.
    let data = cyrus_assets::animation::Animations {
        version: 2,
        world: scripts.world,
        metadata: serde_json::json!({"script_content_id": scripts.content_id,"actors":[]}),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    };
    let mut world = ScriptWorld::prepare(&scripts, 17).unwrap();
    let (sender, receiver) = (world.id(0).unwrap(), world.id(1).unwrap());
    world
        .send_combat_message(sender, receiver, CombatMessageCode::HIT)
        .unwrap();
    let sources = WorldSnapshotSources {
        identity_memo: None,
        effects: None,
        player: None,
        scripts: &scripts,
        animations: &data,
        collision: &Default::default(),
    };
    let snapshot = world.snapshot(&sources).unwrap();
    let mut world = ScriptWorld::from_snapshot(
        snapshot,
        &sources,
        WorldRestoreContext {
            generation: 18,
            globals: std::mem::take(world.globals_mut()),
            samples: &BTreeMap::new(),
        },
    )
    .unwrap();
    let receiver = world.id(1).unwrap();
    let error = actor_reference_step(&mut world, receiver)
        .unwrap_err()
        .to_string();
    assert!(error.contains("GetMessageSenderId sender"), "{error}");
    assert_eq!(
        world.actors()[1].runtime().unwrap().variables()[0],
        ScriptValue::Integer(0)
    );
}

fn delete_message_scripts(target: Object) -> Scripts {
    message_call_scripts(Native::DeleteMessage, target)
}

pub(super) fn message_call_scripts(native: Native, target: Object) -> Scripts {
    message_call_scripts_with(native, target, vec![])
}

fn message_call_scripts_with(native: Native, target: Object, arguments: Vec<Value>) -> Scripts {
    let mut scripts = actor_reference_scripts(Object::Me);
    let function_id = scripts.definition.functions.len() as u16;
    scripts.definition.functions.push(Function {
        name: native,
        kind: CallKind::Function,
        param_count: arguments.len() as u8,
    });
    let actor = scripts.actors.get_mut("LOGIC").unwrap();
    let call = Box::new(Call {
        function_id,
        function: native,
        kind: CallKind::Function,
        target: target.clone(),
        arguments,
    });
    actor.program.steps.get_mut(&3).unwrap().operation = Operation::Assign {
        destination: Place::Local { index: 2 },
        formula: Formula {
            first: if target == Object::Me {
                Value::Call { call }
            } else {
                Value::ObjectCall { call }
            },
            rest: vec![],
        },
    };
    scripts.content_id = scripts.content_identity().unwrap();
    scripts
}

#[test]
fn delete_message_advances_the_receivers_selection_for_direct_and_object_callers() {
    use cyrus_script::script::CombatMessageCode;
    // Direct: actor 1 advances its own selection. Object: actor 1 advances actor 0's.
    for (target, receiver_index) in [(Object::Me, 1), (Object::Named { string: 0 }, 0)] {
        let scripts = delete_message_scripts(target.clone());
        let mut world = ScriptWorld::prepare(&scripts, 17).unwrap();
        let (a, b) = (world.id(0).unwrap(), world.id(1).unwrap());
        let (receiver, other) = if receiver_index == 0 { (a, b) } else { (b, a) };
        for code in [CombatMessageCode::HIT, CombatMessageCode::BLOCK] {
            world.send_combat_message(other, receiver, code).unwrap();
        }
        actor_reference_step(&mut world, b).unwrap();
        actor_reference_step(&mut world, b).unwrap();
        // The call returned zero and moved only the receiver's selection past the first entry.
        assert_eq!(
            world.actors()[1].runtime().unwrap().variables()[2],
            0,
            "target {target:?}"
        );
        let next = world.consume_combat_message(receiver).unwrap().unwrap();
        assert_eq!(next.code, CombatMessageCode::BLOCK, "target {target:?}");
        let untouched = world.peek_combat_message(other).unwrap();
        assert!(untouched.is_none(), "target {target:?}");
    }
}

#[test]
fn delete_message_without_a_selection_changes_nothing_and_wraps_at_the_entry_count() {
    use cyrus_script::script::CombatMessageCode;
    use cyrus_soup::ScriptValue;
    let scripts = delete_message_scripts(Object::Me);
    // No selection: the cursor stays, and a later message is still the selected one.
    let mut world = ScriptWorld::prepare(&scripts, 17).unwrap();
    let (a, b) = (world.id(0).unwrap(), world.id(1).unwrap());
    actor_reference_step(&mut world, b).unwrap();
    actor_reference_step(&mut world, b).unwrap();
    world
        .send_combat_message(a, b, CombatMessageCode::HIT)
        .unwrap();
    assert_eq!(
        world.consume_combat_message(b).unwrap().unwrap().code,
        CombatMessageCode::HIT
    );
    // Selection at the last entry wraps to zero, which is then the write index: no selection.
    let mut world = ScriptWorld::prepare(&scripts, 17).unwrap();
    let (a, b) = (world.id(0).unwrap(), world.id(1).unwrap());
    for _ in 0..7 {
        world
            .send_combat_message(a, b, CombatMessageCode::HIT)
            .unwrap();
        world.consume_combat_message(b).unwrap().unwrap();
    }
    world
        .send_combat_message(a, b, CombatMessageCode::BLOCK)
        .unwrap();
    assert!(world.peek_combat_message(b).unwrap().is_some());
    actor_reference_step(&mut world, b).unwrap();
    actor_reference_step(&mut world, b).unwrap();
    assert!(world.peek_combat_message(b).unwrap().is_none());
    assert_eq!(
        world.actors()[1].runtime().unwrap().variables()[2],
        ScriptValue::Integer(0)
    );
}

#[test]
fn receive_message_returns_the_first_entry_addressed_to_the_receiver_and_keeps_it_selected() {
    use cyrus_script::script::CombatMessageCode;
    use cyrus_soup::ScriptValue;
    // Direct: actor 1 receives for itself. Object: actor 1 receives on behalf of actor 0.
    for (target, receiver_index) in [(Object::Me, 1), (Object::Named { string: 0 }, 0)] {
        let scripts = message_call_scripts(Native::ReceiveMessage, target.clone());
        let mut world = ScriptWorld::prepare(&scripts, 17).unwrap();
        let (a, b) = (world.id(0).unwrap(), world.id(1).unwrap());
        let (receiver, other) = if receiver_index == 0 { (a, b) } else { (b, a) };
        // An entry for the other actor is skipped; the next one is the receiver's.
        world
            .send_combat_message(receiver, other, CombatMessageCode::HIT)
            .unwrap();
        world
            .send_combat_message(other, receiver, CombatMessageCode::BLOCK)
            .unwrap();
        actor_reference_step(&mut world, b).unwrap();
        actor_reference_step(&mut world, b).unwrap();
        assert_eq!(
            world.actors()[1].runtime().unwrap().variables()[2],
            ScriptValue::Integer(0xd000),
            "target {target:?}"
        );
        // The accepted entry stays selected: the combat stage still consumes it, and the skipped
        // one remains for its own receiver.
        let kept = world.consume_combat_message(receiver).unwrap().unwrap();
        assert_eq!(kept.code, CombatMessageCode::BLOCK, "target {target:?}");
        let theirs = world.consume_combat_message(other).unwrap().unwrap();
        assert_eq!(theirs.code, CombatMessageCode::HIT, "target {target:?}");
    }
    // Nothing selected returns zero.
    let scripts = message_call_scripts(Native::ReceiveMessage, Object::Me);
    let mut world = ScriptWorld::prepare(&scripts, 17).unwrap();
    let b = world.id(1).unwrap();
    actor_reference_step(&mut world, b).unwrap();
    actor_reference_step(&mut world, b).unwrap();
    assert_eq!(
        world.actors()[1].runtime().unwrap().variables()[2],
        ScriptValue::Integer(0)
    );
}

fn send_message_scripts(arguments: Vec<Value>) -> Scripts {
    let mut scripts = actor_reference_scripts(Object::Named { string: 0 });
    let function_id = scripts.definition.functions.len() as u16;
    scripts.definition.functions.push(Function {
        name: Native::SendMessage,
        kind: CallKind::Function,
        param_count: 1,
    });
    let actor = scripts.actors.get_mut("LOGIC").unwrap();
    actor.program.steps.get_mut(&3).unwrap().operation = Operation::Call {
        call: Call {
            function_id,
            function: Native::SendMessage,
            kind: CallKind::Function,
            target: Object::Me,
            arguments,
        },
    };
    scripts.content_id = scripts.content_identity().unwrap();
    scripts
}

#[test]
fn send_message_writes_the_queue_the_receive_chain_reads() {
    use cyrus_script::script::CombatMessageCode;
    let recipient = Value::Read {
        place: Place::Local { index: 0 },
    };
    let scripts = send_message_scripts(vec![recipient.clone(), Value::Integer { value: 0xd000 }]);
    let mut world = ScriptWorld::prepare(&scripts, 17).unwrap();
    let (a, b) = (world.id(0).unwrap(), world.id(1).unwrap());
    // Actor 1's script captures a reference to actor 0, then sends it the value.
    actor_reference_step(&mut world, b).unwrap();
    assert_eq!(world.receive_message(a).unwrap(), 0, "nothing sent yet");
    actor_reference_step(&mut world, b).unwrap();
    // The recipient receives the value word and sees the receiver as sender, then deletes it.
    assert_eq!(world.receive_message(a).unwrap(), 0xd000);
    assert_eq!(world.selected_message_sender(a).unwrap(), Some(b));
    let queued = world.peek_combat_message(a).unwrap().unwrap();
    assert_eq!(
        (queued.sender, queued.receiver, queued.code),
        (b, Some(a), CombatMessageCode::BLOCK)
    );
    world.advance_message_selection(a).unwrap();
    assert_eq!(world.receive_message(a).unwrap(), 0);
    assert!(world.peek_combat_message(a).unwrap().is_none());
    // Nothing reached anyone else.
    assert!(world.peek_combat_message(b).unwrap().is_none());
}

#[test]
fn send_message_zero_value_returns_before_reading_the_recipient_and_other_forms_fail() {
    let recipient = Value::Read {
        place: Place::Local { index: 0 },
    };
    // A zero value changes nothing, even with an integer where the recipient belongs.
    for first in [recipient.clone(), Value::Integer { value: 1234 }] {
        let scripts = send_message_scripts(vec![first, Value::Integer { value: 0 }]);
        let mut world = ScriptWorld::prepare(&scripts, 17).unwrap();
        let (a, b) = (world.id(0).unwrap(), world.id(1).unwrap());
        actor_reference_step(&mut world, b).unwrap();
        actor_reference_step(&mut world, b).unwrap();
        assert!(world.peek_combat_message(a).unwrap().is_none());
    }
    // The declared single-argument form and an integer recipient fail explicitly.
    for (arguments, expected) in [
        (
            vec![Value::Integer { value: 7 }],
            "requires a recipient actor reference",
        ),
        (
            vec![Value::Integer { value: 1234 }, Value::Integer { value: 5 }],
            "SendMessage recipient",
        ),
    ] {
        let scripts = send_message_scripts(arguments);
        let mut world = ScriptWorld::prepare(&scripts, 17).unwrap();
        let (a, b) = (world.id(0).unwrap(), world.id(1).unwrap());
        actor_reference_step(&mut world, b).unwrap();
        let error = format!("{:#}", actor_reference_step(&mut world, b).unwrap_err());
        assert!(error.contains(expected), "{error}");
        assert!(world.peek_combat_message(a).unwrap().is_none());
    }
}

#[test]
fn is_message_waiting_reports_a_selection_without_reading_or_changing_the_queue() {
    use cyrus_script::script::CombatMessageCode;
    use cyrus_soup::ScriptValue;
    // Direct: actor 1 asks about itself. Object: actor 1 asks about actor 0.
    for (target, receiver_index) in [(Object::Me, 1), (Object::Named { string: 0 }, 0)] {
        let scripts = message_call_scripts(Native::IsMessageWaiting, target.clone());
        for (send, expected) in [(false, 0), (true, 1)] {
            let mut world = ScriptWorld::prepare(&scripts, 17).unwrap();
            let (a, b) = (world.id(0).unwrap(), world.id(1).unwrap());
            let (receiver, other) = if receiver_index == 0 { (a, b) } else { (b, a) };
            if send {
                // An entry addressed to someone else still counts: the query reads no entry.
                world
                    .send_combat_message(receiver, other, CombatMessageCode::HIT)
                    .unwrap();
                world
                    .send_combat_message(other, receiver, CombatMessageCode::BLOCK)
                    .unwrap();
            }
            actor_reference_step(&mut world, b).unwrap();
            actor_reference_step(&mut world, b).unwrap();
            assert_eq!(
                world.actors()[1].runtime().unwrap().variables()[2],
                ScriptValue::Integer(expected),
                "target {target:?} send {send}"
            );
            if send {
                // Nothing was consumed or advanced.
                assert_eq!(
                    world
                        .consume_combat_message(receiver)
                        .unwrap()
                        .unwrap()
                        .code,
                    CombatMessageCode::BLOCK
                );
                assert_eq!(
                    world.consume_combat_message(other).unwrap().unwrap().code,
                    CombatMessageCode::HIT
                );
            }
        }
    }
    // A consumed queue has no selection again.
    let scripts = message_call_scripts(Native::IsMessageWaiting, Object::Me);
    let mut world = ScriptWorld::prepare(&scripts, 17).unwrap();
    let (a, b) = (world.id(0).unwrap(), world.id(1).unwrap());
    world
        .send_combat_message(a, b, CombatMessageCode::HIT)
        .unwrap();
    world.consume_combat_message(b).unwrap().unwrap();
    actor_reference_step(&mut world, b).unwrap();
    actor_reference_step(&mut world, b).unwrap();
    assert_eq!(
        world.actors()[1].runtime().unwrap().variables()[2],
        ScriptValue::Integer(0)
    );
}

#[test]
fn reply_sender_writes_one_entry_to_the_selected_sender_on_the_receivers_channel() {
    use cyrus_script::script::CombatMessageCode;
    // Direct: actor 1 replies to the sender of its own selected message. Object: actor 1 asks
    // actor 0 to reply to the sender of actor 0's selected message.
    for (target, receiver_index) in [(Object::Me, 1), (Object::Named { string: 0 }, 0)] {
        let scripts = message_call_scripts_with(
            Native::ReplySender,
            target.clone(),
            vec![Value::Integer { value: 0xd000 }],
        );
        let mut world = ScriptWorld::prepare(&scripts, 17).unwrap();
        let (a, b) = (world.id(0).unwrap(), world.id(1).unwrap());
        let (receiver, other) = if receiver_index == 0 { (a, b) } else { (b, a) };
        world
            .send_combat_message(other, receiver, CombatMessageCode::HIT)
            .unwrap();
        // Consume the original so only the reply remains addressed to `other`.
        let before_selection = world.selected_message_sender(receiver).unwrap();
        assert_eq!(before_selection, Some(other));
        actor_reference_step(&mut world, b).unwrap();
        actor_reference_step(&mut world, b).unwrap();
        let reply = world.consume_combat_message(other).unwrap().unwrap();
        assert_eq!(
            (reply.sender, reply.receiver, reply.code),
            (receiver, Some(other), CombatMessageCode::BLOCK),
            "target {target:?}"
        );
        // The receiver's own selection is unchanged.
        assert_eq!(
            world.selected_message_sender(receiver).unwrap(),
            Some(other),
            "target {target:?}"
        );
    }
}

#[test]
fn reply_sender_writes_nothing_without_a_value_a_selection_or_a_distinct_sender() {
    use cyrus_script::script::CombatMessageCode;
    let reply = |value: i32| {
        message_call_scripts_with(
            Native::ReplySender,
            Object::Me,
            vec![Value::Integer { value }],
        )
    };
    let run = |scripts: Scripts, send: Option<(usize, usize)>| {
        let mut world = ScriptWorld::prepare(&scripts, 17).unwrap();
        let ids = [world.id(0).unwrap(), world.id(1).unwrap()];
        if let Some((from, to)) = send {
            world
                .send_combat_message(ids[from], ids[to], CombatMessageCode::HIT)
                .unwrap();
        }
        actor_reference_step(&mut world, ids[1]).unwrap();
        actor_reference_step(&mut world, ids[1]).unwrap();
        world
    };
    // A zero value, no selection, and a message from the receiver to itself each write nothing.
    for (value, send) in [(0, Some((0, 1))), (0xd000, None), (0xd000, Some((1, 1)))] {
        let world = run(reply(value), send);
        let ids = [world.id(0).unwrap(), world.id(1).unwrap()];
        assert!(
            world.peek_combat_message(ids[0]).unwrap().is_none(),
            "value {value} send {send:?}"
        );
    }
}

#[test]
fn block_message_displaces_sender_and_hands_off_without_changing_budget() {
    for (sender_position, expected) in [
        ([10000, 77, 0], [13000, 77, 0]),
        ([-10000, 77, 0], [-13000, 77, 0]),
        ([0, 77, 10000], [0, 77, 13000]),
        ([0, 77, -10000], [0, 77, -13000]),
        ([0, 77, 0], [0, 77, 3000]),
        ([10000, 77, 10000], [12121, 77, 12121]),
        ([-10000, 77, -10000], [-12121, 77, -12121]),
        ([16777217, 77, 0], [16780216, 77, 0]),
    ] {
        for coordinator in [false, true] {
            let (mut world, scripts, data) = coordination_message_fixture(|saved| {
                for index in 0..6 {
                    saved["actors"][index]["control"]["combat"]["flags"] = 0xff.into();
                    saved["actors"][index]["control"]["combat"]["attacker_selection_state"] =
                        0.into();
                }
                saved["actors"][5]["control"]["combat"]["attacker_selection_state"] =
                    i32::from(coordinator).into();
                saved["spatial"]["actors"][0]["local"]["position"] = serde_json::json!([0, 5, 0]);
                saved["spatial"]["actors"][1]["local"]["position"] =
                    serde_json::json!(sender_position);
            });
            world.ordinary_actors([0, 0]).unwrap();
            let receiver = world.id(0).unwrap();
            let sender = world.id(1).unwrap();
            let before = movement_snapshot(&world, &scripts, &data);
            world
                .send_combat_message(sender, receiver, CombatMessageCode::BLOCK)
                .unwrap();
            world.process_combat_message(receiver, None).unwrap();
            assert_eq!(world.spatial(sender).unwrap().local.position, expected);
            assert_eq!(world.spatial(receiver).unwrap().local.position, [0, 5, 0]);
            let after = movement_snapshot(&world, &scripts, &data);
            assert_eq!(
                after["spatial"]["actors"][1]["local"]["angles"],
                before["spatial"]["actors"][1]["local"]["angles"]
            );
            assert_eq!(
                after["spatial"]["actors"][1]["local"]["matrix_bits"],
                before["spatial"]["actors"][1]["local"]["matrix_bits"]
            );
            for index in [0, 1] {
                assert_eq!(
                    world.actors()[index]
                        .control_state()
                        .unwrap()
                        .combat
                        .as_ref()
                        .unwrap()
                        .flags
                        .bits(),
                    0xfc
                );
            }
            let combat = world.actors()[0]
                .control_state()
                .unwrap()
                .combat
                .as_ref()
                .unwrap();
            assert_eq!(combat.decision.aggression, 7);
            assert_eq!(combat.attacker_selection_state, i32::from(!coordinator));
            assert_eq!(
                combat.close_distance_passes,
                if coordinator { 11 } else { 0 }
            );
            if coordinator {
                let message = world
                    .consume_combat_message(world.id(5).unwrap())
                    .unwrap()
                    .unwrap();
                assert_eq!(
                    (message.sender, message.receiver, message.code),
                    (
                        receiver,
                        Some(world.id(5).unwrap()),
                        CombatMessageCode::SENDER_COORDINATES
                    )
                );
            }
            assert_eq!(world.consume_combat_message(receiver).unwrap(), None);
        }
    }
}

#[test]
fn block_message_keeps_displacement_and_intent_clears_before_handoff_error() {
    let (mut world, _, _) = coordination_message_fixture(|saved| {
        saved["actors"][0]["control"]["combat"]["attacker_selection_state"] = 0.into();
        for index in [0, 1] {
            saved["actors"][index]["control"]["combat"]["flags"] = 0xff.into();
            saved["spatial"]["actors"][index]["local"]["position"] = serde_json::json!([0, 77, 0]);
        }
    });
    let receiver = world.id(0).unwrap();
    let sender = world.id(1).unwrap();
    world
        .send_combat_message(sender, receiver, CombatMessageCode::BLOCK)
        .unwrap();
    let error = format!(
        "{:#}",
        world.process_combat_message(receiver, None).unwrap_err()
    );
    assert!(error.contains("ordinary traversal unavailable"), "{error}");
    assert_eq!(world.spatial(sender).unwrap().local.position, [0, 77, 3000]);
    for index in [0, 1] {
        assert_eq!(
            world.actors()[index]
                .control_state()
                .unwrap()
                .combat
                .as_ref()
                .unwrap()
                .flags
                .bits(),
            0xfc
        );
    }
    assert_eq!(world.consume_combat_message(receiver).unwrap(), None);
}

#[test]
fn block_message_handoff_gates_skip_unavailable_traversal_and_selected_death_queries() {
    for (state, dead, selected) in [(1, false, false), (0, true, false), (0, false, true)] {
        let (mut world, _, _) = coordination_message_fixture(|saved| {
            saved["actors"][0]["control"]["combat"]["attacker_selection_state"] = state.into();
            if dead {
                saved["actors"][0]["control"]["death_selector"] = 8.into();
            }
            for index in [0, 1] {
                saved["spatial"]["actors"][index]["local"]["position"] =
                    serde_json::json!([0, 77, 0]);
            }
        });
        let receiver = world.id(0).unwrap();
        let sender = world.id(1).unwrap();
        world
            .send_combat_message(sender, receiver, CombatMessageCode::BLOCK)
            .unwrap();
        world
            .process_combat_message(receiver, selected.then_some(receiver))
            .unwrap();
        assert_eq!(world.spatial(sender).unwrap().local.position, [0, 77, 3000]);
        let combat = world.actors()[0]
            .control_state()
            .unwrap()
            .combat
            .as_ref()
            .unwrap();
        assert_eq!(
            (
                combat.attacker_selection_state,
                combat.close_distance_passes,
                combat.decision.aggression
            ),
            (state, 11, 7)
        );
    }
}
