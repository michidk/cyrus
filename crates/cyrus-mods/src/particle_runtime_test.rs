//! Real WebAssembly guest/host exercise, optionally with offscreen GPU capture.
use super::*;
use crate::{Catalog, Entry, add_components_to_linker, particles};
use bevy::asset::AssetPlugin;
use bevy_hanabi::{EffectAsset, HanabiPlugin, ParticleEffect};
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[derive(Resource)]
struct GuestPath(String);

fn setup(
    guest: Res<GuestPath>,
    mut mods: Mods,
    mut catalog: ResMut<Catalog>,
    mut host: Query<(&mut Settings, &mut Damage)>,
) {
    let entry = &mut catalog.entries[0];
    let instance = mods.spawn(guest.0.clone(), Some(entry.manifest.name.clone()));
    mods.enable_access(instance, ModAccess::World);
    entry.instance = Some(instance);
    let (mut settings, mut damage) = host.single_mut().unwrap();
    settings.numbers.insert("blood/amount".into(), 1.0);
    settings.numbers.insert("blood/lifetime".into(), 0.7);
    damage.events.push(DamageHit {
        actor: "TEST".into(),
        loss: 10,
        position: vec![0.0, 0.0, 65_535.996],
        source: vec![100.0, 0.0, 65_535.996],
    });
}

/// Build the guest first. This ignored test deliberately fails without the explicit artifact.
#[test]
#[ignore = "requires CYRUS_BLOOD_WASM pointing to the built wasm32-wasip2 blood.wasm"]
fn wasm_blood_particles() {
    let wasm = PathBuf::from(std::env::var_os("CYRUS_BLOOD_WASM").expect("set CYRUS_BLOOD_WASM"));
    assert!(wasm.is_file(), "{}", wasm.display());
    let mod_dir = std::env::var_os("CYRUS_BLOOD_MOD_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../mods/blood"));
    let capture = std::env::var_os("CYRUS_PARTICLE_CAPTURE").map(PathBuf::from);
    let assets = AssetPlugin {
        file_path: wasm.parent().unwrap().to_string_lossy().into(),
        ..default()
    };
    let mut app = App::new();
    if capture.is_some() {
        app.add_plugins(
            DefaultPlugins
                .set(assets)
                .set(WindowPlugin {
                    primary_window: None,
                    exit_condition: bevy::window::ExitCondition::DontExit,
                    ..default()
                })
                .disable::<bevy::winit::WinitPlugin>()
                .disable::<bevy::audio::AudioPlugin>(),
        )
        .add_plugins(HanabiPlugin);
    } else {
        app.add_plugins((MinimalPlugins, bevy::log::LogPlugin::default(), assets))
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .init_resource::<Assets<EffectAsset>>();
    }
    let mut actors = ScriptActors::default();
    actors.current = Some(particles::tests::world(1));
    app.add_plugins(ModLoaderPlugin::default().add_functionality(add_components_to_linker))
        .register_type::<Vec<f32>>()
        .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            Duration::from_millis(10),
        ))
        .insert_resource(GuestPath(
            wasm.file_name().unwrap().to_string_lossy().into(),
        ))
        .insert_resource(actors)
        .insert_resource(Catalog {
            entries: vec![Entry::read(&mod_dir, true).unwrap()],
            ..default()
        })
        .init_resource::<particles::ParticleState>()
        .add_systems(Startup, (spawn_host, setup).chain())
        .add_systems(PreUpdate, particles::publish)
        .add_systems(
            PostUpdate,
            particles::update
                .before(bevy::transform::TransformSystems::Propagate)
                .before(bevy_hanabi::EffectSystems::CompileEffects)
                .before(bevy_hanabi::EffectSystems::TickSpawners),
        );

    let image = capture.as_ref().map(|_| {
        use bevy::render::render_resource::{TextureFormat, TextureUsages};
        let mut image = Image::new_target_texture(640, 480, TextureFormat::Rgba8UnormSrgb, None);
        image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
        let image = app.world_mut().resource_mut::<Assets<Image>>().add(image);
        app.world_mut().spawn((
            Camera3d::default(),
            bevy::prelude::Camera {
                clear_color: Color::srgb(0.12, 0.14, 0.17).into(),
                ..default()
            },
            bevy::camera::RenderTarget::Image(image.clone().into()),
            Transform::from_xyz(0.0, 1.0, 4.0).looking_at(Vec3::new(0.0, 1.0, 0.0), Vec3::Y),
        ));
        image
    });
    app.finish();
    app.cleanup();
    let baseline_materials = app.world().resource::<Assets<StandardMaterial>>().len();
    let deadline = Instant::now() + Duration::from_secs(60);
    let count = loop {
        app.update();
        let count = app
            .world_mut()
            .query::<&ParticleEffect>()
            .iter(app.world())
            .count();
        if count > 0 {
            break count;
        }
        assert!(Instant::now() < deadline, "Wasm guest emitted no particles");
        std::thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(
        count, 2,
        "10 health loss emits a droplet burst and a fine spray"
    );
    app.world_mut()
        .query::<&mut Damage>()
        .single_mut(app.world_mut())
        .unwrap()
        .events
        .clear();

    if let (Some(path), Some(image)) = (capture, image) {
        // Let the initial burst spread before capturing its motion.
        app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            Duration::from_millis(150),
        ));
        app.update();
        // Freeze simulation while the GPU finishes pipeline preparation and readback.
        app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            Duration::ZERO,
        ));
        for _ in 0..20 {
            app.update();
        }
        app.world_mut()
            .spawn(bevy::render::view::screenshot::Screenshot::image(image))
            .observe(bevy::render::view::screenshot::save_to_disk(path.clone()));
        while !path.exists() {
            app.update();
            assert!(Instant::now() < deadline, "GPU capture did not finish");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    app.world_mut().resource_mut::<Catalog>().entries[0].active = false;
    app.update();
    assert_eq!(
        app.world_mut()
            .query::<&ParticleEffect>()
            .iter(app.world())
            .count(),
        0
    );
    assert_eq!(
        app.world().resource::<Assets<StandardMaterial>>().len(),
        baseline_materials
    );
}
