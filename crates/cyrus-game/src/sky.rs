//! Glide sky material: a BSI plane projected behind scene geometry, per preservation's sky docs.

use crate::fidelity::{Area, Fidelity, Mode};
use bevy::asset::embedded_asset;
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, CompareFunction, RenderPipelineDescriptor, SpecializedMeshPipelineError,
};
use bevy::shader::ShaderRef;
use std::hash::{BuildHasher, RandomState};

#[derive(Component)]
pub(crate) struct Sky {
    velocity: Vec2,
    tick: u64,
}

impl Sky {
    pub(crate) fn new(speed: u8) -> Self {
        let direction = RandomState::new().hash_one(0_u8) & 2047;
        #[allow(clippy::approx_constant)]
        // Preservation's shared engine angle table uses this decimal period.
        let angle = direction as f64 * 6.2831852 / 2048.0;
        Self {
            velocity: Vec2::new(angle.sin() as f32, angle.cos() as f32) * (speed as f32 / 16.0),
            tick: 0,
        }
    }
}

#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub(crate) struct SkyMaterial {
    #[texture(0)]
    #[sampler(1)]
    pub image: Handle<Image>,
    /// Engine level, UV half-span in texels, image width, image height.
    #[uniform(2)]
    pub plane: Vec4,
    /// Encoded Glide fog color shared with the scene background.
    #[uniform(3)]
    pub fog_rgb: Vec4,
    /// U/V scroll in texels, Modern sky flag, unused channel.
    #[uniform(4)]
    pub scroll: Vec4,
}

impl Material for SkyMaterial {
    fn vertex_shader() -> ShaderRef {
        "embedded://cyrus_game/sky.wesl".into()
    }

    fn fragment_shader() -> ShaderRef {
        Self::vertex_shader()
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    fn specialize(
        _: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        _: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.vertex.buffers = vec![
            layout
                .0
                .get_layout(&[Mesh::ATTRIBUTE_POSITION.at_shader_location(0)])?,
        ];
        descriptor.primitive.cull_mode = None;
        if let Some(depth) = &mut descriptor.depth_stencil {
            depth.depth_write_enabled = Some(false);
            depth.depth_compare = Some(CompareFunction::GreaterEqual);
        }
        Ok(())
    }
}

pub(crate) struct SkyPlugin;
impl Plugin for SkyPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "sky.wesl");
        app.add_plugins(MaterialPlugin::<SkyMaterial>::default())
            .add_systems(PostUpdate, animate_sky);
    }
}

fn animate_sky(
    time: Res<Time<Real>>,
    fidelity: Res<Fidelity>,
    mut skies: Query<(&mut Sky, &MeshMaterial3d<SkyMaterial>)>,
    mut materials: ResMut<Assets<SkyMaterial>>,
) {
    let tick = (time.elapsed_secs_f64() * (1_193_182.0 / 65_536.0)).floor() as u64;
    let modern = u8::from(fidelity.mode(Area::Sky) == Mode::Modern) as f32;
    for (mut sky, handle) in &mut skies {
        if tick == sky.tick
            && materials
                .get(&handle.0)
                .is_some_and(|m| m.scroll.z == modern)
        {
            continue;
        }
        let Some(mut material) = materials.get_mut(&handle.0) else {
            continue;
        };
        material.scroll.z = modern;
        for _ in 0..tick.saturating_sub(sky.tick).min(36) {
            material.scroll += sky.velocity.extend(0.0).extend(0.0);
            for index in 0..2 {
                let channel = &mut material.scroll[index];
                if *channel >= 256.0 {
                    *channel -= 256.0;
                } else if *channel <= -256.0 {
                    *channel += 256.0;
                }
            }
        }
        sky.tick = tick;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn sky_scroll_uses_ticks_caps_stalls_and_wraps_in_both_directions() {
        let mut app = App::new();
        app.init_resource::<Time<Real>>()
            .init_resource::<Fidelity>()
            .init_resource::<Assets<SkyMaterial>>()
            .add_systems(Update, animate_sky);
        let handle = app
            .world_mut()
            .resource_mut::<Assets<SkyMaterial>>()
            .add(SkyMaterial {
                image: Handle::default(),
                plane: Vec4::ZERO,
                fog_rgb: Vec4::ZERO,
                scroll: Vec4::new(255.0, -255.0, 0.0, 0.0),
            });
        app.world_mut().spawn((
            Sky {
                velocity: Vec2::new(1.0, -1.0),
                tick: 0,
            },
            MeshMaterial3d(handle.clone()),
        ));
        let scroll = |app: &App| {
            app.world()
                .resource::<Assets<SkyMaterial>>()
                .get(&handle)
                .unwrap()
                .scroll
        };
        app.world_mut()
            .resource_mut::<Time<Real>>()
            .advance_by(Duration::from_millis(100));
        app.update();
        assert_eq!(scroll(&app), Vec4::ZERO);
        app.update();
        assert_eq!(scroll(&app), Vec4::ZERO);
        app.world_mut()
            .resource_mut::<Time<Real>>()
            .advance_by(Duration::from_secs(100));
        app.update();
        assert_eq!(scroll(&app), Vec4::new(36.0, -36.0, 0.0, 0.0));
        app.world_mut()
            .resource_mut::<Fidelity>()
            .set(Area::Sky, Mode::Modern);
        app.update();
        assert_eq!(scroll(&app), Vec4::new(36.0, -36.0, 1.0, 0.0));
        assert_eq!(Sky::new(0).velocity, Vec2::ZERO);
        assert!((Sky::new(6).velocity.length() - 0.375).abs() < 0.00001);
    }
}
