#![cfg(all(feature = "native", not(target_arch = "wasm32")))]
//! Installed texture frames must survive the shared native/browser world exporter.
use cyrus_assets::{install::GameInstall, source::ReadAssets};
use rgpre::import::bsi::{BsiFile, parse_bsi_file};
use std::collections::{BTreeSet, HashMap};

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
        let mut seen = BTreeSet::new();
        for material in document["materials"].as_array().unwrap() {
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
                let rgba = original
                    .decode_frame_rgba(frame, Some(&world.palette))
                    .unwrap();
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
