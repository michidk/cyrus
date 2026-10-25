//! Asynchronous browser session publication; callers own document validation.
use anyhow::{Context, Result};
use wasm_bindgen::prelude::*;

#[wasm_bindgen(module = "/session-storage.js")]
extern "C" {
    #[wasm_bindgen(catch, js_name = read)]
    async fn read_document(database: &str, key: &str) -> Result<JsValue, JsValue>;
    #[wasm_bindgen(catch, js_name = write)]
    async fn write_document(database: &str, key: &str, value: &str) -> Result<JsValue, JsValue>;
    #[wasm_bindgen(catch, js_name = operateSlots)]
    async fn operate_slots(
        database: &str,
        request: &str,
        document: &str,
    ) -> Result<JsValue, JsValue>;
}

/// Manual slot operations; revision tokens reject lists made stale by another tab.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum SlotRequest {
    List,
    QuickSave,
    AutoSave,
    Create {
        description: String,
    },
    Overwrite {
        slot: u16,
        revision: String,
        description: String,
    },
    Load {
        slot: u16,
        revision: String,
    },
    Delete {
        slot: u16,
        revision: String,
    },
}

pub async fn slots(request: &SlotRequest, document: &str) -> Result<serde_json::Value> {
    operate(serde_json::to_value(request)?, document).await
}

pub async fn plan(request: &SlotRequest) -> Result<serde_json::Value> {
    let mut request = serde_json::to_value(request)?;
    request["prepare"] = true.into();
    operate(request, "").await
}

pub async fn publish(
    request: &SlotRequest,
    slot: u16,
    document: &str,
) -> Result<serde_json::Value> {
    let mut encoded = serde_json::to_value(request)?;
    if matches!(
        request,
        SlotRequest::Create { .. } | SlotRequest::QuickSave | SlotRequest::AutoSave
    ) {
        encoded["expected_slot"] = slot.into();
    }
    operate(encoded, document).await
}

async fn operate(request: serde_json::Value, document: &str) -> Result<serde_json::Value> {
    let database = "cyrus-save-slots";
    let request = serde_json::to_string(&request)?;
    let value = operate_slots(database, &request, document)
        .await
        .map_err(|error| anyhow::anyhow!("{error:?}"))
        .with_context(|| format!("session slots {database}"))?;
    let text = value
        .as_string()
        .context("session slot result is not text")?;
    serde_json::from_str(&text).context("decoding session slot result")
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
