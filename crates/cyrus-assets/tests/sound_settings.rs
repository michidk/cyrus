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
    };
    preferences.values.insert(603, 1);
    preferences.values.insert(504, 0);
    settings.apply_menu(&preferences);
    assert!(settings.speech);
    assert!(!settings.subtitles);
    let default = SoundSettings::parse("");
    assert!(default.effects && default.music && default.speech && default.subtitles);
}
