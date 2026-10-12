//! Browser local-state persistence using the existing storage keys.
use cyrus_game::preferences::{PreferenceKey, PreferenceStore};
pub struct LocalPreferences;
impl PreferenceStore for LocalPreferences {
    fn load(&self, key: PreferenceKey) -> anyhow::Result<Option<String>> {
        #[cfg(target_arch = "wasm32")]
        return storage()?
            .get_item(&format!("cyrus-{}", key.name()))
            .map_err(|e| anyhow::anyhow!("reading {} preferences: {e:?}", key.name()));
        #[cfg(not(target_arch = "wasm32"))]
        {
            let _ = key;
            anyhow::bail!("browser local storage unavailable")
        }
    }
    fn save(&self, key: PreferenceKey, value: &str) -> anyhow::Result<()> {
        #[cfg(target_arch = "wasm32")]
        return storage()?
            .set_item(&format!("cyrus-{}", key.name()), value)
            .map_err(|e| anyhow::anyhow!("saving {} preferences: {e:?}", key.name()));
        #[cfg(not(target_arch = "wasm32"))]
        {
            let _ = (key, value);
            anyhow::bail!("browser local storage unavailable")
        }
    }
}
#[cfg(target_arch = "wasm32")]
fn storage() -> anyhow::Result<web_sys::Storage> {
    web_sys::window()
        .and_then(|w| w.local_storage().ok().flatten())
        .ok_or_else(|| anyhow::anyhow!("browser local storage unavailable"))
}
