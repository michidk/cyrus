//! Original splash-file members and object-space mesh presentation through preservation.
use crate::{Category, Source};
use anyhow::{Context, Result, ensure};
use rgpre::import::{
    glide_splash::{SplashMesh, SplashTexture, parse_glide_splash},
    glide_splash_dll::parse_windows_glide_splash,
};
use serde_json::{Value, json};

pub(super) fn is_source(path: &str) -> bool {
    path.rsplit('/').next().is_some_and(|name| {
        name.eq_ignore_ascii_case("3DfxSpl2.dll") || name.eq_ignore_ascii_case("glide2x_emu.ovl")
    })
}

pub(super) struct Splash {
    pub meshes: Vec<SplashMesh>,
    pub textures: Vec<SplashTexture>,
    sphere_mapped: Option<Vec<bool>>,
}

impl Splash {
    pub fn read(path: &str, bytes: &[u8]) -> Result<Self> {
        if path.to_ascii_lowercase().ends_with(".dll") {
            let splash = parse_windows_glide_splash(bytes)?;
            Ok(Self {
                meshes: splash.meshes,
                textures: splash.textures,
                sphere_mapped: Some(splash.sphere_mapped),
            })
        } else {
            let splash = parse_glide_splash(bytes)?;
            Ok(Self {
                meshes: splash.meshes.into(),
                textures: splash.textures.into(),
                sphere_mapped: None,
            })
        }
    }

    pub fn members(&self) -> Vec<(Category, Source, String)> {
        (0..self.meshes.len())
            .map(|index| {
                (
                    Category::Models,
                    Source::SplashMesh(index),
                    format!("Mesh {index}"),
                )
            })
            .chain((0..self.textures.len()).map(|index| {
                (
                    Category::Images,
                    Source::Splash(index),
                    format!("Texture {index}"),
                )
            }))
            .collect()
    }

    pub fn metadata(&self) -> Value {
        json!({
            "meshes": self.meshes.len(),
            "textures": self.textures.iter().map(|texture| json!({
                "width": texture.width, "height": texture.height,
            })).collect::<Vec<_>>(),
        })
    }

    pub fn mesh_metadata(&self, index: usize) -> Result<Value> {
        let mesh = self.meshes.get(index).context("missing splash mesh")?;
        Ok(json!({
            "mesh": index,
            "coordinate_space": "authored object space",
            "positions": mesh.vertices.iter().map(|vertex| vertex.position).collect::<Vec<_>>(),
            "normals": mesh.vertices.iter().map(|vertex| vertex.normal).collect::<Vec<_>>(),
            "authored_uvs": mesh.vertices.iter().map(|vertex| vertex.uv).collect::<Vec<_>>(),
            "faces": mesh.faces.iter().map(|face| json!({
                "vertices": face.vertices, "material": face.material,
                "antialiased_edges": face.antialiased_edges,
            })).collect::<Vec<_>>(),
            "sphere_mapped": self.sphere_mapped.as_ref().and_then(|flags| flags.get(index)),
        }))
    }
}

/// Adapts parsed geometry to the viewer's GLB transport without applying game transforms.
pub(super) fn mesh_glb(mesh: &SplashMesh) -> Result<Vec<u8>> {
    ensure!(
        !mesh.vertices.is_empty() && !mesh.faces.is_empty(),
        "empty splash mesh"
    );
    let mut data = Vec::new();
    let mut minimum = [f32::INFINITY; 3];
    let mut maximum = [f32::NEG_INFINITY; 3];
    for vertex in &mesh.vertices {
        for axis in 0..3 {
            minimum[axis] = minimum[axis].min(vertex.position[axis]);
            maximum[axis] = maximum[axis].max(vertex.position[axis]);
            data.extend_from_slice(&vertex.position[axis].to_le_bytes());
        }
    }
    let normals_offset = data.len();
    for vertex in &mesh.vertices {
        for value in vertex.normal {
            data.extend_from_slice(&value.to_le_bytes());
        }
    }
    let indices_offset = data.len();
    for face in &mesh.faces {
        for index in face.vertices {
            ensure!(
                index < mesh.vertices.len(),
                "splash face vertex {index} exceeds mesh vertex count"
            );
            data.extend_from_slice(&u32::try_from(index)?.to_le_bytes());
        }
    }
    let document = serde_json::from_value(json!({
        "asset": {"version": "2.0"},
        "scene": 0, "scenes": [{"nodes": [0]}], "nodes": [{"mesh": 0}],
        "meshes": [{"primitives": [{"attributes": {"POSITION": 0, "NORMAL": 1}, "indices": 2, "material": 0}]}],
        "materials": [{"doubleSided": true, "pbrMetallicRoughness": {"metallicFactor": 0.0, "roughnessFactor": 1.0}}],
        "buffers": [{"byteLength": data.len()}],
        "bufferViews": [
            {"buffer": 0, "byteOffset": 0, "byteLength": normals_offset, "target": 34962},
            {"buffer": 0, "byteOffset": normals_offset, "byteLength": indices_offset - normals_offset, "target": 34962},
            {"buffer": 0, "byteOffset": indices_offset, "byteLength": data.len() - indices_offset, "target": 34963},
        ],
        "accessors": [
            {"bufferView": 0, "componentType": 5126, "count": mesh.vertices.len(), "type": "VEC3", "min": minimum, "max": maximum},
            {"bufferView": 1, "componentType": 5126, "count": mesh.vertices.len(), "type": "VEC3"},
            {"bufferView": 2, "componentType": 5125, "count": mesh.faces.len() * 3, "type": "SCALAR"},
        ],
    }))?;
    Ok(rgpre::gltf::to_glb(&document, &data)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rgpre::import::glide_splash::{SplashFace, SplashVertex};

    #[test]
    fn mesh_preview_preserves_source_positions_normals_and_indices() {
        let mut mesh = SplashMesh {
            vertices: [[2.0, 3.0, 4.0], [-2.0, 5.0, 6.0], [2.0, 0.0, 4.0]]
                .map(|position| SplashVertex {
                    position,
                    normal: [0.0, 0.0, 1.0],
                    uv: [0.0, 0.0],
                })
                .into(),
            faces: vec![SplashFace {
                vertices: [0, 1, 2],
                material: 0,
                antialiased_edges: 0,
            }],
        };
        let bytes = mesh_glb(&mesh).unwrap();
        let json_size = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
        let document: Value = serde_json::from_slice(&bytes[20..20 + json_size]).unwrap();
        assert_eq!(document["accessors"][0]["min"], json!([-2.0, 0.0, 4.0]));
        assert_eq!(document["accessors"][0]["max"], json!([2.0, 5.0, 6.0]));
        let binary = &bytes[28 + json_size..];
        let floats: Vec<_> = binary[..72]
            .as_chunks::<4>()
            .0
            .iter()
            .map(|word| f32::from_le_bytes(*word))
            .collect();
        assert_eq!(
            floats,
            [
                2.0, 3.0, 4.0, -2.0, 5.0, 6.0, 2.0, 0.0, 4.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0,
                0.0, 1.0
            ]
        );
        let indices: Vec<_> = binary[72..84]
            .as_chunks::<4>()
            .0
            .iter()
            .map(|word| u32::from_le_bytes(*word))
            .collect();
        assert_eq!(indices, [0, 1, 2]);
        mesh.faces[0].vertices[2] = 3;
        assert!(
            mesh_glb(&mesh)
                .unwrap_err()
                .to_string()
                .contains("vertex 3")
        );
    }
}
