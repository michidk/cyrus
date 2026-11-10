//! Shared native/browser graphics, camera, HUD and subtitles.
pub mod animation;
pub mod camera;
pub mod console;
mod console_environment;
pub mod environment;
mod flat;
pub use flat::prepare_scene_flats;
pub mod fog;
pub mod hud;
pub mod lighting;
pub mod persistence;
pub mod player_model;
pub mod presentation_clock;
mod sampling;
pub mod script_transform;
mod sky;
#[cfg(not(target_arch = "wasm32"))]
pub mod subtitles;
mod terrain;
mod weapon_model;
pub use terrain::TextureAdmission as TerrainTextureAdmission;
pub mod hooks;
mod water;
use bevy::prelude::*;
pub use water::{
    CameraTerrainView, PreparedTerrainMesh, TerrainCandidate, TerrainMeshReplacement,
    TerrainPlaybackSnapshot, TerrainTarget, WaterPlayback,
};
/// Shared world presentation for native and browser hosts.
pub struct WorldRenderPlugin;
impl Plugin for WorldRenderPlugin {
    fn build(&self, app: &mut App) {
        flat::register(app);
        app.add_systems(Update, apply_anisotropic_samplers);
        app.add_plugins(persistence::SceneIdentityPlugin);
        app.add_plugins((
            camera::CameraPlugin,
            console::ConsoleRenderPlugin,
            animation::AnimationRenderPlugin,
            script_transform::ScriptTransformPlugin,
            player_model::PlayerModelPlugin,
            weapon_model::WeaponModelPlugin,
            environment::EnvironmentPlugin,
        ))
        .add_systems(
            Update,
            scene_presentation.after(cyrus_world::lifecycle::WorldChange),
        )
        .add_systems(
            Update,
            scene_ready
                .after(scene_presentation)
                .before(camera::script_view),
        );
    }
}
#[derive(Component)]
struct SceneEntity;

/// Current world whose rendered scene has finished spawning, before actor effects run.
/// The handle identifies that world's animation cache; presence does not establish gameplay readiness.
#[derive(Resource)]
pub struct ReadyScene(pub Handle<cyrus_assets::animation::Animations>);

fn scene_ready(
    mut commands: Commands,
    prepared: Option<Res<cyrus_world::lifecycle::PreparedWorld>>,
    spawner: Res<bevy::world_serialization::WorldInstanceSpawner>,
    roots: Query<&bevy::world_serialization::WorldInstance, With<SceneEntity>>,
) {
    if let Some(prepared) = prepared
        && let Ok(instance) = roots.single()
        && spawner.instance_is_ready(**instance)
    {
        commands.insert_resource(ReadyScene(prepared.animations.clone()));
    } else {
        commands.remove_resource::<ReadyScene>();
    }
}
fn scene_presentation(
    mut commands: Commands,
    assets: Res<AssetServer>,
    prepared: Option<Res<cyrus_world::lifecycle::PreparedWorld>>,
    restored: Option<Res<persistence::scene::RestoredScene>>,
    old: Query<Entity, With<SceneEntity>>,
    mut frame: ResMut<camera::FrameScene>,
) {
    let Some(prepared) = prepared else {
        return;
    };
    if !prepared.is_changed() {
        return;
    }
    if let Some(restored) = restored {
        commands.remove_resource::<persistence::scene::RestoredScene>();
        if restored.matches(&prepared) {
            return;
        }
    }
    for e in &old {
        commands.entity(e).despawn();
    }
    commands.insert_resource(environment::EnvironmentRequest(
        assets.load(prepared.environment_path.clone()),
    ));
    frame.0 = true;
    commands.spawn((
        SceneEntity,
        cyrus_script::script::ScriptScene::new(prepared.animations.clone()),
        WorldAssetRoot(
            assets.load(GltfAssetLabel::Scene(0).from_asset(prepared.scene_path.clone())),
        ),
    ));
}

#[cfg(test)]
mod session_scene_tests {
    use super::*;
    #[test]
    fn adopted_scene_skips_first_prepared_change_without_reframing() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .insert_resource(camera::FrameScene(false))
            .add_systems(Update, scene_presentation);
        let prepared = cyrus_world::lifecycle::PreparedWorld {
            requested_name: "TEST".into(),
            world: Some(17),
            animations: Handle::default(),
            scripts: Handle::default(),
            player_path: "test.player".into(),
            scene_path: "test.glb".into(),
            environment_path: "test.environment".into(),
        };
        app.insert_resource(persistence::scene::RestoredScene::new(&prepared));
        app.insert_resource(prepared);
        let root = app.world_mut().spawn(SceneEntity).id();
        app.update();
        assert!(app.world().get::<SceneEntity>(root).is_some());
        assert!(!app.world().resource::<camera::FrameScene>().0);
        assert!(
            !app.world()
                .contains_resource::<persistence::scene::RestoredScene>()
        );
        app.update();
        assert!(app.world().get::<SceneEntity>(root).is_some());
        assert!(!app.world().resource::<camera::FrameScene>().0);
    }
}

/// Overrides color-image samplers while the optional Modern filtering area is active.
/// Original descriptors are retained so disabling it restores each image's own policy.
fn apply_anisotropic_samplers(
    fidelity: Res<cyrus_game::fidelity::Fidelity>,
    mut images: ResMut<Assets<Image>>,
    mut originals: Local<std::collections::HashMap<AssetId<Image>, bevy::image::ImageSampler>>,
) {
    use bevy::image::{ImageFilterMode, ImageSampler};
    use cyrus_game::fidelity::{Area, Mode};
    if fidelity.mode(Area::AnisotropicFiltering) == Mode::Faithful {
        for (id, sampler) in originals.drain() {
            if let Some(mut image) = images.get_mut(id) {
                image.sampler = sampler;
            }
        }
        return;
    }
    originals.retain(|id, _| images.contains(*id));
    let pending: Vec<_> = images
        .iter()
        .filter(|(id, image)| {
            !originals.contains_key(id) && image.texture_descriptor.format.is_srgb()
        })
        .map(|(id, _)| id)
        .collect();
    for id in pending {
        if let Some(mut image) = images.get_mut(id) {
            originals.insert(id, image.sampler.clone());
            let mut sampler = image.sampler.get_or_init_descriptor().clone();
            sampler.mag_filter = ImageFilterMode::Linear;
            sampler.min_filter = ImageFilterMode::Linear;
            sampler.mipmap_filter = ImageFilterMode::Linear;
            sampler.anisotropy_clamp = 16;
            image.sampler = ImageSampler::Descriptor(sampler);
        }
    }
}

#[cfg(test)]
mod filtering_tests {
    use super::*;
    use bevy::image::{ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
    use cyrus_game::fidelity::{Area, Fidelity, Mode};

    #[test]
    fn filtering_covers_late_images_and_restores_distinct_samplers() {
        let mut app = App::new();
        app.init_resource::<Fidelity>()
            .init_resource::<Assets<Image>>()
            .add_systems(Update, apply_anisotropic_samplers);
        let make = |sampler| {
            let mut image = Image::default();
            image.texture_descriptor.format =
                bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb;
            image.sampler = sampler;
            image
        };
        let first = app
            .world_mut()
            .resource_mut::<Assets<Image>>()
            .add(make(ImageSampler::nearest()));
        app.world_mut()
            .resource_mut::<Fidelity>()
            .set(Area::AnisotropicFiltering, Mode::Modern);
        app.update();
        let late = app
            .world_mut()
            .resource_mut::<Assets<Image>>()
            .add(make(ImageSampler::Default));
        app.update();
        let images = app.world().resource::<Assets<Image>>();
        for handle in [&first, &late] {
            let ImageSampler::Descriptor(sampler) = &images.get(handle).unwrap().sampler else {
                panic!("sampler not overridden")
            };
            assert_eq!(sampler.anisotropy_clamp, 16);
            assert_eq!(sampler.min_filter, ImageFilterMode::Linear);
            assert_eq!(sampler.mipmap_filter, ImageFilterMode::Linear);
        }
        app.world_mut()
            .resource_mut::<Fidelity>()
            .set(Area::AnisotropicFiltering, Mode::Faithful);
        app.update();
        let images = app.world().resource::<Assets<Image>>();
        let ImageSampler::Descriptor(sampler) = &images.get(&first).unwrap().sampler else {
            panic!("nearest descriptor lost")
        };
        assert_eq!(
            sampler.min_filter,
            ImageSamplerDescriptor::nearest().min_filter
        );
        assert_eq!(sampler.anisotropy_clamp, 1);
        assert!(matches!(
            images.get(&late).unwrap().sampler,
            ImageSampler::Default
        ));
    }
}
