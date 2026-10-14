//! Runtime preferences retain load failures and commit only after host persistence.
use bevy::prelude::*;
use cyrus_assets::preferences::MenuPreferences;

/// The two existing local-state documents; neither belongs in the install.
#[derive(Clone, Copy)]
pub enum PreferenceKey {
    Menu,
    Hud,
}
impl PreferenceKey {
    pub fn name(self) -> &'static str {
        match self {
            Self::Menu => "menu",
            Self::Hud => "hud",
        }
    }
}
pub trait PreferenceStore: Send + Sync + 'static {
    fn load(&self, key: PreferenceKey) -> anyhow::Result<Option<String>>;
    fn save(&self, key: PreferenceKey, value: &str) -> anyhow::Result<()>;
}
#[derive(Resource)]
pub struct Preferences {
    store: Option<Box<dyn PreferenceStore>>,
    menu: Result<Option<MenuPreferences>, String>,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            store: None,
            menu: Err("local preference store unavailable".into()),
        }
    }
}
impl Preferences {
    /// Last accepted menu state or load error; changes must go through `save_menu`.
    pub fn menu(&self) -> &Result<Option<MenuPreferences>, String> {
        &self.menu
    }

    /// Loads once; each consumer resolves missing state using its own installed defaults.
    pub fn new(store: impl PreferenceStore) -> Self {
        let menu = (|| {
            store
                .load(PreferenceKey::Menu)?
                .map(|text| {
                    let value: MenuPreferences = serde_json::from_str(&text)?;
                    value.validate()?;
                    Ok(value)
                })
                .transpose()
        })()
        .map_err(|e: anyhow::Error| format!("local menu preferences: {e:#}"));
        Self {
            store: Some(Box::new(store)),
            menu,
        }
    }
    pub fn load_hud(&self) -> anyhow::Result<Option<String>> {
        self.store
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("local preference store unavailable"))?
            .load(PreferenceKey::Hud)
    }
    pub fn save_hud(&self, text: &str) -> anyhow::Result<()> {
        self.store
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("local preference store unavailable"))?
            .save(PreferenceKey::Hud, text)
    }
    /// A failed load or write never becomes a default or replaces the last accepted value.
    pub fn save_menu(&mut self, value: MenuPreferences) -> anyhow::Result<()> {
        if let Err(error) = &self.menu {
            anyhow::bail!("{error}");
        }
        value.validate()?;
        self.store
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("local preference store unavailable"))?
            .save(PreferenceKey::Menu, &serde_json::to_string_pretty(&value)?)?;
        self.menu = Ok(Some(value));
        Ok(())
    }
}
