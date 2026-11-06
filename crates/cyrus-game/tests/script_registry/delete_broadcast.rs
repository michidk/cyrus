//! Broadcast selection advances through the public VM and retained message owner.
use super::*;
use cyrus_game::script::{ScriptInputs, WorldRestoreContext, WorldSnapshotSources};

fn broadcast_scripts(target: Object, kind: CallKind, arguments: Vec<Value>) -> Scripts {
    let function_id = wait_cache(vec![]).definition.functions.len() as u16;
    let call = Call {
        function_id,
        function: Native::DeleteBroadcast,
        kind,
        target: target.clone(),
        arguments,
    };
    let operation = if target == Object::Me {
        Operation::Call { call }
    } else {
        Operation::ObjectCall { call }
    };
    let mut scripts = wait_cache(vec![
        operation,
        set_flag(91),
        Operation::Yield,
        Operation::End { target: 0 },
    ]);
    scripts.definition.functions.push(Function {
        name: Native::DeleteBroadcast,
        param_count: 0,
        kind: CallKind::Function,
    });
    add_standalone_receivers(&mut scripts);
    for actor in scripts.actors.values_mut() {
        actor.attributes[Attribute::MessGroup] = 0;
    }
    scripts.content_id = scripts.content_identity().unwrap();
    scripts
}

fn broadcast_fixture(
    target: Object,
    kind: CallKind,
    arguments: Vec<Value>,
) -> (ScriptWorld, Scripts) {
    let scripts = broadcast_scripts(target, kind, arguments);
    (ScriptWorld::prepare(&scripts, 7).unwrap(), scripts)
}

fn broadcast_step(world: &mut ScriptWorld) -> anyhow::Result<()> {
    world.advance(
        world.id(0).unwrap(),
        ScriptStage::Body(Thread::Main),
        ScriptInputs::default(),
        None,
        None,
        20,
    )
}

fn selection(world: &ScriptWorld, index: usize) -> u64 {
    world.inspect(index, 1)["actors"][0]["broadcast_index"]
        .as_u64()
        .unwrap()
}

fn broadcast_restore(
    world: &ScriptWorld,
    scripts: &Scripts,
    edit: impl FnOnce(&mut serde_json::Value),
) -> ScriptWorld {
    let data = cyrus_assets::animation::Animations {
        version: 2,
        world: scripts.world,
        metadata: serde_json::json!({"script_content_id": scripts.content_id,
            "actors": scripts.actors.keys().map(|name| serde_json::json!({"script_name":name,"model_load_path":"none","mpsz_bounds_primary":-1})).collect::<Vec<_>>() }),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    };
    let collision = Default::default();
    let sources = WorldSnapshotSources {
        identity_memo: None,
        effects: None,
        player: None,
        scripts,
        animations: &data,
        collision: &collision,
    };
    let mut saved = serde_json::to_value(world.snapshot(&sources).unwrap()).unwrap();
    edit(&mut saved);
    ScriptWorld::from_snapshot(
        serde_json::from_value(saved).unwrap(),
        &sources,
        WorldRestoreContext {
            generation: world.generation() + 1,
            globals: Default::default(),
            samples: &Default::default(),
        },
    )
    .unwrap()
}

#[test]
fn delete_broadcast_empty_context_returns_zero_and_task_forms_complete() {
    assert_eq!(
        run_native_calls(&[(Native::DeleteBroadcast, &[])]).unwrap(),
        0
    );
    for kind in [CallKind::Function, CallKind::Task, CallKind::Multitask] {
        let (mut world, _) = broadcast_fixture(Object::Me, kind, vec![]);
        broadcast_step(&mut world).unwrap();
        assert_eq!(selection(&world, 0), 0);
        assert_eq!(world.flags()[0], 91);
        assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
    }
}

#[test]
fn delete_broadcast_routed_call_changes_only_receiver_and_preserves_entry() {
    let (mut world, _) = broadcast_fixture(Object::Player, CallKind::Function, vec![]);
    let sender = world.id(0).unwrap();
    let receiver = world.id(2).unwrap();
    world
        .send_combat_message(sender, receiver, CombatMessageCode::HIT)
        .unwrap();
    let message = world.peek_combat_message(receiver).unwrap().unwrap();
    let globals = serde_json::to_value(world.globals()).unwrap();
    broadcast_step(&mut world).unwrap();
    assert_eq!(selection(&world, 0), 0);
    assert_eq!(selection(&world, 2), 1);
    assert_eq!(world.flags()[0], 91);
    assert_eq!(serde_json::to_value(world.globals()).unwrap(), globals);
    assert_eq!(
        world.peek_combat_message(receiver).unwrap().unwrap(),
        message
    );
    assert_eq!(
        world.consume_combat_message(receiver).unwrap().unwrap(),
        message
    );
    assert_eq!(selection(&world, 2), 1);
}

#[test]
fn delete_broadcast_byte_selection_wrap_and_live_writer_survive_restore() {
    for (read, writes, expected) in [
        (0, 0, 0),
        (0, 1, 1),
        (1, 1, 1),
        (1, 2, 2),
        (6, 7, 7),
        (7, 8, 0),
        (8, 1, 0),
        (254, 1, 0),
        (255, 1, 0),
    ] {
        let (world, scripts) = broadcast_fixture(Object::Me, CallKind::Function, vec![]);
        let mut restored = broadcast_restore(&world, &scripts, |saved| {
            saved["actors"][0]["broadcast_index"] = read.into()
        });
        assert_ne!(restored.generation(), world.generation());
        assert_eq!(selection(&restored, 0), read);
        // Global writer cursors are not part of the world snapshot: write after the restore.
        for _ in 0..writes {
            restored
                .send_combat_message(
                    restored.id(1).unwrap(),
                    restored.id(0).unwrap(),
                    CombatMessageCode::HIT,
                )
                .unwrap();
        }
        let before = serde_json::to_value(restored.globals()).unwrap();
        broadcast_step(&mut restored).unwrap();
        assert_eq!(
            selection(&restored, 0),
            expected,
            "read {read}, writes {writes}"
        );
        assert_eq!(serde_json::to_value(restored.globals()).unwrap(), before);
        let again = broadcast_restore(&restored, &scripts, |_| {});
        assert_eq!(selection(&again, 0), expected);
    }
}

#[test]
fn delete_broadcast_old_saves_default_selection_and_receivers_are_independent() {
    let (mut world, scripts) = broadcast_fixture(Object::Me, CallKind::Function, vec![]);
    world
        .send_combat_message(
            world.id(1).unwrap(),
            world.id(0).unwrap(),
            CombatMessageCode::HIT,
        )
        .unwrap();
    broadcast_step(&mut world).unwrap();
    assert_eq!(selection(&world, 0), 1);
    assert_eq!(selection(&world, 1), 0);
    let restored = broadcast_restore(&world, &scripts, |saved| {
        for actor in saved["actors"].as_array_mut().unwrap() {
            actor.as_object_mut().unwrap().remove("broadcast_index");
        }
    });
    assert_eq!(selection(&restored, 0), 0);
    let replacement = ScriptWorld::prepare(&scripts, restored.generation() + 1).unwrap();
    assert_eq!(selection(&replacement, 0), 0);
    assert!(
        replacement
            .peek_combat_message(replacement.id(0).unwrap())
            .unwrap()
            .is_none()
    );
}

#[test]
fn delete_broadcast_declares_no_parameters_so_an_extra_argument_is_rejected_at_prepare() {
    let scripts = broadcast_scripts(
        Object::Me,
        CallKind::Function,
        vec![Value::Integer { value: 1 }],
    );
    let Err(error) = ScriptWorld::prepare(&scripts, 7) else {
        panic!("an extra argument must be rejected at prepare")
    };
    assert!(
        format!("{error:#}").contains("DeleteBroadcast"),
        "{error:#}"
    );
}

fn broadcast_sender_scripts(target: Object) -> Scripts {
    let mut scripts = message_query_scripts(Native::GetBroadcastSender);
    let first = &mut scripts
        .actors
        .get_mut("LOGIC")
        .unwrap()
        .program
        .steps
        .get_mut(&0)
        .unwrap()
        .operation;
    let Operation::Assign { formula, .. } = first else {
        unreachable!("the query fixture assigns the call result")
    };
    let Value::Call { call } = &mut formula.first else {
        unreachable!("the query fixture assigns a call")
    };
    call.target = target.clone();
    if target != Object::Me {
        formula.first = Value::ObjectCall { call: call.clone() };
    }
    scripts.content_id = scripts.content_identity().unwrap();
    scripts
}

fn sender_result(world: &ScriptWorld) -> cyrus_script::ScriptValue {
    world.actors()[1].runtime().unwrap().variables()[0]
}

#[test]
fn get_broadcast_sender_returns_the_selected_channel_zero_sender_as_a_typed_reference() {
    use cyrus_script::{ActorReference, ScriptValue};
    let scripts = broadcast_sender_scripts(Object::Me);
    let sender_of = |world: &ScriptWorld| {
        ScriptValue::Actor(ActorReference {
            generation: world.generation(),
            index: 0,
        })
    };
    // (broadcast selection, entries written, expected sender present). A selection equal to the
    // write cursor is no selection; a selected slot never written has a null sender.
    for (selection, writes, found) in [
        (0, 0, false),
        (0, 1, true),
        (1, 1, false),
        (3, 1, false),
        (0, 2, true),
        (1, 2, true),
    ] {
        let world = ScriptWorld::prepare(&scripts, 7).unwrap();
        let mut world = broadcast_restore(&world, &scripts, |saved| {
            saved["actors"][1]["broadcast_index"] = selection.into()
        });
        let (sender, receiver) = (world.id(0).unwrap(), world.id(1).unwrap());
        for _ in 0..writes {
            world
                .send_combat_message(sender, receiver, CombatMessageCode::HIT)
                .unwrap();
        }
        let before = serde_json::to_value(world.globals()).unwrap();
        actor_reference_step(&mut world, receiver).unwrap();
        let expected = if found {
            sender_of(&world)
        } else {
            ScriptValue::Integer(0)
        };
        assert_eq!(
            sender_result(&world),
            expected,
            "selection {selection} writes {writes}"
        );
        // The query consumes nothing and changes no selection.
        assert_eq!(serde_json::to_value(world.globals()).unwrap(), before);
        assert_eq!(selection_of(&world, 1), selection);
    }
}

fn selection_of(world: &ScriptWorld, index: usize) -> u64 {
    world.inspect(index, 1)["actors"][0]["broadcast_index"]
        .as_u64()
        .unwrap()
}

#[test]
fn get_broadcast_sender_follows_the_broadcast_selection_not_the_class_message_cursor() {
    use cyrus_script::{ActorReference, ScriptValue};
    let scripts = broadcast_sender_scripts(Object::Me);
    let world = ScriptWorld::prepare(&scripts, 7).unwrap();
    // The class-message cursor is on a written entry; the broadcast selection is not.
    let mut world = broadcast_restore(&world, &scripts, |saved| {
        saved["actors"][1]["message_read"] = 0.into();
        saved["actors"][1]["broadcast_index"] = 1.into();
    });
    let (sender, receiver) = (world.id(0).unwrap(), world.id(1).unwrap());
    world
        .send_combat_message(sender, receiver, CombatMessageCode::HIT)
        .unwrap();
    actor_reference_step(&mut world, receiver).unwrap();
    assert_eq!(sender_result(&world), ScriptValue::Integer(0));
    // Selecting the entry through the broadcast selection returns its sender.
    let fresh = ScriptWorld::prepare(&scripts, 7).unwrap();
    let mut world = broadcast_restore(&fresh, &scripts, |saved| {
        saved["actors"][1]["broadcast_index"] = 0.into()
    });
    let (sender, receiver) = (world.id(0).unwrap(), world.id(1).unwrap());
    world
        .send_combat_message(sender, receiver, CombatMessageCode::HIT)
        .unwrap();
    actor_reference_step(&mut world, receiver).unwrap();
    assert_eq!(
        sender_result(&world),
        ScriptValue::Actor(ActorReference {
            generation: world.generation(),
            index: 0
        })
    );
}

#[test]
fn get_broadcast_sender_reports_a_selection_outside_the_queue_with_its_context() {
    let scripts = broadcast_sender_scripts(Object::Me);
    let world = ScriptWorld::prepare(&scripts, 7).unwrap();
    let mut world = broadcast_restore(&world, &scripts, |saved| {
        saved["actors"][1]["broadcast_index"] = 200.into()
    });
    let receiver = world.id(1).unwrap();
    let error = format!(
        "{:#}",
        actor_reference_step(&mut world, receiver).unwrap_err()
    );
    assert!(error.contains("read cursor 200 outside queue"), "{error}");
}

#[test]
fn get_broadcast_sender_routed_call_reads_the_named_receivers_selection() {
    use cyrus_script::{ActorReference, ScriptValue};
    let scripts = broadcast_sender_scripts(Object::Named { string: 0 });
    let world = ScriptWorld::prepare(&scripts, 7).unwrap();
    // The named receiver (actor 0) selects the written entry; the calling actor (1) does not, so
    // a sender result proves the routed receiver's selection was read.
    let mut world = broadcast_restore(&world, &scripts, |saved| {
        saved["actors"][0]["broadcast_index"] = 0.into();
        saved["actors"][1]["broadcast_index"] = 1.into();
    });
    let (sender, caller) = (world.id(0).unwrap(), world.id(1).unwrap());
    world
        .send_combat_message(sender, caller, CombatMessageCode::HIT)
        .unwrap();
    actor_reference_step(&mut world, caller).unwrap();
    assert_eq!(
        sender_result(&world),
        ScriptValue::Actor(ActorReference {
            generation: world.generation(),
            index: 0
        })
    );
    assert_eq!(selection(&world, 1), 1);
}

/// Restore the fixture with the given per-actor broadcast selections, then step the caller
/// (actor 1) through the call and its dependent statement, returning its result local.
fn receive_broadcast_world(target: Object, selections: [u64; 2]) -> (ScriptWorld, Scripts) {
    let scripts = message_call_scripts(Native::ReceiveBroadcast, target);
    let world = ScriptWorld::prepare(&scripts, 17).unwrap();
    let world = broadcast_restore(&world, &scripts, |saved| {
        for (index, selection) in selections.into_iter().enumerate() {
            saved["actors"][index]["broadcast_index"] = selection.into();
        }
    });
    (world, scripts)
}

fn receive_result(world: &mut ScriptWorld) -> anyhow::Result<cyrus_script::ScriptValue> {
    let caller = world.id(1).unwrap();
    actor_reference_step(world, caller)?;
    actor_reference_step(world, caller)?;
    Ok(world.actors()[1].runtime().unwrap().variables()[2])
}

#[test]
fn receive_broadcast_skips_other_receivers_and_keeps_the_accepted_entry_selected() {
    use cyrus_script::ScriptValue;
    // Direct: actor 1 receives for itself. Routed: actor 1 receives on behalf of actor 0.
    for (target, receiver_index) in [(Object::Me, 1), (Object::Named { string: 0 }, 0)] {
        let (mut world, _) = receive_broadcast_world(target.clone(), [0, 0]);
        let (a, b) = (world.id(0).unwrap(), world.id(1).unwrap());
        let (receiver, other) = if receiver_index == 0 { (a, b) } else { (b, a) };
        // The first entry is addressed to the other actor and is skipped; the second is ours.
        world
            .send_combat_message(receiver, other, CombatMessageCode::HIT)
            .unwrap();
        world
            .send_combat_message(other, receiver, CombatMessageCode::BLOCK)
            .unwrap();
        let before = serde_json::to_value(world.globals()).unwrap();
        assert_eq!(
            receive_result(&mut world).unwrap(),
            ScriptValue::Integer(0xd000),
            "target {target:?}"
        );
        // Only the receiver's broadcast selection moved: skipped entry, accepted one stays.
        assert_eq!(selection(&world, receiver_index), 1, "target {target:?}");
        assert_eq!(
            selection(&world, 1 - receiver_index),
            0,
            "target {target:?}"
        );
        assert_eq!(serde_json::to_value(world.globals()).unwrap(), before);
    }
}

#[test]
fn receive_broadcast_returns_zero_without_a_selection_or_for_an_unwritten_entry() {
    use cyrus_script::ScriptValue;
    // No selection: selection equals channel zero's write cursor.
    let (mut world, _) = receive_broadcast_world(Object::Me, [0, 0]);
    assert_eq!(receive_result(&mut world).unwrap(), ScriptValue::Integer(0));
    assert_eq!(selection(&world, 1), 0);
    // A selected slot that was never written has no recipient and a zero value: accepted.
    let (mut world, _) = receive_broadcast_world(Object::Me, [0, 3]);
    let (sender, receiver) = (world.id(0).unwrap(), world.id(1).unwrap());
    world
        .send_combat_message(sender, receiver, CombatMessageCode::HIT)
        .unwrap();
    assert_eq!(receive_result(&mut world).unwrap(), ScriptValue::Integer(0));
    assert_eq!(selection(&world, 1), 3);
}

#[test]
fn receive_broadcast_skipping_wraps_the_selection_and_ignores_the_class_cursor() {
    use cyrus_script::ScriptValue;
    let scripts = message_call_scripts(Native::ReceiveBroadcast, Object::Me);
    let world = ScriptWorld::prepare(&scripts, 17).unwrap();
    // Selection on the last slot, with the class-message cursor elsewhere.
    let mut world = broadcast_restore(&world, &scripts, |saved| {
        saved["actors"][1]["broadcast_index"] = 7.into();
        saved["actors"][1]["message_read"] = 5.into();
    });
    let (a, b) = (world.id(0).unwrap(), world.id(1).unwrap());
    // Eight writes fill channel zero and return its write cursor to zero; every entry is for
    // actor 0, so the receiver skips slot 7, wraps to zero and finds no selection left.
    for _ in 0..8 {
        world
            .send_combat_message(b, a, CombatMessageCode::HIT)
            .unwrap();
    }
    assert_eq!(receive_result(&mut world).unwrap(), ScriptValue::Integer(0));
    assert_eq!(selection(&world, 1), 0);
}

#[test]
fn receive_broadcast_reports_a_selection_outside_the_queue_before_changing_it() {
    let (mut world, _) = receive_broadcast_world(Object::Me, [0, 200]);
    let error = format!("{:#}", receive_result(&mut world).unwrap_err());
    assert!(error.contains("read cursor 200 outside queue"), "{error}");
    assert_eq!(selection(&world, 1), 200);
}
