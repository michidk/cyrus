use bevy::{asset::AssetPlugin, prelude::*};
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
