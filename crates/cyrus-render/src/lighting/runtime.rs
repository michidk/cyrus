//! Active authored object lights and quantized distance tables for model shaders.
use bevy::{
    asset::RenderAssetUsages,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use cyrus_assets::environment::{Environment, LocalLight};

#[derive(Default)]
pub(crate) struct RuntimeLights {
    previous: Option<(AssetId<Environment>, [i32; 2])>,
    image: Handle<Image>,
}

fn region(target: [i32; 2]) -> [i32; 2] {
    target.map(|value| {
        (if value < 4608 {
            0
        } else if value > 60927 {
            56319
        } else {
            value - 4608
        }) >> 9
    })
}

fn selected(lights: &[LocalLight], origin: [i32; 2]) -> Vec<&LocalLight> {
    let mut selected: Vec<_> = lights
        .iter()
        .enumerate()
        .filter_map(|(index, light)| {
            let x = ((light.position[0] & 65535) >> 9) - origin[0];
            let z = ((light.position[2] & 65535) >> 9) - origin[1];
            (light.radius > 0
                && (0..18).contains(&x)
                && (0..18).contains(&z)
                && !((x == 0 || x == 17) && (z == 0 || z == 17)))
                .then_some(((z, x, std::cmp::Reverse(index)), light))
        })
        .collect();
    selected.sort_by_key(|(key, _)| *key);
    selected.into_iter().map(|(_, light)| light).collect()
}

fn texture(lights: &[&LocalLight]) -> Image {
    let mut values = Vec::with_capacity((256 + lights.len()) * 8);
    for index in 0..256_u32 {
        let sample = f32::from_bits((127 + (index >> 7)) << 23 | ((index & 127) << 16));
        let root = f64::from(sample).sqrt();
        values.extend([
            root as f32,
            (1.0 / root) as f32,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
        ]);
    }
    for light in lights {
        let [x, y, z] = light.position.map(|value| value as f32);
        let [r, g, b] = light.rgb.map(f32::from);
        let radius = f32::from(light.radius);
        values.extend([
            x,
            y,
            z,
            radius,
            r,
            g,
            b,
            ((2.0 * radius + radius * radius) + radius) / 256.0,
        ]);
    }
    Image::new(
        Extent3d {
            width: 2,
            height: (256 + lights.len()) as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        values.into_iter().flat_map(f32::to_le_bytes).collect(),
        TextureFormat::Rgba32Float,
        RenderAssetUsages::default(),
    )
}

impl RuntimeLights {
    pub(crate) fn update(
        &mut self,
        source: AssetId<Environment>,
        environment: &Environment,
        position: Vec3,
        images: &mut Assets<Image>,
    ) -> Handle<Image> {
        let origin = region([
            (-f64::from(position.x) * 20.0).floor() as i32,
            (f64::from(position.z) * 20.0 + 65535.99609375).floor() as i32,
        ]);
        if self.previous != Some((source, origin)) {
            let image = texture(&selected(&environment.object_lights, origin));
            if let Some(mut old) = images.get_mut(&self.image) {
                *old = image;
            } else {
                self.image = images.add(image);
            }
            self.previous = Some((source, origin));
        }
        self.image.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn region_clamps_edges_and_selects_row_order_without_corners() {
        for (target, expected) in [
            ([0, 0], [0, 0]),
            ([5120, 5119], [1, 0]),
            ([60927, 60928], [109, 109]),
            ([65535, 65535], [109, 109]),
        ] {
            assert_eq!(region(target), expected);
        }
        let lights: Vec<_> = [(1, 1), (0, 0), (17, 17), (2, 0), (1, 1), (0, 1), (18, 1)]
            .into_iter()
            .enumerate()
            .map(|(index, (x, z))| LocalLight {
                position: [x * 512, index as i32, z * 512],
                radius: 100,
                rgb: [255, 0, -255],
            })
            .collect();
        assert_eq!(
            selected(&lights, [0, 0])
                .iter()
                .map(|light| light.position[1])
                .collect::<Vec<_>>(),
            [3, 5, 4, 0]
        );
        let image = texture(&selected(&lights, [0, 0]));
        assert_eq!(image.texture_descriptor.size.height, 260);
        let data = image.data.unwrap();
        let value = |offset| f32::from_le_bytes(data[offset..offset + 4].try_into().unwrap());
        assert_eq!(value(0), 1.0);
        assert_eq!(value(4), 1.0);
        assert_eq!(value(256 * 32), 1024.0);
        assert_eq!(value(256 * 32 + 24), -255.0);
    }
}
