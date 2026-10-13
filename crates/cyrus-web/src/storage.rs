//! Shared browser document storage for preferences, cheats and future save records.
#[cfg(target_arch = "wasm32")]
fn storage() -> anyhow::Result<web_sys::Storage> {
    web_sys::window()
        .and_then(|window| window.local_storage().ok().flatten())
        .ok_or_else(|| anyhow::anyhow!("browser local storage unavailable"))
}

pub(crate) fn read(key: &str) -> anyhow::Result<Option<String>> {
    #[cfg(target_arch = "wasm32")]
    return storage()?
        .get_item(key)
        .map_err(|error| anyhow::anyhow!("reading {key}: {error:?}"));
    #[cfg(not(target_arch = "wasm32"))]
    anyhow::bail!("browser local storage unavailable: {key}");
}

pub(crate) fn write(key: &str, value: &str) -> anyhow::Result<()> {
    #[cfg(target_arch = "wasm32")]
    return storage()?
        .set_item(key, value)
        .map_err(|error| anyhow::anyhow!("writing {key}: {error:?}"));
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = value;
        anyhow::bail!("browser local storage unavailable: {key}");
    }
}
