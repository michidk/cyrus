//! INI patches: a mod's declared value changes in the game's INI files (`COMBAT.INI`, `MENU.INI`).
//! A patch is a few words of data (`mod.toml`'s `[[ini-patch]]`), never a copy of game data: it
//! sets a value or corrects a fragment of one. It is applied to the text read from the install when
//! the files are scanned, so the install is not written and removing the mod restores the
//! original values.
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

/// Changes one key in the same-named sections of one INI file: either sets it to `value`, or
/// replaces the fragment `find` of its current value with `replace`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IniPatch {
    /// The file's path in the install (`COMBAT.INI`); case does not matter.
    pub file: String,
    /// The sections to change, by name without brackets (`attack65`); case does not matter.
    pub sections: Vec<String>,
    /// The key in each section; case does not matter. It must already be there.
    pub key: String,
    /// The new value, as it is written after the `=`.
    #[serde(default)]
    pub value: Option<String>,
    /// A fragment of the current value that occurs once; `replace` takes its place.
    #[serde(default)]
    pub find: Option<String>,
    #[serde(default)]
    pub replace: Option<String>,
}

impl IniPatch {
    /// Whether the patch is for the file at `path` in the install.
    pub fn applies_to(&self, path: &str) -> bool {
        self.file.eq_ignore_ascii_case(path)
    }

    /// Applies the patch to the text of its file. Every named section must exist and hold the key,
    /// so a patch written for another version of the file fails instead of silently doing nothing.
    pub fn apply(&self, text: &str) -> Result<String> {
        let mut lines: Vec<String> = text.split('\n').map(str::to_owned).collect();
        for section in &self.sections {
            let header = lines
                .iter()
                .position(|line| {
                    line.trim()
                        .strip_prefix('[')
                        .and_then(|rest| rest.strip_suffix(']'))
                        .is_some_and(|name| name.trim().eq_ignore_ascii_case(section))
                })
                .with_context(|| format!("{}: no section [{section}]", self.file))?;
            let offset = lines[header + 1..]
                .iter()
                .take_while(|line| !line.trim_start().starts_with('['))
                .position(|line| {
                    line.split_once('=')
                        .is_some_and(|(name, _)| name.trim().eq_ignore_ascii_case(&self.key))
                })
                .with_context(|| format!("{}: [{section}] has no key {}", self.file, self.key))?;
            let at = header + 1 + offset;
            let line = lines[at].trim_end_matches('\r');
            let carriage = lines[at].len() != line.len();
            let (name, rest) = line.split_once('=').expect("the key line has an =");
            let current = rest.trim_start();
            let spacing = &rest[..rest.len() - current.len()];
            let value = self.new_value(current.trim_end(), section)?;
            lines[at] = format!(
                "{name}={spacing}{value}{}",
                if carriage { "\r" } else { "" }
            );
        }
        Ok(lines.join("\n"))
    }
}

impl IniPatch {
    /// The value after the patch: `value`, or `current` with `find` replaced.
    fn new_value(&self, current: &str, section: &str) -> Result<String> {
        match (&self.value, &self.find, &self.replace) {
            (Some(value), None, None) => Ok(value.clone()),
            (None, Some(find), Some(replace)) => {
                ensure!(
                    current.matches(find.as_str()).count() == 1,
                    "{}: [{section}] {} does not contain {find:?} exactly once",
                    self.file,
                    self.key
                );
                Ok(current.replacen(find.as_str(), replace, 1))
            }
            _ => bail!(
                "{}: [{section}] {}: a patch has a value, or a find and a replace",
                self.file,
                self.key
            ),
        }
    }
}

/// Applies `patches` that are for `path` to `text`, in order.
pub fn apply_all(patches: &[IniPatch], path: &str, text: &str) -> Result<Option<String>> {
    let mut changed: Option<String> = None;
    for patch in patches.iter().filter(|patch| patch.applies_to(path)) {
        changed = Some(patch.apply(changed.as_deref().unwrap_or(text))?);
    }
    Ok(changed)
}

static PATCHES: OnceLock<Vec<IniPatch>> = OnceLock::new();

/// Sets the patches for the rest of the process, before the first scan. Returns whether they were
/// taken: they are fixed by the first call.
pub fn set_ini_patches(patches: Vec<IniPatch>) -> bool {
    PATCHES.set(patches).is_ok()
}

/// The patches set for this process; none by default.
pub fn ini_patches() -> &'static [IniPatch] {
    PATCHES.get().map_or(&[][..], Vec::as_slice)
}

#[cfg(test)]
mod tests {
    use super::*;

    const COMBAT: &str = "[attack1]\r\ndamage = 10\r\nforce = 5\r\n\r\n[attack2]\r\nforce = 5\r\ndamage = 0\r\n\r\n[attack3]\r\ndamage = 10\r\n";

    fn patch(sections: &[&str]) -> IniPatch {
        IniPatch {
            file: "combat.ini".into(),
            sections: sections.iter().map(|s| (*s).to_owned()).collect(),
            key: "Damage".into(),
            value: Some("100".into()),
            find: None,
            replace: None,
        }
    }

    #[test]
    fn it_changes_only_the_named_sections_and_keeps_the_line_endings() {
        let patched = patch(&["attack1", "ATTACK2"]).apply(COMBAT).unwrap();
        assert_eq!(
            patched,
            "[attack1]\r\ndamage = 100\r\nforce = 5\r\n\r\n[attack2]\r\nforce = 5\r\ndamage = 100\r\n\r\n[attack3]\r\ndamage = 10\r\n"
        );
    }

    #[test]
    fn a_missing_section_or_key_is_an_error_naming_it() {
        let error = patch(&["attack9"]).apply(COMBAT).unwrap_err().to_string();
        assert!(error.contains("no section [attack9]"), "{error}");
        let error = IniPatch {
            key: "arc".into(),
            ..patch(&["attack3"])
        }
        .apply(COMBAT)
        .unwrap_err()
        .to_string();
        assert!(error.contains("[attack3] has no key arc"), "{error}");
    }

    #[test]
    fn a_key_of_the_next_section_does_not_count() {
        let error = IniPatch {
            key: "arc".into(),
            ..patch(&["attack1"])
        }
        .apply("[attack1]\ndamage = 1\n[attack2]\narc = 3\n")
        .unwrap_err()
        .to_string();
        assert!(error.contains("[attack1] has no key arc"), "{error}");
    }

    #[test]
    fn a_fragment_of_the_value_is_replaced_and_the_rest_kept() {
        let menu = "[page3]
movie2_text[6]   =  0,255,255,0,997,1021,I,I have to go.
movie3_text[15] = x
";
        let fix = |find: &str| IniPatch {
            file: "MENU.INI".into(),
            sections: vec!["page3".into()],
            key: "movie2_text[6]".into(),
            value: None,
            find: Some(find.into()),
            replace: Some("I, I".into()),
        };
        assert_eq!(
            fix("I,I").apply(menu).unwrap(),
            "[page3]
movie2_text[6]   =  0,255,255,0,997,1021,I, I have to go.
movie3_text[15] = x
"
        );
        let error = fix("absent").apply(menu).unwrap_err().to_string();
        assert!(error.contains("exactly once"), "{error}");
        let error = fix(",").apply(menu).unwrap_err().to_string();
        assert!(error.contains("exactly once"), "{error}");
        let neither = IniPatch {
            find: None,
            replace: None,
            ..fix("I,I")
        };
        let error = neither.apply(menu).unwrap_err().to_string();
        assert!(
            error.contains("a value, or a find and a replace"),
            "{error}"
        );
    }

    #[test]
    fn only_patches_for_the_file_apply() {
        let other = IniPatch {
            file: "ITEM.INI".into(),
            ..patch(&["attack1"])
        };
        assert_eq!(apply_all(&[other], "COMBAT.INI", COMBAT).unwrap(), None);
        assert!(
            apply_all(&[patch(&["attack1"])], "COMBAT.INI", COMBAT)
                .unwrap()
                .is_some()
        );
    }
}
