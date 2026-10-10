//! Local book preferences, expressed in original menu action units.
use anyhow::{Context, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Toggle/volume preferences and bindings for original actions 710 through 724.
/// Missing local state uses the installed defaults exported by rgpre.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MenuPreferences {
    pub values: BTreeMap<i32, i32>,
    pub bindings: [u8; 15],
}

impl MenuPreferences {
    pub(crate) fn validate(&self) -> anyhow::Result<()> {
        ensure!(
            self.values.len() == 5,
            "menu preferences: expected five settings"
        );
        for (action, max) in [(504, 1), (505, 1), (601, 20), (602, 20), (603, 1)] {
            ensure!(
                self.values
                    .get(&action)
                    .is_some_and(|v| (0..=max).contains(v)),
                "menu preferences: invalid action {action} value"
            );
        }
        ensure!(
            self.bindings.iter().all(|code| *code <= 139),
            "menu preferences: invalid binding code"
        );
        Ok(())
    }

    /// Read local preferences without touching the install. Broken state is an error.
    pub fn load() -> anyhow::Result<Option<Self>> {
        #[cfg(not(target_arch = "wasm32"))]
        let text = match std::fs::read_to_string(Self::path()) {
            Ok(text) => Some(text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e).context("reading local menu preferences"),
        };
        #[cfg(target_arch = "wasm32")]
        let text = Self::storage()?
            .get_item("cyrus-menu")
            .map_err(|e| anyhow::anyhow!("reading menu preferences: {e:?}"))?;
        text.map(|text| {
            let preferences: Self =
                serde_json::from_str(&text).context("local menu preferences")?;
            preferences.validate()?;
            Ok(preferences)
        })
        .transpose()
    }

    pub(crate) fn save(&self) -> anyhow::Result<()> {
        self.validate()?;
        let text = serde_json::to_string_pretty(self)?;
        #[cfg(not(target_arch = "wasm32"))]
        {
            let path = Self::path();
            std::fs::create_dir_all(path.parent().unwrap())?;
            let temporary = path.with_extension("json.tmp");
            std::fs::write(&temporary, text).context("writing local menu preferences")?;
            std::fs::rename(temporary, path).context("saving local menu preferences")?;
        }
        #[cfg(target_arch = "wasm32")]
        Self::storage()?
            .set_item("cyrus-menu", &text)
            .map_err(|e| anyhow::anyhow!("saving menu preferences: {e:?}"))?;
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn path() -> std::path::PathBuf {
        std::path::Path::new(crate::settings::CONFIG_DIR).join("menu.json")
    }

    #[cfg(target_arch = "wasm32")]
    fn storage() -> anyhow::Result<web_sys::Storage> {
        web_sys::window()
            .and_then(|w| w.local_storage().ok().flatten())
            .context("browser local storage unavailable")
    }

    pub(crate) fn rebind(&mut self, index: usize, code: u8) {
        if let Some(conflict) = self.bindings.iter().position(|other| *other == code) {
            self.bindings[conflict] = 0;
        }
        self.bindings[index] = code;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reassignment_clears_first_conflict_and_preserves_other_controls() {
        let mut preferences = MenuPreferences {
            values: BTreeMap::new(),
            bindings: [0; 15],
        };
        preferences.bindings[0] = 17;
        preferences.bindings[1] = 31;
        preferences.rebind(1, 17);
        assert_eq!(&preferences.bindings[..3], &[0, 17, 0]);
        assert!(preferences.validate().is_err());
    }
}
