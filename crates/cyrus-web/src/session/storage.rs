//! Asynchronous browser session publication; callers own document validation.
use anyhow::{Context, Result};
use wasm_bindgen::prelude::*;

#[wasm_bindgen(module = "/session-storage.js")]
extern "C" {
    #[wasm_bindgen(catch, js_name = read)]
    async fn read_document(database: &str, key: &str) -> Result<JsValue, JsValue>;
    #[wasm_bindgen(catch, js_name = write)]
    async fn write_document(database: &str, key: &str, value: &str) -> Result<JsValue, JsValue>;
}

pub async fn read(database: &str, key: &str) -> Result<Option<String>> {
    let value = read_document(database, key)
        .await
        .map_err(|error| anyhow::anyhow!("{error:?}"))
        .with_context(|| format!("reading session {database}/{key}"))?;
    if value.is_undefined() {
        Ok(None)
    } else {
        value.as_string().map(Some).with_context(|| {
            format!("reading session {database}/{key}: stored document is not text")
        })
    }
}

pub async fn write(database: &str, key: &str, document: &str) -> Result<()> {
    write_document(database, key, document)
        .await
        .map_err(|error| anyhow::anyhow!("{error:?}"))
        .with_context(|| format!("writing session {database}/{key}"))?;
    Ok(())
}
