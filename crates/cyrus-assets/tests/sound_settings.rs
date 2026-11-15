//! Public sound startup settings follow the released SYSTEM.INI audio contract.
use cyrus_assets::{preferences::MenuPreferences, sound::SoundSettings};

#[test]
fn disabled_sound_forces_silent_text_even_after_saved_preferences() {
    for disabled in ["0", "off", "OFF"] {
        let ini = format!(
            "[system]\nsound={disabled}\nredbook=on\n[dialog]\ndialog_use_speech=1\ndialog_print_text=0\n"
        );
        let mut settings = SoundSettings::parse(&ini);
        assert!(!settings.effects);
        assert!(!settings.music, "sound={disabled} admitted CD startup");
        assert!(!settings.speech, "sound={disabled} admitted speech");
        assert!(settings.subtitles, "sound={disabled} hid forced text");
        let mut preferences = MenuPreferences {
            values: Default::default(),
            bindings: [0; 15],
            overrides: None,
        };
        preferences.values.insert(603, 1);
        preferences.values.insert(504, 0);
        settings.apply_menu(&preferences);
        assert!(!settings.music);
        assert!(!settings.speech);
        assert!(settings.subtitles);
    }
}

#[test]
fn enabled_sound_retains_independent_music_voice_and_text_preferences() {
    let mut settings = SoundSettings::parse("[system]\nsound=1\nredbook=off\n");
    assert!(settings.effects);
    assert!(!settings.music);
    let mut preferences = MenuPreferences {
        values: Default::default(),
        bindings: [0; 15],
        overrides: None,
    };
    preferences.values.insert(603, 1);
    preferences.values.insert(504, 0);
    settings.apply_menu(&preferences);
    assert!(settings.speech);
    assert!(!settings.subtitles);
    let default = SoundSettings::parse("");
    assert!(default.effects && default.music && default.speech && default.subtitles);
}

#[test]
fn saved_unrelated_settings_preserve_installed_audio_levels() {
    // The installed 255/200 display as 19/15. Only an updated volume control
    // converts its displayed value back to engine units (released menu.md).
    for (overrides, expected) in [
        (vec![504], (255, 200)),
        (vec![], (255, 200)),
        (vec![602], (255, 192)),
        (vec![601], (243, 200)),
    ] {
        let saved = serde_json::json!({
            "values": {"504": 0, "505": 0, "601": 19, "602": 15, "603": 1},
            "bindings": vec![0; 15],
            "overrides": overrides,
        });
        let preferences: MenuPreferences = serde_json::from_value(saved).unwrap();
        preferences.validate().unwrap();
        // Saved preferences must retain their ownership when reopened.
        let preferences: MenuPreferences =
            serde_json::from_str(&serde_json::to_string(&preferences).unwrap()).unwrap();
        let mut settings = SoundSettings::parse("[system]\nvolume=255\nredbook_volume=200");
        settings.apply_menu(&preferences);
        assert_eq!((settings.master_volume, settings.redbook_volume), expected);
    }
}

#[test]
fn saved_override_ownership_rejects_unknown_settings_and_keeps_legacy_documents() {
    let mut saved = serde_json::json!({
        "values": {"504": 1, "505": 0, "601": 19, "602": 15, "603": 1},
        "bindings": vec![0; 15],
    });
    let legacy: MenuPreferences = serde_json::from_value(saved.clone()).unwrap();
    legacy.validate().unwrap();
    let mut audio = SoundSettings::parse("[system]\nvolume=255\nredbook_volume=200");
    audio.apply_menu(&legacy);
    assert_eq!((audio.master_volume, audio.redbook_volume), (243, 192));
    saved["overrides"] = serde_json::json!([999]);
    let invalid: MenuPreferences = serde_json::from_value(saved).unwrap();
    assert!(
        invalid
            .validate()
            .unwrap_err()
            .to_string()
            .contains("override")
    );
}
