//! Cache asset reads await original-file preparation in the browser worker.
use bevy::{
    asset::io::{AssetReader, AssetReaderError, AssetSourceBuilder, PathStream, VecReader},
    prelude::*,
};
use std::path::Path;
use wasm_bindgen::prelude::*;

#[wasm_bindgen(
    inline_js = "export async function readGameAsset(path) { return await globalThis.cyrusGameAssets.read(path); }"
)]
extern "C" {
    #[wasm_bindgen(catch, js_name = readGameAsset)]
    async fn read_game_asset(path: &str) -> Result<JsValue, JsValue>;
}
struct GameReader;
impl AssetReader for GameReader {
    async fn read<'a>(&'a self, path: &'a Path) -> Result<VecReader, AssetReaderError> {
        let name = path
            .to_str()
            .ok_or_else(|| AssetReaderError::NotFound(path.to_owned()))?;
        let bytes = read_game_asset(name).await.map_err(|error| {
            AssetReaderError::Io(std::io::Error::other(format!("{name}: {error:?}")).into())
        })?;
        Ok(VecReader::new(js_sys::Uint8Array::new(&bytes).to_vec()))
    }
    async fn read_meta<'a>(&'a self, path: &'a Path) -> Result<VecReader, AssetReaderError> {
        Err(AssetReaderError::NotFound(path.to_owned()))
    }
    async fn read_directory<'a>(
        &'a self,
        path: &'a Path,
    ) -> Result<Box<PathStream>, AssetReaderError> {
        Err(AssetReaderError::NotFound(path.to_owned()))
    }
    async fn is_directory<'a>(&'a self, _: &'a Path) -> Result<bool, AssetReaderError> {
        Ok(false)
    }
}

pub fn register(app: &mut App) {
    app.register_asset_source("cache", AssetSourceBuilder::new(|| Box::new(GameReader)));
}
