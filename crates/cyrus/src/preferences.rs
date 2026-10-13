//! Native atomic local-state persistence, outside the read-only install.
use cyrus_game::preferences::{PreferenceKey, PreferenceStore};
pub struct LocalPreferences;
impl PreferenceStore for LocalPreferences {
    fn load(&self, key: PreferenceKey) -> anyhow::Result<Option<String>> {
        let path =
            std::path::Path::new(crate::settings::CONFIG_DIR).join(format!("{}.json", key.name()));
        crate::storage::read(&path)?
            .map(|bytes| String::from_utf8(bytes).map_err(Into::into))
            .transpose()
    }

    fn save(&self, key: PreferenceKey, value: &str) -> anyhow::Result<()> {
        let directory = std::path::Path::new(crate::settings::CONFIG_DIR);
        let path = directory.join(format!("{}.json", key.name()));
        crate::storage::write(&path, value.as_bytes())
    }
}
