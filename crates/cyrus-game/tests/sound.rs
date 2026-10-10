use bevy::audio::{AudioSource, Decodable, Source};
use cyrus_game::install::GameInstall;
use cyrus_game::music::{cue_path, read_track, world_track};
use cyrus_game::sound::{SoundSettings, load_speech, pcm_wav, read_settings, sfx_path};
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
    let clips = load_speech(&install.root.join(&settings.rtx_filename)).unwrap();
    assert_eq!(clips.len(), 3933, "RTX voice clip count per preservation");
    let (rate, channels, samples) = decode(clips.values().next().unwrap().clone());
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
    assert_eq!(world_track(&install, "1").unwrap(), Some(2));
    let (rate, channels, samples) = decode(read_track(&cue, 2).unwrap());
    assert_eq!((rate, channels), (44_100, 2));
    assert!(samples > 44_100 * 2 * 60, "track 2 lasts over a minute");
    assert!(read_track(&cue, 1).is_err(), "track 1 is the data track");
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
