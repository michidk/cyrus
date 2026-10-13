//! Browser local-state persistence using the existing storage keys.
use cyrus_game::preferences::{PreferenceKey, PreferenceStore};
pub struct LocalPreferences;
impl PreferenceStore for LocalPreferences {
    fn load(&self, key: PreferenceKey) -> anyhow::Result<Option<String>> {
        crate::storage::read(&format!("cyrus-{}", key.name()))
    }
    fn save(&self, key: PreferenceKey, value: &str) -> anyhow::Result<()> {
        crate::storage::write(&format!("cyrus-{}", key.name()), value)
    }
}
