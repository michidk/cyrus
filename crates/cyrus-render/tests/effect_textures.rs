use bevy::prelude::*;
use cyrus_assets::effects::{EffectImage, Effects};
use cyrus_render::{fog::TextureAnimations, persistence::StagedRenderRoot};
use std::collections::BTreeMap;

fn source() -> Effects {
    Effects {
        ambient_brightness: Some(78),
        capacity: 2,
        disabled: false,
        images: BTreeMap::from([(
            0x2500,
            EffectImage {
                name: "A00000".into(),
                size: [1, 2],
                texture_scale: 256,
                delay_ms: 100,
                flags: 1,
                alpha: 151,
                frames: vec![
                    vec![10, 20, 30, 255, 40, 50, 60, 0],
                    vec![70, 80, 90, 255, 100, 110, 120, 0],
                ],
            },
        )]),
    }
}
fn prepared(world: &mut World) -> (TextureAnimations, Entity) {
    world.init_resource::<Assets<Mesh>>();
    world.init_resource::<Assets<Image>>();
    let root = world.spawn((StagedRenderRoot, Visibility::Hidden)).id();
    (
        TextureAnimations::prepare(
            world,
            Handle::<cyrus_assets::environment::Environment>::default().id(),
            &[root],
        )
        .unwrap(),
        root,
    )
}

#[test]
fn effect_images_are_unrequested_fingerprinted_and_restored_without_allocating_on_rebind()
-> anyhow::Result<()> {
    let mut world = World::new();
    let (mut live, root) = prepared(&mut world);
    let source = source();
    live.prepare_effects(&source, &mut world.resource_mut::<Assets<Image>>())?;
    assert_eq!(live.snapshot().len(), 1);
    assert_eq!(live.snapshot()[0].frame, None);
    assert_eq!(live.snapshot()[0].count, 2);
    let assets = world.resource::<Assets<Image>>();
    assert_eq!(assets.len(), 2);
    let pixels: Vec<_> = assets
        .iter()
        .map(|(_, image)| image.data.clone().unwrap())
        .collect();
    assert!(pixels.contains(&vec![40, 50, 60, 0, 10, 20, 30, 255]));
    assert!(pixels.contains(&vec![100, 110, 120, 0, 70, 80, 90, 255]));
    let sources = live.snapshot_sources(&world, &[root])?;
    live.hold("a00000", Some(1))?;
    let saved = live.snapshot_playback()?;
    live.prepare_effects(&source, &mut world.resource_mut::<Assets<Image>>())?;
    assert_eq!(world.resource::<Assets<Image>>().len(), 2);
    assert_eq!(live.snapshot()[0].frame, Some(1));
    let (mut replacement, replacement_root) = prepared(&mut world);
    replacement.prepare_effects(&source, &mut world.resource_mut::<Assets<Image>>())?;
    sources.validate(&replacement, &world, &[replacement_root])?;
    let replacement = TextureAnimations::from_snapshot(replacement, saved)?;
    assert_eq!(replacement.snapshot(), live.snapshot());
    let mut changed = source.clone();
    changed.images.get_mut(&0x2500).unwrap().frames[0][0] ^= 1;
    let before = serde_json::to_value(live.snapshot_playback()?)?;
    assert!(
        live.prepare_effects(&changed, &mut world.resource_mut::<Assets<Image>>())
            .is_err()
    );
    assert_eq!(serde_json::to_value(live.snapshot_playback()?)?, before);
    assert_eq!(world.resource::<Assets<Image>>().len(), 4);
    for (_, image) in world.resource_mut::<Assets<Image>>().iter_mut() {
        image.data.as_mut().unwrap()[0] ^= 1;
    }
    assert!(
        sources
            .validate(&replacement, &world, &[replacement_root])
            .is_err()
    );
    Ok(())
}

#[test]
fn invalid_effect_batch_and_shared_metadata_leave_existing_playback_untouched() -> anyhow::Result<()>
{
    let mut world = World::new();
    let (mut live, _) = prepared(&mut world);
    for mutate in [0, 1, 2] {
        let mut invalid = source();
        let mut second = invalid.images[&0x2500].clone();
        match mutate {
            0 => second.name = "invalid-key".into(),
            1 => second.delay_ms = -1,
            _ => second.frames[0][0] ^= 1,
        }
        invalid.images.insert(0x2501, second);
        assert!(
            live.prepare_effects(&invalid, &mut world.resource_mut::<Assets<Image>>())
                .is_err()
        );
        assert!(live.snapshot().is_empty());
        assert!(world.resource::<Assets<Image>>().is_empty());
    }
    live.prepare_effects(&source(), &mut world.resource_mut::<Assets<Image>>())?;
    assert_eq!(live.snapshot()[0].frame, None);
    assert_eq!(world.resource::<Assets<Image>>().len(), 2);
    Ok(())
}

#[test]
#[cfg(not(target_arch = "wasm32"))]
fn installed_effect_images_join_the_exact_render_source_census() -> anyhow::Result<()> {
    use cyrus_assets::{environment::Environment, install::GameInstall, native::Files};
    let root = std::env::var_os("REDGUARD_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../redguard")
        });
    let Ok(install) = GameInstall::open(&root) else {
        eprintln!("skipped: no install (effect texture owner)");
        return Ok(());
    };
    let files = cyrus_assets::game::prepare(&Files::open(&install)?, "START.environment")?;
    let environment: Environment = serde_json::from_slice(&files["START.environment"])?;
    let source = environment
        .effects
        .as_ref()
        .expect("installed effect sources");
    let mut world = World::new();
    let (mut live, root) = prepared(&mut world);
    live.prepare_effects(source, &mut world.resource_mut::<Assets<Image>>())?;
    assert_eq!(world.resource::<Assets<Image>>().len(), 38);
    let frames = live.snapshot();
    assert_eq!(frames.len(), 4);
    assert!(
        frames
            .iter()
            .all(|image| image.frame.is_none() && !image.held)
    );
    let sources = live.snapshot_sources(&world, &[root])?;
    let (mut replacement, replacement_root) = prepared(&mut world);
    replacement.prepare_effects(source, &mut world.resource_mut::<Assets<Image>>())?;
    sources.validate(&replacement, &world, &[replacement_root])?;
    let restored = TextureAnimations::from_snapshot(replacement, live.snapshot_playback()?)?;
    assert_eq!(restored.snapshot(), frames);
    Ok(())
}
