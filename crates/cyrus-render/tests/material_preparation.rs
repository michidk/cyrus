use bevy::{asset::AssetPlugin, mesh::PrimitiveTopology, prelude::*};
use cyrus_assets::environment::{Environment, ModelLighting, TerrainLighting};
use cyrus_game::fidelity::Fidelity;
use cyrus_render::{
    fog::{SceneMaterialAssetsPlugin, prepare_scene_materials, validate_scene_materials},
    persistence::StagedRenderRoot,
};

#[test]
fn missing_terrain_mesh_rejects_without_converting_materials_and_can_resume() -> anyhow::Result<()>
{
    for lit in [false, true] {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            SceneMaterialAssetsPlugin,
        ))
        .init_asset::<Mesh>()
        .init_asset::<Image>()
        .init_asset::<StandardMaterial>();
        let environment = environment(lit);
        let world = app.world_mut();
        let geometry = Mesh::new(PrimitiveTopology::TriangleList, default());
        let handle = world.resource_mut::<Assets<Mesh>>().add(geometry);
        let geometry = world
            .resource_mut::<Assets<Mesh>>()
            .remove(&handle)
            .unwrap();
        let material = world
            .resource_mut::<Assets<StandardMaterial>>()
            .add(StandardMaterial::default());
        let root = world
            .spawn((StagedRenderRoot, Visibility::Hidden, Name::new("Terrain")))
            .id();
        let primitive = world
            .spawn((
                Mesh3d(handle.clone()),
                MeshMaterial3d(material.clone()),
                ChildOf(root),
            ))
            .id();
        let prepare = |world: &mut World| {
            prepare_scene_materials(
                world,
                &[root],
                &environment,
                Handle::<Environment>::default().id(),
                &Fidelity::default(),
                environment.clipping,
                Vec3::ZERO,
                &[],
                &Default::default(),
            )
        };
        let before = (
            world.resource::<Assets<Mesh>>().len(),
            world.resource::<Assets<Image>>().len(),
        );
        let error =
            prepare(world).expect_err("missing terrain geometry must reject before conversion");
        assert!(error.to_string().contains("mesh"), "{error:#}");
        assert_eq!(
            world
                .get::<MeshMaterial3d<StandardMaterial>>(primitive)
                .unwrap()
                .0,
            material
        );
        assert_eq!(
            (
                world.resource::<Assets<Mesh>>().len(),
                world.resource::<Assets<Image>>().len()
            ),
            before
        );
        world
            .resource_mut::<Assets<Mesh>>()
            .insert(handle.id(), geometry)?;
        prepare(world)?;
        assert!(
            world
                .get::<MeshMaterial3d<StandardMaterial>>(primitive)
                .is_none()
        );
        validate_scene_materials(world, &[root])?;
    }
    Ok(())
}

fn environment(lit: bool) -> Environment {
    Environment {
        effects: None,
        texture_scale: [1, 1],
        sky: None,
        background: [0; 3],
        clipping: [7.0, 3800.0],
        fog_table: [[0; 4]; 16],
        terrain_heights: None,
        water: None,
        terrain_lighting: lit.then_some(TerrainLighting {
            side: 2,
            ambient: [64.0; 3],
            directional: [0.0; 3],
            direction: [0.0, -1.0, 0.0],
        }),
        model_lighting: ModelLighting {
            ambient: [64.0; 3],
            directional: [0.0; 3],
            direction: [0.0, -1.0, 0.0],
        },
        world_palette: Vec::new(),
        prelighting: Default::default(),
        object_lights: Vec::new(),
        water_textures: Vec::new(),
    }
}

#[test]
fn publication_rejects_corrupted_light_tables_and_accepts_restoration() -> anyhow::Result<()> {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin::default(),
        SceneMaterialAssetsPlugin,
    ))
    .init_asset::<Mesh>()
    .init_asset::<Image>()
    .init_asset::<StandardMaterial>();
    let environment = environment(false);
    let world = app.world_mut();
    let mesh = world
        .resource_mut::<Assets<Mesh>>()
        .add(Mesh::new(PrimitiveTopology::TriangleList, default()));
    let material = world
        .resource_mut::<Assets<StandardMaterial>>()
        .add(StandardMaterial::default());
    let root = world
        .spawn((StagedRenderRoot, Visibility::Hidden, Name::new("Terrain")))
        .id();
    world.spawn((Mesh3d(mesh), MeshMaterial3d(material), ChildOf(root)));
    let lights = [cyrus_assets::environment::LocalLight {
        position: [512, 0, 512],
        radius: 100,
        rgb: [20, -30, 40],
    }];
    prepare_scene_materials(
        world,
        &[root],
        &environment,
        Handle::<Environment>::default().id(),
        &Fidelity::default(),
        environment.clipping,
        Vec3::new(0.0, 0.0, -3276.7998),
        &lights,
        &Default::default(),
    )?;
    validate_scene_materials(world, &[root])?;
    let (id, original) = world
        .resource::<Assets<Image>>()
        .iter()
        .next()
        .map(|(id, image)| (id, image.clone()))
        .unwrap();
    enum Corruption {
        MissingBytes,
        TruncatedBytes,
        WrongWidth,
        ColorEncoding,
        NonfiniteLight,
    }
    for corruption in [
        Corruption::MissingBytes,
        Corruption::TruncatedBytes,
        Corruption::WrongWidth,
        Corruption::ColorEncoding,
        Corruption::NonfiniteLight,
    ] {
        let mut invalid = original.clone();
        match corruption {
            Corruption::MissingBytes => invalid.data = None,
            Corruption::TruncatedBytes => {
                invalid.data.as_mut().unwrap().pop();
            }
            Corruption::WrongWidth => invalid.texture_descriptor.size.width = 4,
            Corruption::ColorEncoding => {
                invalid.texture_descriptor.format =
                    bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb
            }
            Corruption::NonfiniteLight => invalid.data.as_mut().unwrap()[256 * 32..256 * 32 + 4]
                .copy_from_slice(&f32::NAN.to_le_bytes()),
        }
        world.resource_mut::<Assets<Image>>().insert(id, invalid)?;
        let error = validate_scene_materials(world, &[root])
            .expect_err("corrupt light tables must reject publication");
        assert!(format!("{error:#}").contains("light"), "{error:#}");
        world
            .resource_mut::<Assets<Image>>()
            .insert(id, original.clone())?;
        validate_scene_materials(world, &[root])?;
    }
    Ok(())
}
