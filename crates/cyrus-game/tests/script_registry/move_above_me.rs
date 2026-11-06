//! MoveAboveMe places the selected player through the pending placement the world loop consumes,
//! relative to the running script's actor, with a prepared player collision shape.
use super::*;
use cyrus_game::script::{PlayerCollisionSource, ScriptInputs, WorldLoopOutcome};

const ORIGIN: [i32; 3] = [123, 456, 789];
/// The prepared player shape's positive Y extent, in whole map units.
const PLAYER_EXTENT: i32 = 5;

struct PlacementWorld {
    world: ScriptWorld,
    data: cyrus_assets::animation::Animations,
    scene: cyrus_game::player::data::PlayerScene,
}

/// Actor 0 runs `MoveAboveMe(offset, angle)` on `target`; actor 2 is the selected player with a
/// prepared collision shape (positive Y extent `PLAYER_EXTENT` whole units).
fn placement_world(
    offset: [i32; 3],
    angle: i32,
    angles: [u16; 3],
    target: Object,
    shape: bool,
) -> PlacementWorld {
    let call = Call {
        function_id: 4,
        function: Native::MoveAboveMe,
        kind: CallKind::Function,
        target: target.clone(),
        arguments: [offset[0], offset[1], offset[2], angle]
            .map(|value| Value::Integer { value })
            .to_vec(),
    };
    let operation = if matches!(target, Object::Me) {
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
        name: Native::MoveAboveMe,
        param_count: 4,
        kind: CallKind::Function,
    });
    add_standalone_receivers(&mut scripts);
    scripts.actors.get_mut("LOGIC").unwrap().program.strings = vec!["logic".into()];
    for instance in &mut scripts.instances {
        instance.object_type = cyrus_assets::script::PlacementType::MODEL;
    }
    scripts.instances[0].angles = angles.map(u32::from);
    scripts.instances[0].map_position = [10, 5, 20];
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
    world.prepare_spatial().unwrap();
    let player = world.id(2).unwrap();
    world
        .initialize_player_spatial(player, [123, 456, 789], [0, 300, 0])
        .unwrap();
    let data = cyrus_assets::animation::Animations {
        version: 2,
        world: scripts.world,
        metadata: serde_json::json!({"script_content_id": scripts.content_id,
            "actors": scripts.actors.keys().map(|name| if name == "CYRUS" {
                serde_json::json!({"script_name":name,"model_load_path":"animation_table","mpsz_bounds_primary":0,
                    "raex":{"unknown_12":256},"animation_groups":[]})
            } else {
                serde_json::json!({"script_name":name,"model_load_path":"none","mpsz_bounds_primary":-1})
            }).collect::<Vec<_>>(),
            "mpsz_entries":[{"total_extent":[24,12 + PLAYER_EXTENT,24],"pos_extent":[12,PLAYER_EXTENT,12],"neg_extent":[12,12,12],"center_offset":[0,0,0],"flags":0}]}),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    };
    let models =
        cyrus_collision::actor_models::ActorModelCollision::new(&Default::default()).unwrap();
    let scene = ordinary_collision_scene(world.world, models);
    if shape {
        world
            .prepare_ordinary_collision_shape(player, &data, &scene)
            .unwrap();
    }
    world.globals_mut().initialize_selected_health_for_test(100);
    PlacementWorld { world, data, scene }
}

impl PlacementWorld {
    /// Run the caller (actor 0) with or without the installed scene supplied.
    fn run(&mut self, settings: bool) -> anyhow::Result<()> {
        let source = PlayerCollisionSource {
            scene: &self.scene,
            animations: &self.data,
            view: [0, 0],
            actors: &[],
        };
        self.world.advance(
            self.world.id(0).unwrap(),
            ScriptStage::Body(Thread::Main),
            ScriptInputs {
                nominal_ticks: Some(1),
                frame_scale: Some(256),
                player_collision: settings.then_some(source),
                ..Default::default()
            },
            None,
            None,
            20,
        )
    }

    fn player_position(&self) -> [i32; 3] {
        self.world
            .spatial(self.world.id(2).unwrap())
            .unwrap()
            .local
            .position
    }

    fn player_yaw(&self) -> i32 {
        self.world
            .spatial(self.world.id(2).unwrap())
            .unwrap()
            .local
            .angles[1]
    }
}

#[test]
fn move_above_me_places_the_selected_player_when_the_world_loop_consumes_the_request() {
    // The running script's actor (actor 0) is the source, whichever way the player is reached and
    // however the source is turned; the player's own pose is untouched until the loop runs.
    for (angles, offset, angle) in [
        ([0u16; 3], [256, 512, -768], 300),
        ([0, 512, 0], [256, 0, 1024], 2048 + 5),
        ([0, 1500, 0], [-700, 64, 33], -1),
    ] {
        let mut at = placement_world(offset, angle, angles, Object::Player, true);
        let source = at.world.spatial(at.world.id(0).unwrap()).unwrap().local;
        let before = at.player_position();
        at.run(true).unwrap();
        assert_eq!(
            at.world.flags()[0],
            91,
            "the call returned and the caller continued"
        );
        assert_eq!(
            at.player_position(),
            before,
            "applied by the loop, not the call"
        );
        assert!(
            at.world.pending_map_updates().is_empty(),
            "the player is not registered"
        );
        assert_eq!(
            at.world.finish_world_loop(1).unwrap(),
            WorldLoopOutcome::Continue
        );
        let rotated = source.matrix.transform(offset).unwrap();
        // The installed standing height is -2.
        let expected = [
            source.position[0].wrapping_add(rotated[0]),
            source.position[1]
                .wrapping_add(rotated[1])
                .wrapping_sub(PLAYER_EXTENT << 8)
                .wrapping_add(-2 << 8),
            source.position[2].wrapping_add(rotated[2]),
        ];
        let after = at.player_position();
        assert_eq!(after, expected, "angles {angles:?} offset {offset:?}");
        assert_eq!(at.player_yaw(), angle & 2047, "angles {angles:?}");
        // The request is consumed once: a second loop leaves the player where it is.
        assert_eq!(
            at.world.finish_world_loop(1).unwrap(),
            WorldLoopOutcome::Continue
        );
        assert_eq!(at.player_position(), expected);
    }
}

#[test]
fn move_above_me_selected_player_errors_leave_the_player_in_place() {
    // Without the installed standing height the call fails before any placement is raised; the
    // player receiver keeps the failure, so the world loop refuses it and nothing was placed.
    let mut at = placement_world([0, 0, 0], 0, [0; 3], Object::Player, true);
    let error = format!("{:#}", at.run(false).unwrap_err());
    assert!(error.contains("standing height unavailable"), "{error}");
    assert!(at.world.finish_world_loop(1).is_err());
    assert_eq!(at.player_position(), ORIGIN);
    // A slave source is the original diagnostic 137, also before any placement.
    let mut at = placement_world([0, 0, 0], 0, [0; 3], Object::Player, true);
    at.world
        .set_actor_attribute(0, Attribute::Slave.index(), 1)
        .unwrap();
    let error = format!("{:#}", at.run(true).unwrap_err());
    assert!(error.contains("original diagnostic 137"), "{error}");
    assert!(at.world.finish_world_loop(1).is_err());
    assert_eq!(at.player_position(), ORIGIN);
}

#[test]
fn move_above_me_without_a_player_shape_names_the_missing_input() {
    let mut at = placement_world([0, 0, 0], 0, [0; 3], Object::Player, false);
    let error = format!("{:#}", at.run(true).unwrap_err());
    assert!(error.contains("collision shape not prepared"), "{error}");
    // The receiver keeps the failure, so the world loop refuses it and nothing was placed.
    assert!(at.world.finish_world_loop(1).is_err());
    assert_eq!(at.player_position(), ORIGIN);
}
