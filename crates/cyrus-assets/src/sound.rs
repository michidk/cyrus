//! Portable sound settings and exported effect metadata.
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
/// `MAIN.SFX` metadata and the `SYSTEM.INI` values the mixer needs, exported for the browser.
#[derive(Asset, TypePath, Deserialize, Serialize)]
pub struct SoundBank {
    pub effects: Vec<SoundEffect>,
    /// Required SURFACE.INI rows; older banks must be re-exported.
    pub surface_sounds: SurfaceSounds,
    /// Installed CD settings and per-world track selection; old caches require re-export.
    #[serde(default)]
    pub music: Option<MusicSettings>,
    /// `[system] sound`; when off no effect plays.
    pub enabled: bool,
    /// `[system] volume`; 256 when absent.
    pub master_volume: i32,
    /// `[system] sound_distance`; `None` when absent.
    pub sound_distance: Option<i32>,
}

/// Validated SURFACE.INI animation remaps, indexed by surface class then source sound.
/// Twelve independent rows contain 256 unsigned 16-bit effect-bank IDs each.
#[derive(Resource, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Vec<Vec<u16>>", into = "Vec<Vec<u16>>")]
pub struct SurfaceSounds(Vec<Vec<u16>>);

impl TryFrom<Vec<Vec<u16>>> for SurfaceSounds {
    type Error = String;

    fn try_from(rows: Vec<Vec<u16>>) -> Result<Self, Self::Error> {
        if rows.len() != 12 {
            return Err(
                "sounds/bank.sounds surface_sounds: expected 12 rows; re-export sound assets"
                    .into(),
            );
        }
        for (surface, row) in rows.iter().enumerate() {
            if row.len() != 256 {
                return Err(format!(
                    "sounds/bank.sounds surface_sounds[{surface}]: expected 256 entries; re-export sound assets"
                ));
            }
        }
        Ok(Self(rows))
    }
}

impl From<SurfaceSounds> for Vec<Vec<u16>> {
    fn from(value: SurfaceSounds) -> Self {
        value.0
    }
}

impl SurfaceSounds {
    /// Decode installed text through preservation, retaining the raw unknown row.
    #[cfg(feature = "parsers")]
    pub fn parse(text: &str) -> Result<Self, String> {
        let ini = rgpre::import::surface_ini::SurfaceIni::parse(text)?;
        let rows = (0..12)
            .map(|surface| {
                ini.sound_table(surface)
                    .map(|row| row.to_vec())
                    .ok_or_else(|| format!("SURFACE.INI: missing parsed surface row {surface}"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        Self::try_from(rows)
    }

    /// Resolve a signed animation operand; surface zero bypasses its configured row.
    /// Invalid classes or sources return None. Callers still validate the resolved
    /// effect against the loaded bank. Script, combat and speech sounds bypass this API.
    pub fn animation_sound(&self, surface: u8, source: i16) -> Option<usize> {
        let source = usize::from(u8::try_from(source).ok()?);
        let row = self.0.get(usize::from(surface))?;
        Some(if surface == 0 {
            source
        } else {
            usize::from(row[source])
        })
    }
}

/// Installed Red Book configuration for browser playback, using WORLD.INI IDs.
#[derive(Resource, Clone, Deserialize, Serialize)]
pub struct MusicSettings {
    pub enabled: bool,
    /// Original redbook volume units; values outside 0..=127 are ignored by the drive.
    pub volume: i32,
    pub world_tracks: std::collections::BTreeMap<u32, i32>,
}

#[derive(Deserialize, Serialize)]
pub struct SoundEffect {
    pub looping: bool,
    pub sample_rate: u32,
    pub channels: u16,
    pub bits_per_sample: u16,
    /// SFX header byte `0x0C`, the volume of non-positional starts.
    pub base_volume: u8,
}

/// `SYSTEM.INI` audio switches (preservation `docs/config/system-ini.md`).
#[derive(Resource, Debug, Clone)]
pub struct SoundSettings {
    /// `[system] sound`, default on.
    pub effects: bool,
    /// `[system] redbook`, default on.
    pub music: bool,
    /// `[dialog] dialog_use_speech`, default on.
    pub speech: bool,
    /// `[dialog] dialog_print_text`: draw subtitles, default on.
    pub subtitles: bool,
    /// `[system] rtx_filename`, relative to the install root.
    pub rtx_filename: String,
    /// `[system] redbook_volume`; `-1` when absent or not an integer.
    pub redbook_volume: i32,
    /// `[system] volume`, the master volume for effects and speech; 256 when absent.
    pub master_volume: i32,
    /// `[system] sound_distance`, in units of 36 world units; `None` when absent.
    pub sound_distance: Option<i32>,
}

impl SoundSettings {
    /// Overrides the SYSTEM.INI values with the book's saved Sound page preferences:
    /// actions 601 (sound) and 602 (music) in slider units, 603 (voice) as 0/1.
    pub fn apply_menu(&mut self, preferences: &crate::preferences::MenuPreferences) {
        let value = |action| preferences.values.get(&action).copied();
        if let Some(sound) = value(601) {
            self.master_volume = slider_volume(sound);
        }
        if let Some(music) = value(602) {
            self.redbook_volume = slider_volume(music);
        }
        if let Some(voice) = value(603) {
            self.speech = voice != 0;
        }
        if let Some(subtitles) = value(504) {
            self.subtitles = subtitles != 0;
        }
    }

    pub fn parse(ini: &str) -> Self {
        let on = |section, key| {
            !ini_value(ini, section, key).is_some_and(|v| v == "0" || v.eq_ignore_ascii_case("off"))
        };
        Self {
            effects: on("system", "sound"),
            music: on("system", "redbook"),
            speech: on("dialog", "dialog_use_speech"),
            subtitles: on("dialog", "dialog_print_text"),
            rtx_filename: ini_value(ini, "system", "rtx_filename")
                .unwrap_or("ENGLISH.RTX")
                .to_string(),
            redbook_volume: ini_value(ini, "system", "redbook_volume")
                .and_then(|v| v.parse().ok())
                .unwrap_or(-1),
            master_volume: ini_value(ini, "system", "volume")
                .and_then(|v| v.parse().ok())
                .unwrap_or(256),
            sound_distance: ini_value(ini, "system", "sound_distance").and_then(|v| v.parse().ok()),
        }
    }
}

pub fn ini_value<'a>(ini: &'a str, section: &str, key: &str) -> Option<&'a str> {
    let mut current = "";
    for line in ini.lines().map(str::trim) {
        if let Some(name) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            current = name.trim();
        } else if current.eq_ignore_ascii_case(section)
            && let Some((k, v)) = line.split_once('=')
            && k.trim().eq_ignore_ascii_case(key)
        {
            return Some(v.trim());
        }
    }
    None
}

pub fn slider_volume(slider: i32) -> i32 {
    slider * 64 / 5
}

/// One voiced RTX entry: its audio, header base volume and subtitle text.
#[derive(Clone)]
pub struct VoiceClip<A> {
    pub audio: A,
    pub base_volume: u8,
    pub seconds: f64,
    pub text: String,
}
