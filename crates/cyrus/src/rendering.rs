//! Native renderer configuration for software adapters.
use bevy::{
    prelude::*,
    render::{renderer::RenderAdapterInfo, view::NoIndirectDrawing},
};

/// Install after RenderPlugin and before cameras spawn so their draw mode is fixed at creation.
pub(crate) struct SoftwareRenderingPlugin;

impl Plugin for SoftwareRenderingPlugin {
    fn build(&self, _app: &mut App) {}

    fn finish(&self, app: &mut App) {
        if app
            .world()
            .get_resource::<RenderAdapterInfo>()
            .is_some_and(|adapter| adapter.device_type == wgpu_types::DeviceType::Cpu)
        {
            // Software Vulkan spends most of its preparation time on indirect instance buffers.
            app.register_required_components::<Camera3d, NoIndirectDrawing>();
            info!("software renderer: using direct mesh drawing");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn configured(adapter: wgpu_types::DeviceType) -> App {
        let mut app = App::new();
        app.insert_resource(RenderAdapterInfo::new(wgpu_types::AdapterInfo::new(
            adapter,
            wgpu_types::Backend::Vulkan,
        )))
        .add_plugins(SoftwareRenderingPlugin);
        app.finish();
        app
    }

    #[test]
    fn software_cameras_have_direct_drawing_when_spawned_and_replaced() {
        let mut app = configured(wgpu_types::DeviceType::Cpu);
        let first = app.world_mut().spawn(Camera3d::default()).id();
        assert!(app.world().get::<NoIndirectDrawing>(first).is_some());
        app.world_mut().despawn(first);
        let replacement = app.world_mut().spawn(Camera3d::default()).id();
        assert!(app.world().get::<NoIndirectDrawing>(replacement).is_some());
    }

    #[test]
    fn hardware_and_unknown_adapters_keep_the_default_camera_draw_mode() {
        for adapter in [
            wgpu_types::DeviceType::DiscreteGpu,
            wgpu_types::DeviceType::IntegratedGpu,
            wgpu_types::DeviceType::VirtualGpu,
            wgpu_types::DeviceType::Other,
        ] {
            let mut app = configured(adapter);
            let camera = app.world_mut().spawn(Camera3d::default()).id();
            assert!(app.world().get::<NoIndirectDrawing>(camera).is_none());
        }
    }
}
