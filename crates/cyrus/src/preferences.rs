//! Native atomic local-state persistence, outside the read-only install.
use cyrus_game::preferences::{PreferenceKey, PreferenceStore};
pub struct LocalPreferences;
impl PreferenceStore for LocalPreferences {
    fn load(&self, key: PreferenceKey) -> anyhow::Result<Option<String>> {
        let path =
            std::path::Path::new(crate::settings::CONFIG_DIR).join(format!("{}.json", key.name()));
        match std::fs::read_to_string(path) {
            Ok(text) => Ok(Some(text)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }
    fn save(&self, key: PreferenceKey, value: &str) -> anyhow::Result<()> {
        let directory = std::path::Path::new(crate::settings::CONFIG_DIR);
        std::fs::create_dir_all(directory)?;
        let path = directory.join(format!("{}.json", key.name()));
        let temporary = path.with_extension("json.tmp");
        std::fs::write(&temporary, value)?;
        std::fs::rename(temporary, path)?;
        Ok(())
    }
}
