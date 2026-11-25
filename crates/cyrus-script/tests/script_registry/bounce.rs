use super::*;

/// Two animated actors (LOGIC with the ground-collision bit, BOUNCY with bounce power and the
/// landing group 10) in a collision scene without terrain. The group list also carries the fall
/// groups the terrain sink fall start requests.
fn animated_bounce_pair() -> (
    ScriptWorld,
    cyrus_assets::animation::Animations,
    cyrus_script::player::data::PlayerScene,
    [cyrus_script::script::ScriptActorId; 2],
) {
    animated_bounce_pair_at([10, 20, 30])
}

pub(super) fn animated_bounce_pair_at(
    position: [i64; 3],
) -> (
    ScriptWorld,
    cyrus_assets::animation::Animations,
    cyrus_script::player::data::PlayerScene,
    [cyrus_script::script::ScriptActorId; 2],
) {
    animated_bounce_pair_with_group(position, true)
}

fn animated_bounce_pair_with_group(
    position: [i64; 3],
    misc_group: bool,
) -> (
    ScriptWorld,
    cyrus_assets::animation::Animations,
    cyrus_script::player::data::PlayerScene,
    [cyrus_script::script::ScriptActorId; 2],
) {
    use cyrus_assets::{
        animation::{AnimationModel, Animations},
        collision::ActorCollisionModels,
    };
    use cyrus_collision::actor_models::ActorModelCollision;
    let mut scripts = cache();
    scripts.updates.fall_landing = Some(cyrus_assets::script::FallLanding {
        death_height: 1000,
        hurt_height: 500,
        hurt_zap: 1,
        bounce_height: None,
    });
    scripts
        .actors
        .get_mut("LOGIC")
        .unwrap()
        .attributes
        .extend([(Attribute::Collide, 4), (Attribute::JumpTime, 8)]);
    let mut other = scripts.actors["LOGIC"].clone();
    other.instances = 1;
    other.instance_variables = vec![vec![4]];
    other.attributes[Attribute::BouncePower] = 140;
    other.attributes[Attribute::Collide] = 0;
    scripts.actors.insert("BOUNCY".into(), other);
    scripts.instances[1].actor = "BOUNCY".into();
    for (placement, x) in scripts
        .instances
        .iter_mut()
        .zip([position[0], position[0] + 6])
    {
        placement.object_type = PlacementType::MODEL;
        placement.map_position = [x, position[1], position[2]];
        placement.active = 1;
    }
    scripts.content_id = scripts.content_identity().unwrap();
    let group = |index: u16| {
        serde_json::json!({"group_index":index,"anim_id":1,"anim_type":0,
            "commands":[{"opcode":0,"frame_reference":0},{"opcode":2,"value":0}]})
    };
    let data = Animations {
        version: 2,
        world: scripts.world,
        metadata: serde_json::json!({"script_content_id": scripts.content_id,
            "actors": (["LOGIC", "BOUNCY"].map(|name| serde_json::json!({
                "script_name":name, "model_load_path":"animation_table",
                "ravc_count": 0,
                "raan_entries":[{"file_path":"MOVING.3D", "frame_count":1}],
                "animation_groups": ([0,10,39,56,58,113].into_iter().filter(|&index| misc_group || index != AnimationGroup::MISC1.index()).map(group).collect::<Vec<_>>())}))),
            "mpob_objects": scripts.instances.iter().map(|p| serde_json::json!({"script_name":p.actor,"object_type":1})).collect::<Vec<_>>()}),
        models: BTreeMap::from([(
            "MOVING".into(),
            AnimationModel {
                radius: Some(42.0),
                base_positions: vec![],
                base_normals: vec![],
                faces: vec![],
                corner_normals: vec![],
                encoding: "animated_i16".into(),
                positions: vec![vec![]],
                normals: vec![vec![]],
            },
        )]),
        sections: Default::default(),
        unsupported: Default::default(),
    };
    let models = ActorModelCollision::new(&ActorCollisionModels {
        object_count: 2,
        collision_flags: BTreeMap::from([(0, 0), (1, 0)]),
        ..Default::default()
    })
    .unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world.prepare_spatial().unwrap();
    world.prepare_model_resources(&data, &models).unwrap();
    let mut scene = ordinary_collision_scene(world.world, models);
    scene.settings.post_collide_height = 8;
    let ids = [world.id(0).unwrap(), world.id(1).unwrap()];
    for id in ids {
        world
            .initialize_map_animation(id, &data, 0, &mut |_, _| Ok(()))
            .unwrap();
        world
            .prepare_ordinary_collision_shape(id, &data, &scene)
            .unwrap();
    }
    world.refresh_model_presentations(&data, &ids).unwrap();
    (world, data, scene, ids)
}

#[test]
fn the_bounce_handler_prefix_ends_bounce_object_states_and_drops_an_invalid_support() {
    use cyrus_script::script::ModelSupportOwners;
    let (mut world, _data, _scene, ids) = animated_bounce_pair();
    let living = |world: &ScriptWorld| world.actors()[0].control_state().unwrap().living_state;
    // BounceObject owns states 30 and 31: the probe ends before the support query.
    for state in [LivingState::BOUNCE_ENTRY, LivingState::BOUNCE_RISING] {
        world.set_living_state_for_test(ids[0], state);
        assert!(world.bounce_handler_prefix_for_test(ids[0]).unwrap());
        assert_eq!(living(&world), state);
    }
    // State 32 and the ordinary state continue unchanged.
    for state in [LivingState::BOUNCE_FALLING, LivingState::ORDINARY] {
        world.set_living_state_for_test(ids[0], state);
        assert!(!world.bounce_handler_prefix_for_test(ids[0]).unwrap());
        assert_eq!(living(&world), state);
    }
    // States 27 to 29 without a support reference lose their state.
    for state in [
        LivingState::LANDED_ON_ACTOR,
        LivingState::BOUNCING_FROM_ACTOR,
        LivingState::FALLING_FROM_ACTOR,
    ] {
        world
            .set_model_support(ids[0], ModelSupportOwners::default())
            .unwrap();
        world.set_living_state_for_test(ids[0], state);
        assert!(!world.bounce_handler_prefix_for_test(ids[0]).unwrap());
        assert_eq!(living(&world), LivingState::ORDINARY);
        assert_eq!(world.actors()[0].support_actor(), None);
    }
    // A support with bounce power and the landing group keeps the state and reference
    // through the prefix; the later actor-bounce stage owns its movement.
    let support = ModelSupportOwners {
        actor: Some(ids[1]),
        static_placement: None,
    };
    world.set_model_support(ids[0], support).unwrap();
    world.set_living_state_for_test(ids[0], LivingState::BOUNCING_FROM_ACTOR);
    assert!(!world.bounce_handler_prefix_for_test(ids[0]).unwrap());
    assert_eq!(living(&world), LivingState::BOUNCING_FROM_ACTOR);
    assert_eq!(world.actors()[0].support_actor(), Some(ids[1]));
    // Zero bounce power on the support empties the reference again.
    world
        .set_actor_attribute((ids[1]).index, (Attribute::BouncePower).index(), 0)
        .unwrap();
    assert!(!world.bounce_handler_prefix_for_test(ids[0]).unwrap());
    assert_eq!(living(&world), LivingState::ORDINARY);
    assert_eq!(world.actors()[0].support_actor(), None);
}

#[test]
fn an_actor_on_a_bouncy_support_bounces_through_the_ground_probe() {
    use cyrus_script::script::{ModelSupportOwners, OrdinaryInputs, ScriptInputs};
    for misc_group in [true, false] {
        let (mut world, data, scene, ids) =
            animated_bounce_pair_with_group([10, 20, 30], misc_group);
        world
            .set_actor_attribute((ids[0]).index, (Attribute::JumpHeight).index(), 80)
            .unwrap();
        world
            .set_model_support(
                ids[0],
                ModelSupportOwners {
                    actor: Some(ids[1]),
                    static_placement: None,
                },
            )
            .unwrap();
        world.set_living_state_for_test(ids[0], LivingState::LANDED_ON_ACTOR);
        world
            .update_ordinary(
                &data,
                [10, 30],
                OrdinaryInputs {
                    scripts: ScriptInputs {
                        nominal_ticks: Some(1),
                        frame_scale: Some(256),
                        fine_ticks: Some(1),
                        step_fps: Some(30),
                        ..Default::default()
                    },
                    scene: Some(&scene),
                },
                &mut cyrus_game::random::EngineRandom::default(),
                &mut |_, _| Ok(()),
                100,
            )
            .unwrap();
        assert!(
            world.actors()[0].failure().is_none(),
            "{:?}",
            world.actors()[0].failure()
        );
        // The support's current group is not the landing group, so there is no wait: the actor
        // starts rising with the jump height as its lift less one decay step of (256 - 140) x 16.
        let control = world.actors()[0].control_state().unwrap();
        assert_eq!(control.living_state, LivingState::BOUNCING_FROM_ACTOR);
        assert_eq!(control.fall_origin_y, 80 * 256 - 116 * 16);
        assert_eq!(world.actors()[0].support_actor(), Some(ids[1]));
        eprintln!(
            "ordinary bounce misc_group={misc_group}: living={} position={:?} support=1",
            world.actors()[0].control_state().unwrap().living_state,
            world.spatial(ids[0]).unwrap().local.position
        );
    }
}

#[test]
fn queued_motion_resolves_before_the_support_query_and_needs_the_fine_tick_increment() {
    use cyrus_script::script::{OrdinaryInputs, ScriptInputs};
    // The early queued resolution runs for an actor with the ground-collision bit before any
    // support query; without the step's fine-tick increment it refuses explicitly.
    let (mut world, data, scene, ids) = animated_bounce_pair();
    world.set_queued_motion_for_test(ids[0], JumpMotion::FORWARD);
    world
        .update_ordinary(
            &data,
            [10, 30],
            OrdinaryInputs {
                scripts: ScriptInputs {
                    nominal_ticks: Some(1),
                    frame_scale: Some(256),
                    fine_ticks: None,
                    step_fps: Some(30),
                    ..Default::default()
                },
                scene: Some(&scene),
            },
            &mut cyrus_game::random::EngineRandom::default(),
            &mut |_, _| Ok(()),
            100,
        )
        .unwrap();
    let failure = world.actors()[0].failure().expect("the update refuses");
    assert!(failure.contains("fine-tick increment"), "{failure}");
    // The refused update changed no queued state.
    assert_eq!(
        world.actors()[0].control_state().unwrap().queued_motion,
        JumpMotion::FORWARD
    );
}

#[test]
fn a_bounce_object_early_exit_still_retains_the_stable_position_when_nothing_is_moving() {
    use cyrus_script::script::{OrdinaryInputs, ScriptInputs};
    for (falling, retained) in [(false, true), (true, false)] {
        let (mut world, data, scene, ids) = animated_bounce_pair();
        world.set_living_state_for_test(ids[0], LivingState::BOUNCE_RISING);
        if falling {
            let y = world.spatial(ids[0]).unwrap().local.position[1];
            world.set_falling_for_test(ids[0], JumpMotion::STANDING, y);
        }
        world
            .update_ordinary(
                &data,
                [10, 30],
                OrdinaryInputs {
                    scripts: ScriptInputs {
                        nominal_ticks: Some(1),
                        frame_scale: Some(256),
                        fine_ticks: Some(1),
                        step_fps: Some(30),
                        ..Default::default()
                    },
                    scene: Some(&scene),
                },
                &mut cyrus_game::random::EngineRandom::default(),
                &mut |_, _| Ok(()),
                100,
            )
            .unwrap();
        let position = world.spatial(ids[0]).unwrap().local.position;
        let control = world.actors()[0].control_state().unwrap();
        // The probe ends before the support query, and its caller retains the stable position
        // afterwards only while the queued, falling and sliding bytes are all zero.
        assert_eq!(control.stable_position_valid, retained, "falling {falling}");
        if retained {
            assert_eq!(control.stable_local_position, position);
        }
    }
}
