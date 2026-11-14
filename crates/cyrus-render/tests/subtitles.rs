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
    assert_eq!(font_alpha("125,255"), Ok([255, 199]), "Cyrus copy");
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

#[test]
fn installed_subtitle_replacement_updates_the_public_overlay() {
    use bevy::prelude::*;
    use cyrus_render::subtitles::{SubtitlePlugin, SubtitleRequest};

    let Some(install) = install("subtitle replacement") else {
        return;
    };
    let font = cyrus_text::Font::from(
        &rgpre::import::fnt::parse_fnt(
            &std::fs::read(install.root.join("fonts/REDSEL.FNT")).unwrap(),
        )
        .unwrap(),
    );
    let ini = cyrus_assets::install::system_ini(&install).unwrap();
    let alpha =
        font_alpha(cyrus_assets::sound::ini_value(&ini, "3dfx", "font_sel").unwrap_or("255,255"))
            .unwrap();
    let mut app = App::new();
    app.init_resource::<Time<Real>>()
        .init_resource::<Assets<Image>>()
        .insert_resource(install)
        .add_plugins(SubtitlePlugin);
    app.update();
    let request = |text: &str| SubtitleRequest::Speech {
        text: text.into(),
        seconds: 2.0,
    };
    app.world_mut().write_message(request("A"));
    app.update();
    app.world_mut().write_message(SubtitleRequest::StopAll);
    app.world_mut().write_message(request("AA"));
    app.update();
    let image = app
        .world_mut()
        .query::<&ImageNode>()
        .single(app.world())
        .unwrap()
        .image
        .clone();
    assert_eq!(
        app.world()
            .resource::<Assets<Image>>()
            .get(&image)
            .unwrap()
            .data
            .as_ref()
            .unwrap(),
        &draw(&font, "AA", alpha),
        "reusing the active slot must publish the replacement installed glyphs"
    );
}

#[test]
fn subtitle_admission_does_not_debit_the_previous_update() {
    use bevy::prelude::*;
    use cyrus_render::subtitles::{SubtitlePlugin, SubtitleRequest};
    use std::time::Duration;

    let Some(install) = install("subtitle admission clock") else {
        return;
    };
    let mut app = App::new();
    app.init_resource::<Time<Real>>()
        .init_resource::<Assets<Image>>()
        .insert_resource(install)
        .add_plugins(SubtitlePlugin);
    app.update();
    app.world_mut()
        .resource_mut::<Time<Real>>()
        .advance_by(Duration::from_secs(2));
    app.world_mut().write_message(SubtitleRequest::Speech {
        text: "A".into(),
        seconds: 2.0,
    });
    app.update();
    let overlays = |app: &mut App| {
        app.world_mut()
            .query::<&ImageNode>()
            .iter(app.world())
            .count()
    };
    assert_eq!(overlays(&mut app), 1, "new speech has not elapsed yet");
    app.world_mut()
        .resource_mut::<Time<Real>>()
        .advance_by(Duration::from_millis(1999));
    app.update();
    assert_eq!(
        overlays(&mut app),
        1,
        "the supplied duration starts at admission"
    );
    app.world_mut()
        .resource_mut::<Time<Real>>()
        .advance_by(Duration::from_millis(1));
    app.update();
    assert_eq!(overlays(&mut app), 0, "the complete duration has elapsed");
}

#[test]
fn expired_subtitles_release_capacity_before_new_speech_admission() {
    use bevy::prelude::*;
    use cyrus_render::subtitles::{SubtitlePlugin, SubtitleRequest};
    use std::time::Duration;

    let Some(install) = install("subtitle expired capacity") else {
        return;
    };
    let ini = cyrus_assets::install::system_ini(&install).unwrap();
    let slots = cyrus_assets::sound::ini_value(&ini, "dialog", "dialog_max_dialog_lines")
        .unwrap()
        .parse::<usize>()
        .unwrap()
        .max(16);
    let mut app = App::new();
    app.init_resource::<Time<Real>>()
        .init_resource::<Assets<Image>>()
        .insert_resource(install)
        .add_plugins(SubtitlePlugin);
    app.update();
    for _ in 0..slots {
        app.world_mut().write_message(SubtitleRequest::Speech {
            text: "A".into(),
            seconds: 1.0,
        });
    }
    app.update();
    app.world_mut()
        .resource_mut::<Time<Real>>()
        .advance_by(Duration::from_secs(1));
    app.world_mut().write_message(SubtitleRequest::Speech {
        text: "AA".into(),
        seconds: 1.0,
    });
    app.update();
    assert_eq!(
        app.world_mut()
            .query::<&ImageNode>()
            .iter(app.world())
            .count(),
        1,
        "expired lines cannot reject speech admitted at their deadline"
    );
    app.world_mut()
        .resource_mut::<Time<Real>>()
        .advance_by(Duration::from_secs(1));
    app.world_mut().write_message(SubtitleRequest::Speech {
        text: "A".into(),
        seconds: 0.0,
    });
    app.update();
    assert_eq!(
        app.world_mut()
            .query::<&ImageNode>()
            .iter(app.world())
            .count(),
        0,
        "zero-duration requests are still retired before publication"
    );
}
