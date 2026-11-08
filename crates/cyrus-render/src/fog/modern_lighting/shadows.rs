//! Shadow-only twins of the models and terrain, so the sun's shadow cascades batch their draws.
//!
//! Every model and terrain piece has its own material, because the material carries the
//! original's per-object placement, lighting and compositing. Bevy batches shadow draws by
//! material, so the shadow cascades drew each object on its own, which cost most of a Modern
//! frame. The shadow pass only writes depth, so a caster's material does not matter there:
//! each shadow caster gets a child that shares its mesh and one plain material and is only on
//! [`SHADOW_LAYER`], which the sun lights and the camera does not see. The original stops
//! casting. Flats and chroma-keyed models skip the shadow prepass and stay without a twin.
use super::super::{SceneMaterial, TerrainMaterial};
use crate::camera::ViewCamera;
use bevy::{
    camera::{primitives::Aabb, visibility::RenderLayers},
    light::{NotShadowCaster, NotShadowReceiver},
    pbr::{ExtendedMaterial, MaterialExtension},
    prelude::*,
    render::render_resource::AsBindGroup,
};

/// The render layer only the shadow twins and the sun are on.
pub(super) const SHADOW_LAYER: usize = 7;

/// The layers the sun lights: what the camera sees, and the twins for its shadow cascades.
pub(super) fn sun_layers() -> RenderLayers {
    RenderLayers::default().with(SHADOW_LAYER)
}

/// A shadow-only child that copies its parent's mesh and world transform.
#[derive(Component)]
pub(super) struct ShadowTwin;

/// A caster whose shadow its [`ShadowTwin`] child casts instead.
#[derive(Component)]
pub(super) struct Twinned;

/// The twins' material: a plain standard material under its own type, because the scene
/// preparation treats every standard-material mesh as one it still has to convert.
pub(super) type ShadowMaterial = ExtendedMaterial<StandardMaterial, ShadowExtension>;

#[derive(Asset, TypePath, AsBindGroup, Clone, Default)]
pub(super) struct ShadowExtension {}

impl MaterialExtension for ShadowExtension {}

/// The one material every twin shares, so their shadow draws batch.
#[derive(Resource)]
pub(super) struct TwinMaterial(Handle<ShadowMaterial>);

impl FromWorld for TwinMaterial {
    fn from_world(world: &mut World) -> Self {
        Self(
            world
                .resource_mut::<Assets<ShadowMaterial>>()
                .add(ShadowMaterial {
                    base: StandardMaterial {
                        unlit: true,
                        ..default()
                    },
                    extension: ShadowExtension::default(),
                }),
        )
    }
}

/// Gives every shadow-casting model and terrain piece a twin.
// Bevy supplies the candidate queries and material assets as separate parameters.
#[allow(clippy::type_complexity)]
pub(super) fn add_twins(
    mut commands: Commands,
    material: Res<TwinMaterial>,
    models: Query<
        (Entity, &Mesh3d, &MeshMaterial3d<SceneMaterial>),
        (
            Without<Twinned>,
            Without<NotShadowCaster>,
            Without<ShadowTwin>,
        ),
    >,
    terrain: Query<
        (Entity, &Mesh3d),
        (
            With<MeshMaterial3d<TerrainMaterial>>,
            Without<Twinned>,
            Without<NotShadowCaster>,
        ),
    >,
    scene_materials: Res<Assets<SceneMaterial>>,
) {
    let models = models.iter().filter_map(|(entity, mesh, handle)| {
        let extension = &scene_materials.get(handle)?.extension;
        // Flats are sprites and keyed models cut out texels; neither writes the shadow prepass.
        let casts = extension.compositing.x < 0.0 && extension.chroma_key.w == 0.0;
        casts.then_some((entity, mesh))
    });
    for (entity, mesh) in models.chain(&terrain) {
        commands
            .entity(entity)
            .insert((Twinned, NotShadowCaster))
            .with_child((
                ShadowTwin,
                mesh.clone(),
                MeshMaterial3d(material.0.clone()),
                RenderLayers::layer(SHADOW_LAYER),
                NotShadowReceiver,
            ));
    }
}

/// Keeps each twin on its parent's current mesh, bounds and world transform, and off while the camera
/// does not see its parent. Script poses write world transforms directly, so propagation alone
/// can leave a child behind.
// Bevy supplies the twin and parent queries as separate parameters.
#[allow(clippy::type_complexity)]
pub(super) fn follow(
    mut twins: Query<
        (
            &ChildOf,
            &mut Mesh3d,
            &mut GlobalTransform,
            &mut RenderLayers,
            Option<&mut Aabb>,
        ),
        With<ShadowTwin>,
    >,
    parents: Query<
        (
            &Mesh3d,
            &GlobalTransform,
            Option<&RenderLayers>,
            Option<&Aabb>,
        ),
        Without<ShadowTwin>,
    >,
    cameras: Query<Option<&RenderLayers>, (With<ViewCamera>, Without<ShadowTwin>)>,
) {
    let seen = cameras.iter().fold(RenderLayers::none(), |seen, layers| {
        seen.union(layers.unwrap_or(&RenderLayers::default()))
    });
    let shadow = RenderLayers::layer(SHADOW_LAYER);
    for (parent, mut mesh, mut transform, mut layers, bounds) in &mut twins {
        let Ok((parent_mesh, parent_transform, parent_layers, parent_bounds)) =
            parents.get(parent.parent())
        else {
            continue;
        };
        if mesh.0 != parent_mesh.0 {
            mesh.0 = parent_mesh.0.clone();
        }
        transform.set_if_neq(*parent_transform);
        if let (Some(mut bounds), Some(parent_bounds)) = (bounds, parent_bounds) {
            bounds.set_if_neq(*parent_bounds);
        }
        let visible = parent_layers
            .unwrap_or(&RenderLayers::default())
            .intersects(&seen);
        let wanted = if visible {
            &shadow
        } else {
            &RenderLayers::none()
        };
        if *layers != *wanted {
            *layers = wanted.clone();
        }
    }
}

/// Removes the twins and lets the originals cast again.
pub(super) fn remove_twins(
    commands: &mut Commands,
    twins: &Query<Entity, With<ShadowTwin>>,
    twinned: &Query<Entity, With<Twinned>>,
) {
    for twin in twins {
        commands.entity(twin).despawn();
    }
    for entity in twinned {
        commands
            .entity(entity)
            .remove::<(Twinned, NotShadowCaster)>();
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::{FogExtension, TerrainExtension, model_material};
    use super::*;

    fn fog() -> FogExtension {
        FogExtension {
            color: Vec4::ZERO,
            clipping: Vec4::ZERO,
            table: [Vec4::ZERO; 16],
            water: Vec4::ZERO,
            ambient: Vec4::ZERO,
            directional: Vec4::ZERO,
            direction: Vec4::ZERO,
            lighting: Vec4::X,
        }
    }

    fn model(world: &mut World, chroma_key: Vec4) -> Entity {
        let material = model_material(
            &StandardMaterial::default(),
            &fog(),
            false,
            None,
            None,
            0,
            None,
            None,
            chroma_key,
            UVec4::ZERO,
            Vec4::ZERO,
            &Handle::default(),
        );
        let handle = world.resource_mut::<Assets<SceneMaterial>>().add(material);
        let mesh = world.resource_mut::<Assets<Mesh>>().add(Cuboid::default());
        world.spawn((Mesh3d(mesh), MeshMaterial3d(handle))).id()
    }

    fn twin_of(world: &mut World, parent: Entity) -> Option<Entity> {
        world
            .query_filtered::<(Entity, &ChildOf), With<ShadowTwin>>()
            .iter(world)
            .find(|(_, child_of)| child_of.parent() == parent)
            .map(|(twin, _)| twin)
    }

    #[test]
    fn casters_get_a_shared_shadow_twin_that_follows_them_until_removed() {
        let mut world = World::new();
        world.init_resource::<Assets<Mesh>>();
        world.init_resource::<Assets<SceneMaterial>>();
        world.init_resource::<Assets<TerrainMaterial>>();
        world.init_resource::<Assets<ShadowMaterial>>();
        world.init_resource::<TwinMaterial>();
        world.spawn(ViewCamera);
        let plain = model(&mut world, Vec4::ZERO);
        let keyed = model(&mut world, Vec4::ONE);
        let terrain_material =
            world
                .resource_mut::<Assets<TerrainMaterial>>()
                .add(TerrainMaterial {
                    base: StandardMaterial::default(),
                    extension: TerrainExtension { fog: fog() },
                });
        let terrain = world
            .spawn((Mesh3d(Handle::default()), MeshMaterial3d(terrain_material)))
            .id();

        world.run_system_cached(add_twins).unwrap();
        world.run_system_cached(add_twins).unwrap();
        for caster in [plain, terrain] {
            assert!(world.entity(caster).contains::<NotShadowCaster>());
            assert!(twin_of(&mut world, caster).is_some());
        }
        // A keyed model's cut-out texels write no shadow prepass, so it keeps casting nothing.
        assert!(!world.entity(keyed).contains::<NotShadowCaster>());
        assert!(twin_of(&mut world, keyed).is_none());
        assert_eq!(world.query::<&ShadowTwin>().iter(&world).count(), 2);

        let twin = twin_of(&mut world, plain).unwrap();
        let animated = world.resource_mut::<Assets<Mesh>>().add(Sphere::default());
        world.entity_mut(plain).insert((
            Mesh3d(animated.clone()),
            GlobalTransform::from_xyz(1.0, 2.0, 3.0),
        ));
        world.run_system_cached(follow).unwrap();
        let twin_ref = world.entity(twin);
        assert_eq!(twin_ref.get::<Mesh3d>().unwrap().0, animated);
        assert_eq!(
            twin_ref.get::<GlobalTransform>().unwrap().translation(),
            Vec3::new(1.0, 2.0, 3.0)
        );
        assert_eq!(
            twin_ref.get::<RenderLayers>(),
            Some(&RenderLayers::layer(SHADOW_LAYER))
        );
        // A caster the camera does not see casts no shadow through its twin either.
        world.entity_mut(plain).insert(RenderLayers::none());
        world.run_system_cached(follow).unwrap();
        assert_eq!(
            world.entity(twin).get::<RenderLayers>(),
            Some(&RenderLayers::none())
        );

        world
            .run_system_cached(
                |mut commands: Commands,
                 twins: Query<Entity, With<ShadowTwin>>,
                 twinned: Query<Entity, With<Twinned>>| {
                    remove_twins(&mut commands, &twins, &twinned);
                },
            )
            .unwrap();
        assert_eq!(world.query::<&ShadowTwin>().iter(&world).count(), 0);
        assert!(!world.entity(plain).contains::<NotShadowCaster>());
        assert!(!world.entity(terrain).contains::<NotShadowCaster>());
    }
}
