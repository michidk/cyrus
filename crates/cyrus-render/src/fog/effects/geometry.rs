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
        draw.flags & EffectDraw::BASE != 0
            && draw.flags & !EffectDraw::SUPPORTED == 0
            && (draw.flags & EffectDraw::SPECIAL != 0) == draw.constant_rgb.is_some(),
        "effect billboard drawing flags {:#x} unsupported",
        draw.flags
    );
    let mut half = draw.half_extents / 20.0;
    if draw.flags & EffectDraw::MIRROR_X != 0 {
        half.x = -half.x;
    }
    if draw.flags & EffectDraw::MIRROR_Y != 0 {
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
    if let Some(rgb) = draw.constant_rgb {
        material.extension.solid_color = Vec3::from_array(rgb.map(f32::from)).extend(1.0);
        material.extension.compositing.z = 1.0;
        material.extension.lighting.z = 0.0;
    }
    material
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::mesh::VertexAttributeValues;

    #[test]
    fn special_actor_constant_rgb_bypasses_texture_color_and_model_lighting() -> Result<()> {
        let fog = FogExtension {
            color: Vec4::ZERO,
            clipping: Vec4::new(1.0, 1000.0, 0.0, 0.0),
            table: [Vec4::ZERO; 16],
            water: Vec4::ZERO,
            ambient: Vec4::splat(71.0),
            directional: Vec4::splat(137.0),
            direction: Vec4::new(0.0, 0.0, -1.0, 0.0),
        };
        let mut draw = EffectDraw {
            texture: Handle::default(),
            frame: 0,
            half_extents: Vec2::new(8.0, 4.0),
            radius: 9.0,
            brightness: 255,
            alpha: 255,
            flags: 0x281,
            constant_rgb: Some([200; 3]),
        };
        mesh(&draw)?;
        let special = material(&draw, &fog, &Handle::default());
        assert_eq!(special.base.base_color_texture, Some(draw.texture.clone()));
        assert_eq!(
            special.extension.solid_color,
            Vec4::new(200.0, 200.0, 200.0, 1.0)
        );
        assert_eq!(special.extension.compositing.x, 255.0);
        assert_eq!(special.extension.compositing.z, 1.0);
        assert_eq!(special.extension.lighting.z, 0.0);
        assert!(!crate::fog::ModelKey::from(&special.extension).write_depth);
        draw.constant_rgb = None;
        assert!(mesh(&draw).is_err());
        draw.flags = 1;
        mesh(&draw)?;
        let ordinary = material(&draw, &fog, &Handle::default());
        assert_eq!(ordinary.extension.compositing.z, 0.0);
        assert_eq!(ordinary.extension.lighting.z, 1.0);
        assert!(ordinary.extension != special.extension);
        Ok(())
    }

    #[test]
    fn centered_special_and_ordinary_geometry_keep_axis_flips_and_lighting_normal() -> Result<()> {
        for (flags, x, y) in [
            (1, 2.0, 3.0),
            (0x21, -2.0, 3.0),
            (0x41, 2.0, -3.0),
            (0x61, -2.0, -3.0),
            // Ignored FLAT bits draw like cleared bits.
            (0x1f, 2.0, 3.0),
            (0x281, 2.0, 3.0),
            (0x2a1, -2.0, 3.0),
            (0x2c1, 2.0, -3.0),
            (0x2e1, -2.0, -3.0),
        ] {
            let special = flags & 0x80 != 0;
            let brightness = if special { 255 } else { 78 };
            let draw = EffectDraw {
                texture: Handle::default(),
                frame: 0,
                half_extents: Vec2::new(40.0, 60.0),
                radius: 70.0,
                brightness,
                alpha: if special { 255 } else { 140 },
                flags,
                constant_rgb: special.then_some([200; 3]),
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
                Some(&VertexAttributeValues::Float32x2(vec![
                    [
                        f32::from(brightness),
                        1.0
                    ];
                    4
                ]))
            );
        }
        Ok(())
    }
}
