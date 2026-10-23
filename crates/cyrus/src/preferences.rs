//! Native atomic local-state persistence, outside the read-only install.
use cyrus_game::preferences::{PreferenceKey, PreferenceStore};
pub struct LocalPreferences;
impl PreferenceStore for LocalPreferences {
    fn load(&self, key: PreferenceKey) -> anyhow::Result<Option<String>> {
        let path = cyrus_config::config_dir().join(format!("{}.json", key.name()));
        cyrus_config::read(&path)?
            .map(|bytes| String::from_utf8(bytes).map_err(Into::into))
            .transpose()
    }

    fn save(&self, key: PreferenceKey, value: &str) -> anyhow::Result<()> {
        let path = cyrus_config::config_dir().join(format!("{}.json", key.name()));
        cyrus_config::write(&path, value.as_bytes())
    }
}
