//! Typed browser documents; source admission and live replacement remain separate.
use anyhow::{Context, Result};
use cyrus_session::RuntimeSessionSnapshot;

pub async fn read(database: &str, key: &str) -> Result<Option<RuntimeSessionSnapshot>> {
    super::storage::read(database, key)
        .await?
        .map(|document| {
            RuntimeSessionSnapshot::decode_document(document.as_bytes())
                .with_context(|| format!("decoding session {database}/{key}"))
        })
        .transpose()
}

pub async fn write(database: &str, key: &str, snapshot: &RuntimeSessionSnapshot) -> Result<()> {
    let bytes = snapshot
        .encode_document()
        .with_context(|| format!("encoding session {database}/{key}"))?;
    let document = std::str::from_utf8(&bytes)
        .with_context(|| format!("encoding session {database}/{key}: document is not UTF-8"))?;
    super::storage::write(database, key, document).await
}
