//! Installed subtitle layout stays in presentation.
use cyrus_assets::install::GameInstall;
use cyrus_render::subtitles::{draw, font_alpha};
use std::path::Path;

fn install(check: &str) -> Option<GameInstall> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../redguard");
    let install = GameInstall::locate(Some(&root)).ok();
    if install.is_none() {
        eprintln!("skipped: no install ({check})");
    }
    install
}

#[test]
fn subtitles_wrap_and_place_like_the_original() {
    assert_eq!(font_alpha("255,255"), Ok([255, 199]), "NPC copy");
    assert_eq!(font_alpha("125,255"), Ok([255, 97]), "Cyrus copy");
    assert!(font_alpha("255").is_err());
    let Some(install) = install("subtitle") else {
        return;
    };
    let font = cyrus_text::Font::from(
        &rgpre::import::fnt::parse_fnt(
            &std::fs::read(install.root.join("fonts").join("REDSEL.FNT")).unwrap(),
        )
        .unwrap(),
    );
    let line = "YOU TWO DANCE VERY WELL, BUT, ALAS --";
    assert_eq!(font.wrap(line, 500), line, "fits one line at width 500");
    let wrapped = font.wrap(&format!("{line} {line}"), 500);
    assert!(wrapped.contains('\n'), "two copies exceed width 500");
    assert!(
        wrapped
            .split('\n')
            .all(|l| !l.is_empty() && !l.starts_with(' ')),
        "breaks drop the leading space: {wrapped:?}"
    );
    // The original drew this NPC line with glyph pixels spanning x 73..=565,
    // y 423..=439 of the 640×480 screen (DOSBox-X capture, ship save).
    let rgba = draw(&font, line, [255, 199]);
    let main: Vec<(usize, usize)> = (0..480)
        .flat_map(|y| (0..640).map(move |x| (x, y)))
        .filter(|&(x, y)| {
            let p = &rgba[(y * 640 + x) * 4..][..4];
            p[3] == 255 && p[..3] != [0, 0, 0]
        })
        .collect();
    let xs = main.iter().map(|p| p.0);
    let ys = main.iter().map(|p| p.1);
    assert_eq!(
        (xs.clone().min(), xs.max(), ys.clone().min(), ys.max()),
        (Some(73), Some(565), Some(423), Some(439))
    );
}
