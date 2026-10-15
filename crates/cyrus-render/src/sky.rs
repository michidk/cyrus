//! Glide sky material: a BSI plane projected behind scene geometry, per preservation's sky docs.

use bevy::asset::embedded_asset;
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, CompareFunction, RenderPipelineDescriptor, SpecializedMeshPipelineError,
};
use bevy::shader::ShaderRef;
use cyrus_game::fidelity::Area;
use cyrus_game::fidelity::Fidelity;
use cyrus_game::fidelity::Mode;
use std::hash::{BuildHasher, RandomState};

/// One-shot synchronization with a captured original sky, independent of scene identity.
#[derive(Resource, Default)]
pub struct SkyScrollSync {
    pending: Option<(u16, Vec2)>,
}

impl SkyScrollSync {
    /// Apply a captured 2048-step direction and offsets in texels on the next render update.
    /// This does not reproduce the rest of the original session's shared random stream.
    pub fn set(&mut self, direction: u16, offset: [f32; 2]) -> anyhow::Result<()> {
        anyhow::ensure!(direction < 2048, "sky direction must be below 2048");
        anyhow::ensure!(
            offset
                .iter()
                .all(|v| v.is_finite() && (-256.0..=256.0).contains(v)),
            "sky offsets must be finite and within -256..=256 texels"
        );
        self.pending = Some((direction, Vec2::from_array(offset)));
        Ok(())
    }
}

fn scroll_velocity(direction: u16, speed: f32) -> Vec2 {
    // Preserve the original angle table's rounded turn constant.
    #[allow(clippy::approx_constant)]
    let angle = f64::from(direction) * 6.2831852 / 2048.0;
    Vec2::new(angle.sin() as f32, angle.cos() as f32) * (speed / 16.0)
}

#[derive(Component)]
pub(crate) struct Sky {
    velocity: Vec2,
    speed: f32,
    tick: u64,
}

impl Sky {
    /// Change direction and signed live speed without resetting offsets or timer state.
    /// Follows preservation's sky scrolling console-edit contract.
    pub(crate) fn set_speed(&mut self, direction: u16, value: i32) {
        self.speed = value as f32;
        self.velocity = scroll_velocity(direction, self.speed);
    }

    pub(crate) fn new(speed: u8) -> Self {
        let direction = (RandomState::new().hash_one(0_u8) & 2047) as u16;
        Self {
            velocity: scroll_velocity(direction, f32::from(speed)),
            speed: f32::from(speed),
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
    /// U/V scroll in texels, Modern sky flag, Modern fog flag.
    #[uniform(4)]
    pub scroll: Vec4,
    #[texture(5)]
    #[sampler(6)]
    pub sun_image: Handle<Image>,
    /// Camera-relative Bevy center and half-width.
    #[uniform(7)]
    pub sun_position: Vec4,
    /// Encoded tint and half-height in Bevy units.
    #[uniform(8)]
    pub sun_color: Vec4,
}

impl Material for SkyMaterial {
    fn vertex_shader() -> ShaderRef {
        "embedded://cyrus_render/sky.wesl".into()
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
        app.init_resource::<SkyScrollSync>()
            .add_plugins(MaterialPlugin::<SkyMaterial>::default())
            .add_systems(PostUpdate, animate_sky);
    }
}

fn animate_sky(
    time: Res<Time<Real>>,
    clock: Option<Res<crate::presentation_clock::PresentationClock>>,
    fidelity: Res<Fidelity>,
    mut reference: ResMut<SkyScrollSync>,
    mut skies: Query<(&mut Sky, &MeshMaterial3d<SkyMaterial>)>,
    mut materials: ResMut<Assets<SkyMaterial>>,
) {
    let elapsed = crate::presentation_clock::PresentationClock::elapsed(clock.as_deref(), &time);
    let tick = (elapsed.as_secs_f64() * (1_193_182.0 / 65_536.0)).floor() as u64;
    let modern = u8::from(fidelity.mode(Area::Sky) == Mode::Modern) as f32;
    let modern_fog = u8::from(fidelity.mode(Area::Fog) == Mode::Modern) as f32;
    let reference = if skies.is_empty() {
        None
    } else {
        reference.pending.take()
    };
    for (mut sky, handle) in &mut skies {
        if let Some((direction, offset)) = reference {
            sky.velocity = scroll_velocity(direction, sky.speed);
            sky.tick = tick;
            if let Some(mut material) = materials.get_mut(&handle.0) {
                material.scroll.x = offset.x;
                material.scroll.y = offset.y;
            }
        }
        if tick == sky.tick
            && materials
                .get(&handle.0)
                .is_some_and(|m| m.scroll.z == modern && m.scroll.w == modern_fog)
        {
            continue;
        }
        let Some(mut material) = materials.get_mut(&handle.0) else {
            continue;
        };
        material.scroll.z = modern;
        material.scroll.w = modern_fog;
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
            .init_resource::<SkyScrollSync>()
            .init_resource::<Assets<SkyMaterial>>()
            .add_systems(Update, animate_sky);
        let handle = app
            .world_mut()
            .resource_mut::<Assets<SkyMaterial>>()
            .add(SkyMaterial {
                image: Handle::default(),
                sun_image: Handle::default(),
                sun_position: Vec4::ZERO,
                sun_color: Vec4::ZERO,
                plane: Vec4::ZERO,
                fog_rgb: Vec4::ZERO,
                scroll: Vec4::new(255.0, -255.0, 0.0, 0.0),
            });
        app.world_mut().spawn((
            Sky {
                velocity: Vec2::new(1.0, -1.0),
                speed: 16.0,
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
        app.world_mut()
            .resource_mut::<SkyScrollSync>()
            .set(0, [12.0, 20.0])
            .unwrap();
        app.update();
        assert_eq!(scroll(&app), Vec4::new(12.0, 20.0, 1.0, 0.0));
        app.world_mut()
            .resource_mut::<Time<Real>>()
            .advance_by(Duration::from_millis(55));
        app.update();
        assert_eq!(scroll(&app), Vec4::new(12.0, 21.0, 1.0, 0.0));
        for (direction, offset) in [(2048, [0.0; 2]), (0, [f32::NAN, 0.0]), (0, [257.0, 0.0])] {
            assert!(
                app.world_mut()
                    .resource_mut::<SkyScrollSync>()
                    .set(direction, offset)
                    .is_err()
            );
        }
        assert_eq!(Sky::new(0).velocity, Vec2::ZERO);
        assert!((Sky::new(6).velocity.length() - 0.375).abs() < 0.00001);

        let entity = app
            .world_mut()
            .query_filtered::<Entity, With<Sky>>()
            .single(app.world())
            .unwrap();
        let tick = app.world().get::<Sky>(entity).unwrap().tick;
        app.world_mut()
            .get_mut::<Sky>(entity)
            .unwrap()
            .set_speed(0, 0);
        assert_eq!(app.world().get::<Sky>(entity).unwrap().tick, tick);
        app.world_mut()
            .resource_mut::<Time<Real>>()
            .advance_by(Duration::from_millis(55));
        app.update();
        assert_eq!(scroll(&app), Vec4::new(12.0, 21.0, 1.0, 0.0));
        app.world_mut()
            .get_mut::<Sky>(entity)
            .unwrap()
            .set_speed(0, -257);
        app.update();
        assert_eq!(scroll(&app), Vec4::new(12.0, 21.0, 1.0, 0.0));
        app.world_mut()
            .resource_mut::<Time<Real>>()
            .advance_by(Duration::from_millis(55));
        app.update();
        assert_eq!(scroll(&app), Vec4::new(12.0, 4.9375, 1.0, 0.0));
    }
}
