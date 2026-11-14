//! Scene-lit owned geometry and read-only retained terrain material access for native mods.
use super::*;
use bevy::render::render_resource::{RenderPipelineDescriptor, SpecializedMeshPipelineError};

/// Shared raster material. UV x is a phase and UV y a deformation weight when the
/// active shader library uses them. Parameters are interpreted by that library.
pub type GeometryMaterial = ExtendedMaterial<StandardMaterial, GeometryExtension>;

#[derive(Asset, TypePath, AsBindGroup, Clone)]
#[uniform(50, GeometryUniform)]
pub struct GeometryExtension {
    fog: FogExtension,
    pub parameters: Vec4,
}

#[derive(ShaderType)]
struct GeometryUniform {
    fog: FogExtension,
    parameters: Vec4,
}
impl From<&GeometryExtension> for GeometryUniform {
    fn from(value: &GeometryExtension) -> Self {
        Self {
            fog: value.fog.clone(),
            parameters: value.parameters,
        }
    }
}
impl MaterialExtension for GeometryExtension {
    fn vertex_shader() -> ShaderRef {
        "embedded://cyrus_render/fog/geometry.wesl".into()
    }
    fn fragment_shader() -> ShaderRef {
        Self::vertex_shader()
    }
    fn specialize(
        _pipeline: &bevy::pbr::MaterialExtensionPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &bevy::mesh::MeshVertexBufferLayoutRef,
        _key: bevy::pbr::MaterialExtensionKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        if is_prepass(descriptor) {
            skip_prepass(descriptor);
        } else {
            define_modern_lighting(descriptor);
        }
        Ok(())
    }
}

/// Current scene fog/light context. None before the requested environment is ready.
#[derive(Resource, Default)]
pub struct GeometryContext(Option<FogExtension>);
impl GeometryContext {
    /// Prepare the same fog/light context for a staged or isolated native scene.
    /// The caller supplies a validated environment and clipping distances in engine units.
    pub fn from_environment(
        environment: &Environment,
        fidelity: &Fidelity,
        clipping: [f32; 2],
    ) -> Self {
        Self(Some(fog_extension(environment, fidelity, clipping)))
    }

    /// Create a double-sided matte scene material, with linear vertex colors.
    pub fn material(&self, parameters: Vec4) -> Option<GeometryMaterial> {
        Some(GeometryMaterial {
            base: StandardMaterial {
                base_color: Color::WHITE,
                perceptual_roughness: 1.0,
                reflectance: 0.0,
                double_sided: true,
                cull_mode: None,
                ..default()
            },
            extension: GeometryExtension {
                fog: self.0.clone()?,
                parameters,
            },
        })
    }
}

/// Terrain texture bound to an actual live terrain entity, with its original UVs.
pub fn terrain_texture(world: &World, entity: Entity) -> Option<Handle<Image>> {
    let binding = world.get::<MeshMaterial3d<TerrainMaterial>>(entity)?;
    world
        .get_resource::<Assets<TerrainMaterial>>()?
        .get(&binding.0)?
        .base
        .base_color_texture
        .clone()
}

pub(super) fn register(app: &mut App) {
    embedded_asset!(app, "geometry.wesl");
    app.init_resource::<GeometryContext>()
        .add_plugins(MaterialPlugin::<GeometryMaterial>::default())
        .add_systems(PostUpdate, refresh);
}
fn refresh(
    request: Option<Res<EnvironmentRequest>>,
    environments: Res<Assets<Environment>>,
    fidelity: Option<Res<Fidelity>>,
    distance: Res<RenderDistance>,
    daylight: Option<Res<Daylight>>,
    mut context: ResMut<GeometryContext>,
    mut materials: ResMut<Assets<GeometryMaterial>>,
) {
    context.0 = request
        .as_ref()
        .zip(fidelity.as_ref())
        .and_then(|(request, fidelity)| {
            let environment = environments.get(&request.0)?;
            let clipping = distance.effective(fidelity)?;
            let mut fog = fog_extension(environment, fidelity, clipping);
            if let Some(sky) = daylight.as_ref().and_then(|daylight| daylight.0) {
                fog.color = (fog.color.truncate() * sky.tint).extend(fog.color.w);
            }
            Some(fog)
        });
    if let Some(fog) = &context.0 {
        for (_, material) in materials.iter_mut() {
            if material.extension.fog != *fog {
                material.extension.fog = fog.clone();
            }
        }
    }
}
