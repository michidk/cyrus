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
