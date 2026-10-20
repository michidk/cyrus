//! Retained browser slot selection, separate from captured gameplay state.
use anyhow::{Context, Result};
use bevy::prelude::World;
use serde_json::Value;

pub(super) fn retain_selection(world: &mut World, value: &Value) -> Result<u16> {
    let slot = value["slot"]["slot"]
        .as_u64()
        .and_then(|slot| u16::try_from(slot).ok())
        .filter(|slot| *slot <= 299)
        .context("browser save selection: invalid slot")?;
    let description = value["slot"]["description"]
        .as_str()
        .context("browser save selection: description unavailable")?
        .to_owned();
    anyhow::ensure!(
        !description.is_empty()
            && description.len() <= 64
            && description
                .bytes()
                .all(|byte| matches!(byte, 11 | 13 | 32..=126)),
        "browser save selection: invalid description"
    );
    world
        .get_resource_mut::<cyrus_game::script::DeathDispatch>()
        .context("browser save selection: last-save dispatcher unavailable")?
        .last_save = Some(cyrus_game::script::LastSaveSelection { slot, description });
    Ok(slot)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cyrus_game::script::DeathDispatch;
    use serde_json::json;

    #[test]
    fn admitted_selection_survives_failed_document_decode() -> Result<()> {
        let mut world = World::new();
        world.init_resource::<DeathDispatch>();
        let response =
            json!({"slot":{"slot":17,"description":"Mixed\u{b}case\rtext"},"document":"invalid"});
        assert_eq!(retain_selection(&mut world, &response)?, 17);
        assert!(cyrus_session::RuntimeSessionSnapshot::decode_document(b"invalid").is_err());
        let selected = world
            .resource::<DeathDispatch>()
            .last_save
            .as_ref()
            .unwrap();
        assert_eq!(selected.slot, 17);
        assert_eq!(selected.description, "Mixed\u{b}case\rtext");
        Ok(())
    }

    #[test]
    fn invalid_selection_preserves_the_accepted_dispatcher() -> Result<()> {
        let mut world = World::new();
        world.init_resource::<DeathDispatch>();
        retain_selection(
            &mut world,
            &json!({"slot":{"slot":2,"description":"ACCEPTED"}}),
        )?;
        for value in [
            json!({"slot":{"slot":300,"description":"BAD"}}),
            json!({"slot":{"slot":3,"description":"bad\ntext"}}),
            json!({"slot":{"slot":3,"description":""}}),
            json!({"slot":{"slot":3}}),
        ] {
            assert!(retain_selection(&mut world, &value).is_err());
            let selected = world
                .resource::<DeathDispatch>()
                .last_save
                .as_ref()
                .unwrap();
            assert_eq!(selected.slot, 2);
            assert_eq!(selected.description, "ACCEPTED");
        }
        Ok(())
    }
}
