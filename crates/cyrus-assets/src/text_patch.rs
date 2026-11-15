//! Text patches: a mod's declared corrections to the lines of the game's dialogue file
//! (`ENGLISH.RTX`), by their four-character label. A patch names a short fragment of a line and
//! what replaces it, never a copy of the line; it is applied to the text when a line is read, so the
//! install is not written and removing the mod restores the original text. A line's on-screen
//! time follows its length, so it follows a correction too.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

/// Replaces one fragment of one dialogue line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextPatch {
    /// The line's four-character label (`$042`, `5drb`); case does not matter.
    pub label: String,
    /// A fragment of the line that occurs exactly once.
    pub find: String,
    /// What takes the place of the fragment.
    pub replace: String,
}

impl TextPatch {
    fn applies_to(&self, label: [u8; 4]) -> bool {
        self.label.as_bytes().eq_ignore_ascii_case(&label)
    }
}

/// Applies the patches for `label` to `text` in order. A fragment that is not in the line exactly
/// once is an error naming the label, so a patch written for another version of the file fails
/// instead of silently doing nothing.
pub fn apply_all(patches: &[TextPatch], label: [u8; 4], text: &mut String) -> Result<()> {
    for patch in patches.iter().filter(|patch| patch.applies_to(label)) {
        ensure!(
            text.matches(patch.find.as_str()).count() == 1,
            "dialogue label {}: the line does not contain {:?} exactly once",
            patch.label,
            patch.find
        );
        *text = text.replacen(patch.find.as_str(), &patch.replace, 1);
    }
    Ok(())
}

/// A digest of the patches, to fold into the cache identity of the dialogue file so that a cache
/// made without or with other patches is not reused.
pub fn digest(patches: &[TextPatch]) -> Option<String> {
    if patches.is_empty() {
        return None;
    }
    let mut hash = blake3::Hasher::new();
    for patch in patches {
        for field in [&patch.label, &patch.find, &patch.replace] {
            hash.update(&(field.len() as u64).to_le_bytes());
            hash.update(field.as_bytes());
        }
    }
    Some(hash.finalize().to_hex().to_string())
}

static PATCHES: OnceLock<Vec<TextPatch>> = OnceLock::new();

/// Sets the patches for the rest of the process, before the first scan. Returns whether they were
/// taken: they are fixed by the first call.
pub fn set_text_patches(patches: Vec<TextPatch>) -> bool {
    PATCHES.set(patches).is_ok()
}

/// The patches set for this process; none by default.
pub fn text_patches() -> &'static [TextPatch] {
    PATCHES.get().map_or(&[][..], Vec::as_slice)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn patch(label: &str, find: &str, replace: &str) -> TextPatch {
        TextPatch {
            label: label.into(),
            find: find.into(),
            replace: replace.into(),
        }
    }

    #[test]
    fn a_fragment_is_replaced_in_the_named_line_only() {
        let patches = [patch("5DRB", "DRAGGIN TALE", "DRAGGIN' TALE")];
        let mut line = "RUNS THE DRAGGIN TALE, THE TAVERN.".to_owned();
        apply_all(&patches, *b"5drb", &mut line).unwrap();
        assert_eq!(line, "RUNS THE DRAGGIN' TALE, THE TAVERN.");
        let mut other = "DRAGGIN TALE".to_owned();
        apply_all(&patches, *b"5dra", &mut other).unwrap();
        assert_eq!(other, "DRAGGIN TALE");
    }

    #[test]
    fn several_patches_for_a_line_apply_in_order() {
        let patches = [
            patch("xlga", "KILLED EM", "KILLED 'EM"),
            patch("xlga", "JOIN EM", "JOIN 'EM"),
        ];
        let mut line = "KILLED EM ALL. THEY WANT ME TO JOIN EM.".to_owned();
        apply_all(&patches, *b"xlga", &mut line).unwrap();
        assert_eq!(line, "KILLED 'EM ALL. THEY WANT ME TO JOIN 'EM.");
    }

    #[test]
    fn a_fragment_that_is_absent_or_ambiguous_is_an_error_naming_the_label() {
        for find in ["NOWHERE", "A"] {
            let error = apply_all(&[patch("0b13", find, "B")], *b"0b13", &mut "A..A".into())
                .unwrap_err()
                .to_string();
            assert!(
                error.contains("0b13") && error.contains("exactly once"),
                "{error}"
            );
        }
    }

    #[test]
    fn the_digest_changes_with_the_patches_and_is_absent_without() {
        assert_eq!(digest(&[]), None);
        let one = digest(&[patch("a", "b", "c")]);
        assert!(one.is_some());
        assert_ne!(one, digest(&[patch("a", "b", "d")]));
        assert_ne!(
            digest(&[patch("ab", "c", "d")]),
            digest(&[patch("a", "bc", "d")]),
            "field boundaries count"
        );
    }
}
