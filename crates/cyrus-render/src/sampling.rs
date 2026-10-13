//! Selected ROB sampling attributes from preservation's positioned-model GLTF contract.

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

#[cfg(test)]
mod tests {
    use super::*;

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
