#![cfg(all(feature = "native", not(target_arch = "wasm32")))]
//! Installed texture frames must survive the shared native/browser world exporter.
use cyrus_assets::{install::GameInstall, source::ReadAssets};
use rgpre::import::bsi::{BsiFile, parse_bsi_file};
use std::collections::{BTreeSet, HashMap};

fn check_polygons(
    scene: &str,
    world: &cyrus_assets::runtime::geometry::WorldGeometry,
    document: &serde_json::Value,
    buffer: &[u8],
) {
    let mut checked = BTreeSet::new();
    let mut count = 0;
    for node in document["nodes"].as_array().unwrap() {
        let Some(mesh) = node["mesh"].as_u64() else {
            continue;
        };
        if !checked.insert(mesh) {
            continue;
        }
        let name = node["name"].as_str().unwrap();
        let primitives = document["meshes"][mesh as usize]["primitives"]
            .as_array()
            .unwrap();
        if name == "Terrain" {
            assert!(
                primitives
                    .iter()
                    .all(|p| p["extras"].get("rgpre_admission").is_none())
            );
            continue;
        }
        let model = &world
            .models
            .iter()
            .find(|m| m.model_name == name)
            .unwrap()
            .model;
        let mut faces = BTreeSet::new();
        for primitive in primitives {
            let admission = &primitive["extras"]["rgpre_admission"];
            assert_eq!(admission["version"], 1, "{scene} {name}");
            assert_eq!(admission["radius_fixed"], model.header.radius);
            let accessor = &document["accessors"]
                [primitive["attributes"]["POSITION"].as_u64().unwrap() as usize];
            let view = &document["bufferViews"][accessor["bufferView"].as_u64().unwrap() as usize];
            let base = view["byteOffset"].as_u64().unwrap_or(0) as usize
                + accessor["byteOffset"].as_u64().unwrap_or(0) as usize;
            let stride = view["byteStride"].as_u64().unwrap_or(12) as usize;
            for polygon in admission["polygons"].as_array().unwrap() {
                let face = polygon["source_face"].as_u64().unwrap() as usize;
                assert!(faces.insert(face), "{scene} {name}: repeated face {face}");
                let original = &model.face_data[face];
                let normal = &model.face_normals[face];
                assert_eq!(
                    polygon["normal"],
                    serde_json::json!([-normal.x, -normal.y, normal.z])
                );
                let vertices = polygon["vertices"].as_array().unwrap();
                assert_eq!(vertices.len(), original.face_vertices.len());
                for (index, corner) in vertices.iter().zip(&original.face_vertices) {
                    let index = index.as_u64().unwrap();
                    assert!(index < accessor["count"].as_u64().unwrap());
                    let offset = base + index as usize * stride;
                    let actual: [f32; 3] = std::array::from_fn(|axis| {
                        let start = offset + axis * 4;
                        f32::from_le_bytes(buffer[start..start + 4].try_into().unwrap())
                    });
                    let vertex = &model.vertex_coords[corner.vertex_index as usize];
                    assert_eq!(
                        actual,
                        [-vertex.x / 20.0, -vertex.y / 20.0, vertex.z / 20.0],
                        "{scene} {name} face {face}"
                    );
                }
                count += 1;
            }
        }
        assert_eq!(
            faces.len(),
            model
                .face_data
                .iter()
                .filter(|face| face.face_vertices.len() >= 3)
                .count()
        );
    }
    assert!(count > 0);
    eprintln!("{scene}: verified {count} authored polygon records");
}

#[test]
fn world_texture_frames_match_installed_images_in_order() {
    let root = std::env::var_os("REDGUARD_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../redguard"));
    let Ok(install) = GameInstall::open(&root) else {
        eprintln!("skipped: no install (texture animation export)");
        return;
    };
    let source = cyrus_assets::native::Files::open(&install).unwrap();
    let mut banks = HashMap::<u16, BsiFile>::new();
    for file in &source.manifest().files {
        let name = file.path.rsplit('/').next().unwrap().to_ascii_uppercase();
        if let Some(id) = name.strip_prefix("TEXBSI.").and_then(|id| id.parse().ok()) {
            banks
                .entry(id)
                .or_insert_with(|| parse_bsi_file(&source.read(&file.path).unwrap()).unwrap());
        }
    }
    let mut checked_frames = 0;
    for scene in ["START", "ISLAND", "BELLTOWR"] {
        let world = cyrus_assets::runtime::geometry::world(&source, scene).unwrap();
        let expected: BTreeSet<_> = world
            .models
            .iter()
            .flat_map(|placed| {
                placed.model.face_data.iter().filter_map(|face| {
                    let rgpre::model3d::TextureData::Texture {
                        texture_id,
                        image_id,
                    } = face.texture_data
                    else {
                        return None;
                    };
                    let id = rgpre::gltf::TextureCache::resolve_texture_id(texture_id, |id| {
                        banks.contains_key(&id)
                    });
                    let image = banks
                        .get(&id)?
                        .images
                        .iter()
                        .find(|image| image.image_index == u16::from(image_id))?;
                    (image.frame_count > 1).then_some(image.name.as_str())
                })
            })
            .collect();
        let bytes = &world.glb;
        let json_len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
        let document: serde_json::Value =
            serde_json::from_slice(&bytes[20..20 + json_len]).unwrap();
        let buffer = &bytes[28 + json_len..];
        check_polygons(scene, &world, &document, buffer);
        let terrain_materials: BTreeSet<_> = document["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|node| node["name"] == "Terrain")
            .flat_map(|node| {
                document["meshes"][node["mesh"].as_u64().unwrap() as usize]["primitives"]
                    .as_array()
                    .unwrap()
            })
            .map(|primitive| primitive["material"].as_u64().unwrap() as usize)
            .collect();
        let mut seen = BTreeSet::new();
        for (material_index, material) in
            document["materials"].as_array().unwrap().iter().enumerate()
        {
            let terrain = terrain_materials.contains(&material_index);
            if terrain {
                assert!(
                    material["alphaMode"].is_null() || material["alphaMode"] == "OPAQUE",
                    "{scene}: terrain material {material_index} must be opaque"
                );
            }
            let Some(animation) = material["extras"].get("rgpre_texture_animation") else {
                continue;
            };
            assert_eq!(animation["version"], 1, "{scene}");
            let name = animation["image_name"].as_str().unwrap();
            seen.insert(name);
            let original = banks
                .values()
                .flat_map(|bank| &bank.images)
                .find(|image| image.name == name)
                .unwrap();
            assert_eq!(animation["delay_ms"], original.anim_delay, "{scene} {name}");
            let textures = animation["textures"].as_array().unwrap();
            assert_eq!(
                textures.len(),
                usize::from(original.frame_count),
                "{scene} {name}"
            );
            assert_eq!(
                material["pbrMetallicRoughness"]["baseColorTexture"]["index"],
                textures[0]
            );
            let first = textures[0].as_u64().unwrap() as usize;
            let sampler = &document["textures"][first]["sampler"];
            let mut transparent = false;
            for (frame, texture) in textures.iter().enumerate() {
                let texture = &document["textures"][texture.as_u64().unwrap() as usize];
                assert_eq!(&texture["sampler"], sampler, "{scene} {name} frame {frame}");
                let image = &document["images"][texture["source"].as_u64().unwrap() as usize];
                let view = &document["bufferViews"][image["bufferView"].as_u64().unwrap() as usize];
                let offset = view["byteOffset"].as_u64().unwrap_or(0) as usize;
                let length = view["byteLength"].as_u64().unwrap() as usize;
                let actual = image::load_from_memory(&buffer[offset..offset + length])
                    .unwrap()
                    .into_rgba8();
                assert_eq!(
                    actual.dimensions(),
                    (u32::from(original.width), u32::from(original.height))
                );
                let mut rgba = original
                    .decode_frame_rgba(frame, Some(&world.palette))
                    .unwrap();
                if terrain {
                    for pixel in rgba.as_chunks_mut::<4>().0 {
                        if pixel[3] == 0 {
                            pixel[..3].copy_from_slice(&world.palette.colors[0]);
                        }
                        pixel[3] = 255;
                    }
                }
                transparent |= rgba.as_chunks::<4>().0.iter().any(|pixel| pixel[3] == 0);
                let flipped: Vec<_> = rgba
                    .chunks_exact(usize::from(original.width) * 4)
                    .rev()
                    .flatten()
                    .copied()
                    .collect();
                assert_eq!(actual.as_raw(), &flipped, "{scene} {name} frame {frame}");
                checked_frames += 1;
            }
            if transparent {
                assert_eq!(material["alphaMode"], "MASK", "{scene} {name}");
            }
        }
        assert!(
            expected.is_subset(&seen),
            "{scene}: missing authored images {:?}",
            expected.difference(&seen).collect::<Vec<_>>()
        );
        eprintln!("{scene}: verified {} animated texture images", seen.len());
    }
    assert!(
        checked_frames > 0,
        "comparison scenes must exercise animated textures"
    );
    eprintln!("verified {checked_frames} installed texture frames");
}
