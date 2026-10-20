//! Centered effect billboards use the same local axes and lighting inputs as placed flats.
use super::super::{EffectDraw, FogExtension, SceneMaterial};
use anyhow::{Result, ensure};
use bevy::{
    asset::RenderAssetUsages,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
};

pub(super) fn mesh(draw: &EffectDraw) -> Result<Mesh> {
    ensure!(
        draw.flags & 1 != 0 && draw.flags & !0x363 == 0,
        "effect billboard drawing flags {:#x} unsupported",
        draw.flags
    );
    let mut half = draw.half_extents / 20.0;
    if draw.flags & 0x20 != 0 {
        half.x = -half.x;
    }
    if draw.flags & 0x40 != 0 {
        half.y = -half.y;
    }
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_POSITION,
        vec![
            [-half.x, half.y, 0.0],
            [half.x, half.y, 0.0],
            [half.x, -half.y, 0.0],
            [-half.x, -half.y, 0.0],
        ],
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; 4]);
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_UV_0,
        vec![[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]],
    );
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_UV_1,
        vec![[f32::from(draw.brightness), 1.0]; 4],
    );
    mesh.insert_indices(Indices::U32(vec![0, 2, 1, 0, 3, 2]));
    Ok(mesh)
}

pub(super) fn material(
    draw: &EffectDraw,
    fog: &FogExtension,
    lights: &Handle<Image>,
) -> SceneMaterial {
    let flat = crate::flat::Flat {
        brightness: Some(draw.brightness),
        alpha: draw.alpha,
        source_index: 0,
        radius: draw.radius,
    };
    let base = StandardMaterial {
        base_color_texture: Some(draw.texture.clone()),
        cull_mode: None,
        double_sided: true,
        ..default()
    };
    let mut material = super::super::model_material(
        &base,
        fog,
        false,
        Some(&flat),
        None,
        0,
        None,
        Some(3),
        Vec4::new(0.0, 0.0, 0.0, 1.0),
        UVec4::ZERO,
        Vec4::ZERO,
        lights,
    );
    material.extension.compositing.y = f32::from(draw.flags);
    material
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::mesh::VertexAttributeValues;

    #[test]
    fn centered_geometry_reverses_axes_without_reversing_the_lighting_normal() -> Result<()> {
        for (flags, x, y) in [
            (1, 2.0, 3.0),
            (0x21, -2.0, 3.0),
            (0x41, 2.0, -3.0),
            (0x61, -2.0, -3.0),
        ] {
            let draw = EffectDraw {
                texture: Handle::default(),
                frame: 0,
                half_extents: Vec2::new(40.0, 60.0),
                radius: 70.0,
                brightness: 78,
                alpha: 140,
                flags,
            };
            let mesh = mesh(&draw)?;
            assert_eq!(
                mesh.attribute(Mesh::ATTRIBUTE_POSITION),
                Some(&VertexAttributeValues::Float32x3(vec![
                    [-x, y, 0.0],
                    [x, y, 0.0],
                    [x, -y, 0.0],
                    [-x, -y, 0.0]
                ]))
            );
            assert_eq!(
                mesh.attribute(Mesh::ATTRIBUTE_NORMAL),
                Some(&VertexAttributeValues::Float32x3(vec![[0.0, 0.0, 1.0]; 4]))
            );
            assert_eq!(
                mesh.attribute(Mesh::ATTRIBUTE_UV_1),
                Some(&VertexAttributeValues::Float32x2(vec![[78.0, 1.0]; 4]))
            );
        }
        Ok(())
    }
}
