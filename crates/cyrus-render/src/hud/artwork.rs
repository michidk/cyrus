//! Ordered framebuffer storage for opaque HUD, inventory and island-map artwork.
use bevy::prelude::*;
use std::collections::HashMap;

/// Image in the HUD's logical 640×480 coordinate space, including its modal panels.
#[derive(Component)]
pub(super) struct StoredArtwork;

#[derive(Default, Resource)]
pub(super) struct ArtworkStorage {
    variants: HashMap<(AssetId<Image>, [u32; 2]), Handle<Image>>,
    sources: HashMap<AssetId<Image>, Handle<Image>>,
}

/// Source images remain immutable: the same icon can occupy different grid cells.
pub(super) fn store_artwork(
    mut nodes: Query<(&Node, &mut ImageNode), With<StoredArtwork>>,
    mut images: ResMut<Assets<Image>>,
    mut storage: ResMut<ArtworkStorage>,
) {
    for (node, mut image_node) in &mut nodes {
        let (Val::Percent(x), Val::Percent(y)) = (node.left, node.top) else {
            continue;
        };
        let origin =
            [(x * 6.4).round() as i32, (y * 4.8).round() as i32].map(|v| v.rem_euclid(4) as u32);
        let source = storage
            .sources
            .get(&image_node.image.id())
            .cloned()
            .unwrap_or_else(|| image_node.image.clone());
        let key = (source.id(), origin);
        if let Some(stored) = storage.variants.get(&key) {
            if image_node.image != *stored {
                image_node.image = stored.clone();
            }
            continue;
        }
        let Some(image) = images.get(&source) else {
            continue;
        };
        if image.texture_descriptor.format
            != bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb
        {
            continue;
        }
        let mut image = image.clone();
        let width = image.width();
        let Some(data) = image.data.as_mut() else {
            continue;
        };
        for (index, pixel) in data.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            // Translucent artwork must be stored after destination blending; leave that
            // separate composition path intact rather than quantizing its source colour.
            if pixel[3] == 255 {
                let colour = cyrus_assets::framebuffer::store(
                    [pixel[0], pixel[1], pixel[2]],
                    origin[0] + index as u32 % width,
                    origin[1] + index as u32 / width,
                );
                pixel[..3].copy_from_slice(&colour);
            }
        }
        let stored = images.add(image);
        storage.sources.insert(stored.id(), source);
        storage.variants.insert(key, stored.clone());
        image_node.image = stored;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{
        asset::RenderAssetUsages,
        render::render_resource::{Extent3d, TextureDimension, TextureFormat},
    };

    #[test]
    fn artwork_stores_opaque_pixels_at_each_grid_origin_without_mutating_source() {
        let mut app = App::new();
        app.init_resource::<Assets<Image>>()
            .init_resource::<ArtworkStorage>()
            .add_systems(Update, store_artwork);
        let source = app
            .world_mut()
            .resource_mut::<Assets<Image>>()
            .add(Image::new(
                Extent3d {
                    width: 2,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                TextureDimension::D2,
                vec![100, 100, 100, 255, 100, 100, 100, 0],
                TextureFormat::Rgba8UnormSrgb,
                RenderAssetUsages::default(),
            ));
        let a = app
            .world_mut()
            .spawn((
                StoredArtwork,
                super::super::place([0, 0], [2, 1]),
                ImageNode::new(source.clone()),
            ))
            .id();
        let b = app
            .world_mut()
            .spawn((
                StoredArtwork,
                super::super::place([105, 102], [2, 1]),
                ImageNode::new(source.clone()),
            ))
            .id();
        app.update();
        let read = |app: &App, entity| {
            let handle = &app.world().get::<ImageNode>(entity).unwrap().image;
            app.world()
                .resource::<Assets<Image>>()
                .get(handle)
                .unwrap()
                .data
                .clone()
                .unwrap()
        };
        assert_eq!(read(&app, a), [99, 97, 99, 255, 100, 100, 100, 0]);
        assert_eq!(read(&app, b), [99, 101, 99, 255, 100, 100, 100, 0]);
        assert_eq!(
            app.world()
                .resource::<Assets<Image>>()
                .get(&source)
                .unwrap()
                .data
                .as_ref()
                .unwrap(),
            &[100, 100, 100, 255, 100, 100, 100, 0]
        );
        let count = app.world().resource::<Assets<Image>>().len();
        app.update();
        assert_eq!(read(&app, a), [99, 97, 99, 255, 100, 100, 100, 0]);
        assert_eq!(app.world().resource::<Assets<Image>>().len(), count);
    }
}
