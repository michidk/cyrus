//! Native readback is world-bound and feeds only the saved-image record group.
use cyrus::original_slot::capture::{CapturedFrame, capture_images};

#[test]
fn packed_capture_keeps_rgb565_fields_rows_and_independent_thumbnail_sampling() {
    let mut rgba = vec![0; 640 * 480 * 4];
    for p in rgba.as_chunks_mut::<4>().0 {
        p[3] = 255;
    }
    for (x, colour) in [
        (0, [255, 0, 0, 255]),
        (2, [0, 255, 0, 255]),
        (5, [0, 0, 255, 255]),
    ] {
        rgba[x * 4..x * 4 + 4].copy_from_slice(&colour);
    }
    rgba[(2 * 640) * 4..(2 * 640) * 4 + 4].copy_from_slice(&[255, 255, 255, 255]);
    let images = capture_images(
        1,
        Some(CapturedFrame {
            world: 1,
            width: 640,
            height: 480,
            rgba: &rgba,
        }),
    )
    .unwrap();
    let (screen, thumbnail) = images.records();
    let word = |bytes: &[u8], i: usize| u16::from_le_bytes([bytes[i * 2], bytes[i * 2 + 1]]);
    assert_eq!(word(&screen.data, 0), 0xf800);
    assert_eq!(word(&screen.data, 2), 0x07e0);
    assert_eq!(word(&thumbnail.data, 0), 0xf800);
    assert_eq!(word(&thumbnail.data, 1), 0x07e0);
    assert_eq!(word(&thumbnail.data, 2), 0x001f);
    assert_eq!(word(&thumbnail.data, 256), 0xffff);
}

#[test]
fn unavailable_malformed_transparent_and_wrong_world_capture_reject_with_context() {
    assert!(format!("{:#}", capture_images(20, None).unwrap_err()).contains("world 20"));
    for (world, width, height, rgba, expected) in [
        (1, 640, 480, vec![255; 640 * 480 * 4], "world 1"),
        (20, 0, 0, vec![], "dimensions"),
        (20, 640, 480, vec![], "bytes"),
        (20, 640, 480, vec![0; 640 * 480 * 4], "opaque"),
        (20, 1280, 720, vec![255; 1280 * 720 * 4], "4:3"),
    ] {
        let error = format!(
            "{:#}",
            capture_images(
                20,
                Some(CapturedFrame {
                    world,
                    width,
                    height,
                    rgba: &rgba
                })
            )
            .unwrap_err()
        );
        assert!(
            error.contains("world 20") && error.contains(expected),
            "{error}"
        );
    }
}

#[test]
fn installed_worlds_one_and_twenty_readback_admits_only_matching_saved_images() {
    use rgpre::import::savegame::{SaveImageKind, load_save_slot};
    let Some(install) = std::env::var_os("REDGUARD_ORIGINAL_SAVE_INSTALL") else {
        eprintln!("SKIP native saved-image capture: REDGUARD_ORIGINAL_SAVE_INSTALL required");
        return;
    };
    assert!(
        std::env::var_os("REDGUARD_DIR").is_some(),
        "linked install required"
    );
    let mut worlds = std::collections::BTreeSet::new();
    for entry in std::fs::read_dir(std::path::Path::new(&install).join("SAVEGAME")).unwrap() {
        let entry = entry.unwrap();
        if !entry.file_type().unwrap().is_dir() {
            continue;
        }
        let slot = load_save_slot(entry.path()).unwrap();
        let world = slot.game.metadata.world_id;
        if ![1, 20].contains(&world) {
            continue;
        }
        let screen = slot.game.image(SaveImageKind::Screen).unwrap().unwrap();
        let rgba: Vec<_> = screen
            .pixels()
            .iter()
            .flat_map(|word| {
                let r = ((word >> 11) & 31) as u8;
                let g = ((word >> 5) & 63) as u8;
                let b = (word & 31) as u8;
                [
                    (r << 3) | (r >> 2),
                    (g << 2) | (g >> 4),
                    (b << 3) | (b >> 2),
                    255,
                ]
            })
            .collect();
        let captured = capture_images(
            world,
            Some(CapturedFrame {
                world,
                width: 640,
                height: 480,
                rgba: &rgba,
            }),
        )
        .unwrap();
        assert_eq!(
            captured.records().0.data,
            slot.game
                .sections
                .iter()
                .find(|s| s.name == *b"SCR3")
                .unwrap()
                .data
        );
        captured.admit(&slot.game).unwrap();
        let mut other_world = slot.game.clone();
        other_world.metadata.world_id = if world == 1 { 20 } else { 1 };
        let error = format!("{:#}", captured.admit(&other_world).unwrap_err());
        assert!(
            error.contains("capture world") && error.contains("SCR3/THMB"),
            "{error}"
        );
        worlds.insert(world);
    }
    assert_eq!(
        worlds,
        std::collections::BTreeSet::from([1, 20]),
        "both authentic world fixtures required"
    );
    println!(
        "native packed-image capture worlds {worlds:?}; retained screen words roundtrip; matching-world typed admission only"
    );
}

#[test]
fn thumbnail_samples_original_frame_instead_of_reduced_screen() {
    let mut rgba = vec![0; 1280 * 960 * 4];
    for p in rgba.as_chunks_mut::<4>().0 {
        p[3] = 255;
    }
    rgba[5 * 4..5 * 4 + 4].copy_from_slice(&[255, 0, 0, 255]);
    let captured = capture_images(
        20,
        Some(CapturedFrame {
            world: 20,
            width: 1280,
            height: 960,
            rgba: &rgba,
        }),
    )
    .unwrap();
    let (screen, thumbnail) = captured.records();
    assert_eq!(&screen.data[4..6], &[0, 0]);
    assert_eq!(&thumbnail.data[2..4], &0xf800u16.to_le_bytes());
}

#[test]
fn unavailable_capture_rejects_export_before_destination_or_native_save_changes() {
    use cyrus::original_slot::request::ExportRequest;
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("SAVEGAME.002")).unwrap();
    std::fs::write(root.path().join("SAVEGAME.002/KEEP"), b"existing original").unwrap();
    std::fs::write(root.path().join("native-save"), b"native fallback").unwrap();
    let error = ExportRequest::prepare(root.path(), 2, || {
        capture_images(20, None)?;
        anyhow::bail!("complete inverse mappings still required")
    })
    .expect_err("missing capture must reject");
    let error = format!("{error:#}");
    assert!(
        error.contains("world 20") && error.contains("SCR3/THMB"),
        "{error}"
    );
    assert_eq!(
        std::fs::read(root.path().join("SAVEGAME.002/KEEP")).unwrap(),
        b"existing original"
    );
    assert_eq!(
        std::fs::read(root.path().join("native-save")).unwrap(),
        b"native fallback"
    );
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 2);
}
