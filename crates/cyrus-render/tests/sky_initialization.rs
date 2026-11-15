use bevy::{asset::AssetPlugin, prelude::*, time::TimeUpdateStrategy};
use cyrus_game::{fidelity::Fidelity, random::EngineRandom};
use cyrus_render::environment::{Environment, EnvironmentPlugin, EnvironmentRequest, SkyImage};

#[test]
fn world_sky_construction_consumes_one_shared_random_draw() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_asset::<Image>()
        .init_asset::<Mesh>()
        .init_asset::<StandardMaterial>()
        .init_asset::<Shader>()
        .init_resource::<Fidelity>()
        .insert_resource(EngineRandom::from_state(1))
        .insert_resource(ClearColor::default())
        .add_plugins(EnvironmentPlugin);
    let source = app
        .world_mut()
        .resource_mut::<Assets<Environment>>()
        .add(environment(6));
    app.world_mut().insert_resource(EnvironmentRequest(source));
    app.update();
    assert_eq!(app.world().resource::<EngineRandom>().state(), 1);
    app.world_mut().spawn(Camera3d::default());
    app.update();
    assert_eq!(app.world().resource::<EngineRandom>().state(), 1103527590);
    let saved = cyrus_render::persistence::SkySnapshot::capture(app.world()).unwrap();
    let saved = serde_json::to_value(saved).unwrap();
    let velocity: [f32; 2] = serde_json::from_value(saved["velocity"].clone()).unwrap();
    assert_eq!(
        velocity,
        [
            cyrus_world::transform::rotation_sample(454) * (6.0 / 16.0),
            cyrus_world::transform::rotation_sample(454 + 512) * (6.0 / 16.0),
        ]
    );
    app.update();
    assert_eq!(app.world().resource::<EngineRandom>().state(), 1103527590);
    let next = app
        .world_mut()
        .resource_mut::<Assets<Environment>>()
        .add(environment(0));
    app.world_mut().insert_resource(EnvironmentRequest(next));
    app.update();
    assert_eq!(app.world().resource::<EngineRandom>().state(), 2524885223);
    let saved = cyrus_render::persistence::SkySnapshot::capture(app.world()).unwrap();
    let saved = serde_json::to_value(saved).unwrap();
    assert_eq!(saved["velocity"], serde_json::json!([0.0, 0.0]));
}

fn environment(speed: u8) -> Environment {
    Environment {
        world_palette: Vec::new(),
        effects: None,
        terrain_heights: None,
        texture_scale: [1, 1],
        sky: Some(SkyImage {
            width: 1,
            height: 1,
            rgba: vec![128, 128, 128, 255],
            level: -3500.0,
            scale: 2000.0,
            speed,
            sun: None,
        }),
        background: [0; 3],
        clipping: [1.0, 65000.0],
        fog_table: [[0; 4]; 16],
        water: None,
        terrain_lighting: None,
        model_lighting: Default::default(),
        prelighting: Default::default(),
        water_textures: Vec::new(),
        object_lights: Vec::new(),
    }
}

#[test]
fn restored_sky_wraps_both_axes_at_observed_original_boundaries() {
    use cyrus_render::persistence::{SkyImages, SkySnapshot};
    use std::time::Duration;

    let mut app = sky_app(0);
    let reference_environment = environment(0);
    let entity = app
        .world_mut()
        .query_filtered::<Entity, With<Mesh3d>>()
        .single(app.world())
        .unwrap();
    let image = app
        .world_mut()
        .resource_mut::<Assets<Image>>()
        .add(Image::default());

    // Presentation-owned original zero-speed controls, including the adjacent
    // representable value below the lower bound. Restore retains the exact state.
    let below = f32::from_bits((-256.0_f32).to_bits() + 1);
    for (input, expected) in [(-256.0, 0.0), (256.0, 0.0), (below, -1.0 / 32768.0)] {
        let mut saved = serde_json::to_value(SkySnapshot::capture(app.world()).unwrap()).unwrap();
        saved["scroll"] = serde_json::json!([input, input, 0.0, 0.0]);
        let snapshot: SkySnapshot = serde_json::from_value(saved).unwrap();
        snapshot
            .restore(
                &reference_environment,
                SkyImages {
                    sky: image.clone(),
                    sun: image.clone(),
                },
            )
            .unwrap()
            .queue(&mut app.world_mut().commands(), entity);
        app.world_mut().flush();
        let scroll = |app: &App| {
            let saved = serde_json::to_value(SkySnapshot::capture(app.world()).unwrap()).unwrap();
            serde_json::from_value::<[f32; 4]>(saved["scroll"].clone()).unwrap()
        };
        assert_eq!(scroll(&app), [input, input, 0.0, 0.0]);
        app.world_mut()
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
                55,
            )));
        app.update();
        assert_eq!(
            scroll(&app),
            [expected, expected, 0.0, 0.0],
            "input {input}"
        );
        app.update();
        assert_eq!(
            scroll(&app),
            [expected, expected, 0.0, 0.0],
            "zero-speed continuation"
        );
    }
}

fn sky_app(speed: u8) -> App {
    use std::time::Duration;
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_asset::<Image>()
        .init_asset::<Mesh>()
        .init_asset::<StandardMaterial>()
        .init_asset::<Shader>()
        .init_resource::<Fidelity>()
        .init_resource::<ClearColor>()
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO))
        .add_plugins(EnvironmentPlugin);
    let source = app
        .world_mut()
        .resource_mut::<Assets<Environment>>()
        .add(environment(speed));
    app.world_mut().insert_resource(EnvironmentRequest(source));
    app.world_mut().spawn(Camera3d::default());
    app.update();
    app
}

#[test]
fn moving_sky_matches_presented_original_ticks_through_a_full_wrap() {
    use cyrus_render::{environment::SkyScrollSync, persistence::SkySnapshot};
    use std::time::Duration;
    let mut app = sky_app(255);
    app.world_mut()
        .resource_mut::<SkyScrollSync>()
        .set(0, [0.0, 15.9375])
        .unwrap();
    app.update();
    let mut previous_tick = 0;
    // Retained original presentation endpoints, direction zero and speed 255/16.
    // Clock units are processed BIOS ticks, not game-update or rendered-frame counts.
    for (tick, expected_v) in [
        (0, 15.9375),
        (2, 47.8125),
        (3, 63.75),
        (5, 95.625),
        (7, 127.5),
        (8, 143.4375),
        (11, 191.25),
        (13, 223.125),
        (15, 255.0),
        (16, 14.9375),
        (18, 46.8125),
    ] {
        if tick > previous_tick {
            app.world_mut()
                .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
                    55 * (tick - previous_tick),
                )));
            app.update();
        }
        let saved = serde_json::to_value(SkySnapshot::capture(app.world()).unwrap()).unwrap();
        let actual: [f32; 4] = serde_json::from_value(saved["scroll"].clone()).unwrap();
        assert_eq!(actual, [0.0, expected_v, 0.0, 0.0], "processed tick {tick}");
        assert_eq!(saved["tick"].as_u64().unwrap(), tick);
        previous_tick = tick;
    }
}
