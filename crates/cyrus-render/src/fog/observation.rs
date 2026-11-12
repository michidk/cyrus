//! Read-only acquisition of the CPU image bound to a live scene material.
use super::SceneMaterial;
use anyhow::{Context, Result, ensure};
use bevy::{
    prelude::{Assets, ChildOf, Children, Entity, Image, MeshMaterial3d, Name, World},
    render::render_resource::{TextureDimension, TextureFormat},
};

/// A material binding observation, not GPU readback or a visual parity result.
#[derive(Clone, Debug, serde::Serialize)]
pub struct BoundSceneLightImage {
    /// Live mesh entity selected by the host.
    pub entity: String,
    /// Runtime asset identities, meaningful only within this process.
    pub material: String,
    /// Image asset referenced by the material at acquisition time.
    pub image: String,
    /// Logical world identity and generation observed during acquisition.
    pub world: u32,
    /// Logical world's replacement counter, not a GPU submission sequence.
    pub generation: u64,
    /// Prepared environment path used to check the current logical owner.
    pub environment: String,
    /// Image dimensions in texels. Each row contains two RGBA float texels.
    pub dimensions: [u32; 2],
    /// Actual bound image descriptor format, checked before decoding.
    pub format: String,
    /// Raw light rows after the 256 distance-lookup rows, preserving image order.
    /// Columns are map X/Y/Z, radius, signed RGB, and quantized falloff scale.
    pub rows_after_lookup_256: Vec<[f32; 8]>,
}

impl BoundSceneLightImage {
    /// Select a material-bearing descendant of an exact unique named scene node.
    ///
    /// `primitive` is the zero-based index among that node's direct children with
    /// scene-material bindings, in the current ECS Children order. It is a runtime
    /// selector, not an original model face or material identifier. A material on
    /// the named entity itself is index zero. Duplicate node names are rejected.
    pub fn capture_named_primitive(
        world: &mut World,
        name: &str,
        primitive: usize,
    ) -> Result<Self> {
        let mut named = world.query::<(Entity, &Name)>();
        let mut matches = named
            .iter(world)
            .filter(|(_, candidate)| candidate.as_str() == name)
            .map(|(entity, _)| entity);
        let node = matches
            .next()
            .with_context(|| format!("bound light observation: entity name {name:?} absent"))?;
        ensure!(
            matches.next().is_none(),
            "bound light observation: entity name {name:?} is ambiguous"
        );
        let entity = if world.get::<MeshMaterial3d<SceneMaterial>>(node).is_some() {
            ensure!(
                primitive == 0,
                "bound light node {name:?}: primitive {primitive} absent"
            );
            node
        } else {
            world
                .get::<Children>(node)
                .into_iter()
                .flatten()
                .copied()
                .filter(|entity| {
                    world
                        .get::<MeshMaterial3d<SceneMaterial>>(*entity)
                        .is_some()
                })
                .nth(primitive)
                .with_context(|| {
                    format!("bound light node {name:?}: primitive {primitive} absent")
                })?
        };
        Self::capture(world, entity)
    }

    /// Inspect the image referenced by this entity's actual scene material.
    ///
    /// Call after the host has completed its Update schedule and applied deferred
    /// material bindings. The caller must record frame, logical/rendered camera,
    /// source/package and capture identities separately. This method checks current
    /// prepared ownership and rejects staged roots, but does not establish when the
    /// material was updated, scan for retired owners, or prove GPU submission.
    pub fn capture(world: &World, entity: Entity) -> Result<Self> {
        let request = world
            .get_resource::<crate::environment::EnvironmentRequest>()
            .context("bound light observation: environment request absent")?;
        let prepared = world
            .get_resource::<cyrus_world::lifecycle::PreparedWorld>()
            .context("bound light observation: prepared world absent")?;
        let current = world
            .get_resource::<cyrus_script::script::ScriptActors>()
            .and_then(|actors| actors.current.as_ref())
            .context("bound light observation: logical world absent")?;
        ensure!(
            world
                .get_resource::<crate::ReadyScene>()
                .is_some_and(|ready| ready.0 == prepared.animations)
                && super::owns_prepared_environment(
                    current,
                    prepared,
                    request,
                    world.get_resource::<crate::environment::ActiveEnvironment>(),
                    world.get_resource::<cyrus_world::animation::AnimationRequest>(),
                    world.get_resource::<Assets<cyrus_assets::animation::Animations>>(),
                ),
            "world {} generation {}: bound light observation has mismatched prepared ownership",
            current.world,
            current.generation(),
        );
        let mut cursor = Some(entity);
        let mut seen = std::collections::HashSet::new();
        let mut scene_root = false;
        while let Some(node) = cursor {
            ensure!(
                seen.insert(node),
                "bound light entity {entity:?}: hierarchy cycle"
            );
            ensure!(
                world
                    .get::<crate::persistence::StagedRenderRoot>(node)
                    .is_none(),
                "bound light entity {entity:?}: staged hierarchy",
            );
            scene_root |= world.get::<crate::SceneEntity>(node).is_some();
            cursor = world.get::<ChildOf>(node).map(ChildOf::parent);
        }
        ensure!(
            scene_root,
            "bound light entity {entity:?}: live scene root absent"
        );
        let binding = world
            .get::<MeshMaterial3d<SceneMaterial>>(entity)
            .with_context(|| {
                format!(
                    "world {} entity {entity:?}: scene material binding absent",
                    current.world
                )
            })?;
        let material = world
            .get_resource::<Assets<SceneMaterial>>()
            .and_then(|materials| materials.get(&binding.0))
            .with_context(|| {
                format!(
                    "world {} entity {entity:?}: bound scene material absent",
                    current.world
                )
            })?;
        let image = world
            .get_resource::<Assets<Image>>()
            .and_then(|images| images.get(&material.extension.lights))
            .with_context(|| {
                format!(
                    "world {} entity {entity:?}: bound light image absent",
                    current.world
                )
            })?;
        let rows = decode(image).with_context(|| {
            format!(
                "world {} entity {entity:?}: bound light image {:?}",
                current.world,
                material.extension.lights.id()
            )
        })?;
        Ok(Self {
            entity: format!("{entity:?}"),
            material: format!("{:?}", binding.0.id()),
            image: format!("{:?}", material.extension.lights.id()),
            world: current.world,
            generation: current.generation(),
            environment: prepared.environment_path.to_string(),
            dimensions: [
                image.texture_descriptor.size.width,
                image.texture_descriptor.size.height,
            ],
            format: format!("{:?}", image.texture_descriptor.format),
            rows_after_lookup_256: rows,
        })
    }
}

fn decode(image: &Image) -> Result<Vec<[f32; 8]>> {
    let descriptor = &image.texture_descriptor;
    ensure!(
        descriptor.dimension == TextureDimension::D2
            && descriptor.format == TextureFormat::Rgba8Unorm
            && descriptor.size.width == crate::lighting::runtime::LIGHT_TABLE_WIDTH
            && descriptor.size.height >= 256
            && descriptor.size.depth_or_array_layers == 1
            && descriptor.mip_level_count == 1
            && descriptor.sample_count == 1,
        "expected single-layer, single-mip 8-by-(256+lights) Rgba8Unorm image of float bytes",
    );
    let data = image
        .data
        .as_deref()
        .context("CPU image bytes unavailable")?;
    ensure!(
        data.len() % 32 == 0 && data.len() / 32 == descriptor.size.height as usize,
        "light image byte length does not match dimensions"
    );
    data[256 * 32..]
        .as_chunks::<32>()
        .0
        .iter()
        .map(|bytes| {
            let row = std::array::from_fn(|column| {
                let start = column * 4;
                f32::from_le_bytes([
                    bytes[start],
                    bytes[start + 1],
                    bytes[start + 2],
                    bytes[start + 3],
                ])
            });
            ensure!(
                row.iter().all(|value| value.is_finite()),
                "non-finite light image row"
            );
            Ok(row)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{asset::RenderAssetUsages, prelude::Handle, render::render_resource::Extent3d};

    #[test]
    fn image_rows_preserve_signed_values_and_reject_invalid_storage() -> Result<()> {
        let row: [f32; 8] = [1.0, 2.0, 3.0, 100.0, -255.0, 0.0, 127.0, 40.0];
        let mut data = vec![0; 256 * 32];
        data.extend(row.into_iter().flat_map(f32::to_le_bytes));
        let mut image = Image::new(
            Extent3d {
                width: 8,
                height: 257,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            data,
            TextureFormat::Rgba8Unorm,
            RenderAssetUsages::default(),
        );
        assert_eq!(decode(&image)?, vec![row]);
        image.texture_descriptor.format = TextureFormat::Rgba32Float;
        assert!(decode(&image).is_err());
        image.texture_descriptor.format = TextureFormat::Rgba8Unorm;
        image.data.as_mut().unwrap()[256 * 32..256 * 32 + 4]
            .copy_from_slice(&f32::NAN.to_le_bytes());
        assert!(decode(&image).is_err());
        image.data.as_mut().unwrap()[256 * 32..256 * 32 + 4]
            .copy_from_slice(&1.0_f32.to_le_bytes());
        image.data.as_mut().unwrap().pop();
        assert!(decode(&image).is_err());
        image.data = None;
        assert!(decode(&image).is_err());
        Ok(())
    }

    #[test]
    fn named_capture_refuses_missing_and_duplicate_entities() {
        let mut world = World::new();
        assert!(
            BoundSceneLightImage::capture_named_primitive(&mut world, "mesh", 0)
                .unwrap_err()
                .to_string()
                .contains("absent")
        );
        world.spawn(Name::new("mesh"));
        world.spawn(Name::new("mesh"));
        assert!(
            BoundSceneLightImage::capture_named_primitive(&mut world, "mesh", 0)
                .unwrap_err()
                .to_string()
                .contains("ambiguous")
        );
    }

    #[test]
    fn named_node_selects_bound_children_and_rejects_out_of_range_index() {
        let mut world = World::new();
        let node = world.spawn(Name::new("placement")).id();
        world.spawn((ChildOf(node), Name::new("metadata-only")));
        world.spawn((
            ChildOf(node),
            MeshMaterial3d::<SceneMaterial>(Handle::default()),
        ));
        world.spawn((
            ChildOf(node),
            MeshMaterial3d::<SceneMaterial>(Handle::default()),
        ));
        for primitive in [0, 1] {
            // Selecting a bound child reaches ownership validation, not the absent-primitive error.
            assert!(
                BoundSceneLightImage::capture_named_primitive(&mut world, "placement", primitive)
                    .unwrap_err()
                    .to_string()
                    .contains("environment request absent")
            );
        }
        assert!(
            BoundSceneLightImage::capture_named_primitive(&mut world, "placement", 2)
                .unwrap_err()
                .to_string()
                .contains("primitive 2 absent")
        );
    }

    #[test]
    fn capture_rejects_missing_production_ownership() {
        let mut world = World::new();
        let entity = world.spawn_empty().id();
        assert!(
            BoundSceneLightImage::capture(&world, entity)
                .unwrap_err()
                .to_string()
                .contains("environment request absent")
        );
        world.init_resource::<crate::environment::EnvironmentRequest>();
        assert!(
            BoundSceneLightImage::capture(&world, entity)
                .unwrap_err()
                .to_string()
                .contains("prepared world absent")
        );
    }
}
