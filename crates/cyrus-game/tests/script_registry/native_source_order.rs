//! Which entries publish the shared native-command source, read through `PointAt`.
//!
//! The ordinary entry/task/main interpreter entry (and the separate-record one, which shares its
//! stage code but needs a spatial record this fixture lacks) publish their actor
//! before anything else; the RAHK native hook and the script-control `0x04` alternate path do not,
//! so a native they run reads the actor of the most recent publishing entry.
use super::*;
use cyrus_game::script::{ScriptInputs, WorldRestoreContext, WorldSnapshotSources};

/// Two actors run `PointAt()` on themselves from their main body and from their native hook.
/// Actor 0 stands at the origin and actor 1 five units along X, so facing each other is told
/// apart from facing oneself (zero differences) by the yaw.
fn order_world(script_control_of_0: u8) -> ScriptWorld {
    let call = Call {
        function_id: 4,
        function: Native::PointAt,
        kind: CallKind::Function,
        target: Object::Me,
        arguments: vec![],
    };
    let mut scripts = wait_cache(vec![
        Operation::Call { call: call.clone() },
        set_flag(91),
        Operation::Yield,
        Operation::End { target: 0 },
    ]);
    scripts.definition.functions.push(Function {
        name: Native::PointAt,
        param_count: 0,
        kind: CallKind::Function,
    });
    scripts.actors.get_mut("LOGIC").unwrap().native_hook = Some(call);
    for instance in &mut scripts.instances {
        instance.object_type = cyrus_assets::script::PlacementType::MODEL;
        instance.angles = [0, 300, 0];
    }
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
    world.prepare_spatial().unwrap();
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
        scripts: &scripts,
        animations: &data,
        collision: &collision,
    };
    let mut saved = serde_json::to_value(world.snapshot(&sources).unwrap()).unwrap();
    for (index, position) in [(0, [0.0f32; 3]), (1, [5.0, 0.0, 0.0])] {
        saved["actors"][index]["model"]["position"] = serde_json::json!(position.map(f32::to_bits));
    }
    if script_control_of_0 != 0 {
        saved["actors"][0]["control"]["script_control"] = script_control_of_0.into();
    }
    ScriptWorld::from_snapshot(
        serde_json::from_value(saved).unwrap(),
        &sources,
        WorldRestoreContext {
            generation: 8,
            globals: Default::default(),
            samples: &Default::default(),
        },
    )
    .unwrap()
}

fn stage(world: &mut ScriptWorld, index: usize, stage: ScriptStage) -> anyhow::Result<()> {
    world.advance(
        world.id(index).unwrap(),
        stage,
        ScriptInputs {
            nominal_ticks: Some(1),
            frame_scale: Some(256),
            ..Default::default()
        },
        None,
        None,
        20,
    )
}

fn yaw(world: &ScriptWorld, index: usize) -> i32 {
    world
        .spatial(world.id(index).unwrap())
        .unwrap()
        .local
        .angles[1]
}

/// Facing the actor five units along X from the origin.
const FACING_ONE_FROM_ORIGIN: i32 = 1536;

#[test]
fn an_ordinary_entry_publishes_its_own_actor_as_the_source() {
    let mut world = order_world(0);
    stage(&mut world, 0, ScriptStage::OrdinaryMain).unwrap();
    assert_eq!(
        yaw(&world, 0),
        0,
        "actor 0 is its own source: no difference"
    );
    stage(&mut world, 1, ScriptStage::OrdinaryMain).unwrap();
    assert_eq!(
        yaw(&world, 1),
        0,
        "actor 1 is its own source: no difference"
    );
}

#[test]
fn the_native_hook_reads_the_source_of_the_last_entry_without_publishing() {
    let mut world = order_world(0);
    stage(&mut world, 1, ScriptStage::OrdinaryMain).unwrap();
    // Actor 0's hook runs after actor 1's entry: it faces actor 1, not itself, and a second hook
    // run still faces actor 1, so it published nothing.
    stage(&mut world, 0, ScriptStage::NativeHook).unwrap();
    assert_eq!(yaw(&world, 0), FACING_ONE_FROM_ORIGIN);
    stage(&mut world, 0, ScriptStage::OrdinaryHook).unwrap();
    assert_eq!(yaw(&world, 0), FACING_ONE_FROM_ORIGIN);
    // Its own entry then publishes actor 0.
    stage(&mut world, 0, ScriptStage::OrdinaryMain).unwrap();
    assert_eq!(yaw(&world, 0), 0);
}

#[test]
fn the_tasks_disabled_path_does_not_publish_its_actor() {
    // Script-control mask 4 selects the alternate entry/main path, which skips the existing-task
    // pass and the source publication: actor 0 reads actor 1's source.
    let mut world = order_world(4);
    stage(&mut world, 1, ScriptStage::OrdinaryMain).unwrap();
    stage(&mut world, 0, ScriptStage::OrdinaryMain).unwrap();
    assert_eq!(yaw(&world, 0), FACING_ONE_FROM_ORIGIN);
}
