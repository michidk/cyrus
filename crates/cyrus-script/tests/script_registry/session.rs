use super::*;

#[test]
fn queue_movie_plays_five_through_ten_at_once_and_queues_the_rest_for_the_next_load() {
    let queued = |calls: &[(Native, &[i32])]| {
        let mut world = run_native_world(calls).unwrap();
        let play = world.globals_mut().take_host_requests().play_movie;
        (world.globals().queued_movie(), play)
    };
    assert_eq!(queued(&[(Native::QueueMovie, &[3])]), (Some(3), None));
    assert_eq!(queued(&[(Native::QueueMovie, &[11])]), (Some(11), None));
    assert_eq!(
        queued(&[(Native::QueueMovie, &[-1])]),
        (None, None),
        "-1 is the empty queue"
    );
    assert_eq!(queued(&[(Native::QueueMovie, &[-2])]), (Some(-2), None));
    assert_eq!(queued(&[(Native::QueueMovie, &[5])]), (None, Some(5)));
    assert_eq!(
        queued(&[(Native::QueueMovie, &[3]), (Native::QueueMovie, &[10])]),
        (None, Some(10)),
        "playing at once empties the queue"
    );
    assert!(run_native_world(&[(Native::QueueMovie, &[])]).is_err());
}

#[test]
fn session_request_natives_raise_requests_for_the_host() {
    use cyrus_script::script::HostRequests;
    let mut world = run_native_world(&[
        (Native::LoadWorld, &[7, 11, -3]),
        (Native::WonGame, &[]),
        (Native::PushPlayer, &[1]),
        (Native::PopPlayer, &[1]),
    ])
    .unwrap();
    assert_eq!(
        world.finish_world_loop(u32::MAX).unwrap(),
        cyrus_script::script::WorldLoopOutcome::WorldTransition([7, 11, -3])
    );
    assert_eq!(
        world.globals_mut().take_host_requests(),
        HostRequests {
            load_world: Some([7, 11, -3]),
            won_game: true,
            push_player: true,
            pop_player: true,
            play_movie: None,
            mainloop_exit: None,
        }
    );
    assert_eq!(
        world.globals_mut().take_host_requests(),
        HostRequests::default()
    );
    assert_eq!(run_native_calls(&[(Native::IsWithinMap, &[1])]).unwrap(), 1);
    assert_eq!(run_native_calls(&[(Native::Animal, &[5])]).unwrap(), 0);
    assert!(run_native_calls(&[(Native::LoadWorld, &[1])]).is_err());
}

#[test]
fn world_loop_exit_system_publishes_native_world_transition_at_last() {
    use bevy::ecs::system::RunSystemOnce;
    use bevy::prelude::*;
    use cyrus_script::script::{OrdinaryContext, ScriptActors, ScriptPlugin, WorldLoopOutcome};
    let world =
        run_native_world(&[(Native::LoadWorld, &[7, 11, -3]), (Native::WonGame, &[])]).unwrap();
    let mut app = App::new();
    app.add_plugins(ScriptPlugin)
        .init_resource::<Assets<cyrus_assets::animation::Animations>>()
        .insert_resource(State::new(cyrus_game::AppState::InGame));
    app.world_mut().resource_mut::<ScriptActors>().current = Some(world);
    assert!(
        app.world_mut()
            .run_system_once(|context: OrdinaryContext| context.is_running())
            .unwrap()
    );
    assert!(
        !app.world()
            .contains_resource::<cyrus_world::lifecycle::SceneRequest>()
    );
    app.world_mut().run_schedule(Last);
    assert_eq!(
        app.world()
            .resource::<cyrus_world::lifecycle::SceneRequest>()
            .0,
        "7"
    );
    assert_eq!(
        *app.world()
            .resource::<cyrus_script::player::placement::MarkerEntry>(),
        cyrus_script::player::placement::MarkerEntry {
            marker: 11,
            heading: 2045
        }
    );
    assert!(
        app.world()
            .contains_resource::<cyrus_world::lifecycle::ReloadScene>()
    );
    let actors = app.world().resource::<ScriptActors>();
    assert_eq!(
        actors.world_loop_outcome,
        Some(WorldLoopOutcome::WorldTransition([7, 11, -3]))
    );
    assert!(
        !app.world_mut()
            .run_system_once(|context: OrdinaryContext| context.is_running())
            .unwrap()
    );
    // A delayed destination cannot resume the old loop or dispatch its queued ending.
    app.world_mut().run_schedule(Last);
    assert!(
        !app.world()
            .contains_resource::<cyrus_script::script::EndingEntry>()
    );
    assert_eq!(
        app.world()
            .resource::<cyrus_world::lifecycle::SceneRequest>()
            .0,
        "7"
    );
}
