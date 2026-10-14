//! ROB sampling attributes and effective palette keys from preservation's GLTF contract.

pub(super) fn attributes<'a>(extras: impl Iterator<Item = &'a str>) -> anyhow::Result<Option<u8>> {
    for value in extras {
        let value: serde_json::Value = serde_json::from_str(value)?;
        let Some(render) = value.get("rgpre_render") else {
            continue;
        };
        anyhow::ensure!(
            render["version"] == 1,
            "unsupported model render metadata version"
        );
        let value = render
            .get("rob_attributes")
            .ok_or_else(|| anyhow::anyhow!("missing ROB attributes"))?;
        if value.is_null() {
            return Ok(None);
        }
        return value
            .as_u64()
            .and_then(|value| u8::try_from(value).ok())
            .map(Some)
            .ok_or_else(|| anyhow::anyhow!("ROB attributes must be a byte or null"));
    }
    anyhow::bail!("missing model render metadata")
}

/// Effective palette-zero bytes from preservation's textured-material export contract.
pub(super) fn chroma_key(extras: Option<&str>, geometry: &str) -> anyhow::Result<Option<[u8; 3]>> {
    #[derive(serde::Deserialize)]
    struct Lighting {
        solid_rgb: Option<[u8; 3]>,
    }
    #[derive(serde::Deserialize)]
    struct Geometry {
        rgpre_lighting: Lighting,
    }
    if serde_json::from_str::<Geometry>(geometry)?
        .rgpre_lighting
        .solid_rgb
        .is_some()
    {
        return Ok(None);
    }
    let extras = extras.ok_or_else(|| anyhow::anyhow!("missing texture-key metadata"))?;

    #[derive(serde::Deserialize)]
    struct Texture {
        version: u32,
        chroma_key_rgb: [u8; 3],
    }
    #[derive(serde::Deserialize)]
    struct Extras {
        rgpre_texture: Texture,
    }
    let texture = serde_json::from_str::<Extras>(extras)?.rgpre_texture;
    anyhow::ensure!(
        texture.version == 1,
        "unsupported texture-key metadata version"
    );
    Ok(Some(texture.chroma_key_rgb))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texture_keys_preserve_all_palette_bytes_and_reject_missing_provenance() {
        assert_eq!(
            chroma_key(
                Some(r#"{"rgpre_texture":{"version":1,"chroma_key_rgb":[17,128,255]}}"#),
                r#"{"rgpre_lighting":{"solid_rgb":null}}"#
            )
            .unwrap(),
            Some([17, 128, 255])
        );
        for value in [
            r#"{}"#,
            r#"{"rgpre_texture":{"version":2,"chroma_key_rgb":[0,0,0]}}"#,
            r#"{"rgpre_texture":{"version":1,"chroma_key_rgb":[0,0]}}"#,
            r#"{"rgpre_texture":{"version":1,"chroma_key_rgb":[0,0,256]}}"#,
        ] {
            assert!(chroma_key(Some(value), r#"{"rgpre_lighting":{"solid_rgb":null}}"#).is_err());
        }
    }

    #[test]
    fn authored_solid_faces_do_not_require_texture_keys_for_palette_texture_exports() {
        assert_eq!(
            chroma_key(None, r#"{"rgpre_lighting":{"solid_rgb":[0,0,0]}}"#).unwrap(),
            None
        );
        assert!(chroma_key(None, r#"{"rgpre_lighting":{"solid_rgb":null}}"#).is_err());
        assert!(chroma_key(None, r#"{"rgpre_lighting":{"solid_rgb":[0]}}"#).is_err());
    }

    #[test]
    fn node_attributes_distinguish_absent_provenance_from_zero_flags() {
        for (json, expected) in [("0", Some(0)), ("255", Some(255)), ("null", None)] {
            let node = format!(r#"{{"rgpre_render":{{"version":1,"rob_attributes":{json}}}}}"#);
            assert_eq!(
                attributes([r#"{"rgpre_lighting":{}}"#, &node].into_iter()).unwrap(),
                expected
            );
        }
        for node in [
            r#"{}"#,
            r#"{"rgpre_render":{"version":2,"rob_attributes":0}}"#,
            r#"{"rgpre_render":{"version":1}}"#,
            r#"{"rgpre_render":{"version":1,"rob_attributes":256}}"#,
            r#"{"rgpre_render":{"version":1,"rob_attributes":-1}}"#,
        ] {
            assert!(attributes([node].into_iter()).is_err());
        }
    }
}
