use bevy::audio::{AudioSource, Decodable, Source};
use cyrus_game::install::GameInstall;
use cyrus_game::music::{cue_path, read_track, redbook_gain, world_track};
use cyrus_game::sound::{
    SoundSettings, distance_volume, heading, load_speech, output_gain, pan, pan_gains, pcm_wav,
    read_settings, sfx_path, slider_volume,
};
use cyrus_game::subtitles::{draw, font_alpha, wrap};
use rgpre::import::sfx::parse_sfx_file;
use std::path::Path;

/// Decodes `source` with Bevy's own decoder: (sample rate, channels, sample count).
fn decode(source: AudioSource) -> (u32, u16, usize) {
    let decoder = source.decoder();
    let (rate, channels) = (decoder.sample_rate().get(), decoder.channels().get());
    (rate, channels, decoder.count())
}

fn install(check: &str) -> Option<GameInstall> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../redguard");
    let install = GameInstall::locate(Some(&root)).ok();
    if install.is_none() {
        eprintln!("skipping installed {check} check: no Redguard install");
    }
    install
}

#[test]
fn installed_effects_decode_at_their_stored_format() {
    let Some(install) = install("sound effect") else {
        return;
    };
    let file = parse_sfx_file(&std::fs::read(sfx_path(&install)).unwrap()).unwrap();
    assert_eq!(
        file.effects.len(),
        118,
        "MAIN.SFX effect count per preservation"
    );
    for (index, effect) in file.effects.iter().enumerate() {
        let wav = pcm_wav(effect.audio_type, effect.sample_rate, &effect.pcm_data)
            .unwrap_or_else(|e| panic!("effect {index}: {e}"));
        let bytes_per_sample = usize::from(effect.audio_type.bits_per_sample() / 8);
        assert_eq!(
            decode(AudioSource { bytes: wav.into() }),
            (
                effect.sample_rate,
                effect.audio_type.channels(),
                effect.pcm_data.len() / bytes_per_sample
            ),
            "effect {index}"
        );
    }
    let looping = file.effects.iter().filter(|e| e.loop_flag != 0).count();
    assert!(looping > 0 && looping < file.effects.len());
}

#[test]
fn installed_voice_clips_all_load() {
    let Some(install) = install("speech") else {
        return;
    };
    let settings = read_settings(&install);
    assert!(settings.effects && settings.music && settings.speech);
    assert_eq!(settings.redbook_volume, 200);
    assert_eq!(
        (settings.master_volume, settings.sound_distance),
        (255, Some(64))
    );
    let clips = load_speech(&install.root.join(&settings.rtx_filename)).unwrap();
    assert_eq!(clips.len(), 3933, "RTX voice clip count per preservation");
    let (rate, channels, samples) = decode(clips.values().next().unwrap().audio.clone());
    assert!(rate > 0 && channels > 0 && samples > 0);
}

#[test]
fn installed_world_music_reads_red_book_tracks() {
    let Some(install) = install("music") else {
        return;
    };
    let Some(cue) = cue_path(&install) else {
        eprintln!("skipping installed music check: no game.ins beside the install");
        return;
    };
    assert_eq!(world_track(&install, "1").unwrap(), 2);
    let (rate, channels, samples) = decode(read_track(&cue, 2).unwrap());
    assert_eq!((rate, channels), (44_100, 2));
    assert!(samples > 44_100 * 2 * 60, "track 2 lasts over a minute");
    assert!(read_track(&cue, 1).is_err(), "track 1 is the data track");
}

#[test]
fn distance_volume_matches_the_original_curve() {
    // Shipped sound_distance 64: radius 2304, divisor 3640 (preservation docs/engine/sound.md).
    for (distance, volume) in [
        (0.0, 127),
        (18.0, 127),
        (19.0, 126),
        (500.0, 100),
        (1152.0, 64),
        (2286.0, 1),
        (2287.0, 0),
        (2305.0, 0),
    ] {
        assert_eq!(
            distance_volume(64, distance, 0),
            Ok(volume),
            "distance {distance}"
        );
    }
    assert_eq!(distance_volume(64, 0.0, -40), Ok(87));
    assert_eq!(distance_volume(64, 1000.0, 100), Ok(127));
    assert_eq!(distance_volume(64, 2400.0, 100), Ok(0), "beyond the radius");
    assert!(
        distance_volume(3, 0.0, 0).is_err(),
        "the original divides by zero"
    );
    // Miles mixer: (v + 1) / 128 after v = volume × master >> 8.
    assert_eq!(output_gain(127, 256), 1.0);
    assert_eq!(output_gain(127, 128), 0.5, "v 63");
    assert_eq!(output_gain(64, 255), 0.5, "v 63");
    assert_eq!(output_gain(1, 255), 0.0, "v 0 is silent");
    // Sound page slider to engine volume (preservation docs/engine/menu.md).
    let volumes: Vec<i32> = (0..=20).map(slider_volume).collect();
    assert_eq!(
        volumes,
        [
            0, 12, 25, 38, 51, 64, 76, 89, 102, 115, 128, 140, 153, 166, 179, 192, 204, 217, 230,
            243, 256
        ]
    );
    assert_eq!(redbook_gain(slider_volume(9)), Some(230.0 / 255.0));
    assert_eq!(
        redbook_gain(slider_volume(10)),
        None,
        "128 is ignored by the drive"
    );
}

#[test]
fn subtitles_wrap_and_place_like_the_original() {
    assert_eq!(font_alpha("255,255"), Ok([255, 199]), "NPC copy");
    assert_eq!(font_alpha("125,255"), Ok([255, 97]), "Cyrus copy");
    assert!(font_alpha("255").is_err());
    let Some(install) = install("subtitle") else {
        return;
    };
    let font = rgpre::import::fnt::parse_fnt(
        &std::fs::read(install.root.join("fonts").join("REDSEL.FNT")).unwrap(),
    )
    .unwrap();
    let line = "YOU TWO DANCE VERY WELL, BUT, ALAS --";
    assert_eq!(wrap(&font, line, 500), line, "fits one line at width 500");
    let wrapped = wrap(&font, &format!("{line} {line}"), 500);
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
fn pan_follows_the_original_heading_and_miles_pan_law() {
    // Emitter offsets (dx, dz) with the listener facing +Z (preservation docs/engine/sound.md).
    for ((dx, dz), expected) in [
        ((0, 100), 64),
        ((100, 100), 96),
        ((100, 0), 64),
        ((100, -1), 127),
        ((0, -100), 64),
        ((-100, -100), 32),
        ((-100, 0), 64),
        ((-100, 100), 32),
    ] {
        assert_eq!(pan(heading(dx, dz), 0), expected, "({dx}, {dz})");
    }
    assert_eq!(heading(100, -1), 515);
    assert_eq!(pan(heading(100, 100), 512), 32, "listener turned a quarter");
    // Miles pan table: centre keeps both channels full; extremes silence one.
    assert_eq!(pan_gains(64), [1.0, 1.0]);
    assert_eq!(pan_gains(127), [0.0, 1.0]);
    assert_eq!(pan_gains(0), [1.0, 0.0]);
    assert_eq!(pan_gains(96), [62.0 / 128.0, 1.0]);
    assert_eq!(pan_gains(32), [1.0, 64.0 / 128.0]);
}

#[test]
fn system_ini_switches_default_on() {
    let defaults = SoundSettings::parse("");
    assert!(defaults.effects && defaults.music && defaults.speech);
    assert_eq!(defaults.rtx_filename, "ENGLISH.RTX");
    let off = SoundSettings::parse(
        "[system]\nsound=0\nredbook=off\nrtx_filename=FRENCH.RTX\n[dialog]\ndialog_use_speech=0\n[other]\nsound=1\n",
    );
    assert!(!off.effects && !off.music && !off.speech);
    assert_eq!(off.rtx_filename, "FRENCH.RTX");
}
