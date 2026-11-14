//! Installed HUD glyph overflow through the public font raster interface.
use cyrus_assets::install::GameInstall;
use cyrus_text::Font;
use std::path::Path;

#[test]
fn installed_hud_overflow_retains_the_complete_glyph_on_the_next_line() {
    let root = std::env::var_os("REDGUARD_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../redguard"));
    let Ok(install) = GameInstall::locate(Some(&root)) else {
        eprintln!("skipped: no install (HUD glyph overflow)");
        return;
    };
    let font = Font::from(
        &rgpre::import::fnt::parse_fnt(
            &std::fs::read(install.root.join("fonts/REDSEL.FNT")).unwrap(),
        )
        .unwrap(),
    );
    let glyph = font.glyph(b'5').unwrap();
    let start = 640 - i32::from(glyph.width);
    let mut expected = vec![0; 640 * 480 * 4];
    font.draw_shadowed(&mut expected, 640, "5", [start, 394], [255, 199]);
    font.draw_shadowed(
        &mut expected,
        640,
        "3",
        [start, 394 + i32::from(font.line_height)],
        [255, 199],
    );
    let mut actual = vec![0; expected.len()];
    font.draw_shadowed_wrapped(&mut actual, 640, "53", [start, 394], [255, 199]);
    let corrected = actual
        .as_chunks::<4>()
        .0
        .iter()
        .zip(expected.as_chunks::<4>().0)
        .filter(|(a, b)| a != b)
        .count();
    assert_eq!(corrected, 0, "full installed glyph/shadow raster");
    let mut clipped = vec![0; expected.len()];
    font.draw_shadowed(&mut clipped, 640, "53", [start, 394], [255, 199]);
    let missing = actual
        .as_chunks::<4>()
        .0
        .iter()
        .zip(clipped.as_chunks::<4>().0)
        .filter(|(a, b)| a != b)
        .count();
    assert!(missing > 0, "the old clipping path must fail comparison");
    if let Some(path) = std::env::var_os("CYRUS_HUD_RASTER_OUTPUT") {
        let path = std::path::PathBuf::from(path);
        std::fs::write(&path, &actual).expect("write requested HUD raster");
        std::fs::write(path.with_extension("old.rgba"), &clipped)
            .expect("write requested clipping control");
        std::fs::write(
            path.with_extension("font.json"),
            serde_json::to_vec(&font).unwrap(),
        )
        .expect("write requested installed font metadata");
    }
    eprintln!(
        "HUD overflow: installed glyph width {}, line height {}, corrected mismatch pixels 0, old clipping mismatch pixels {missing}",
        glyph.width, font.line_height
    );
}
