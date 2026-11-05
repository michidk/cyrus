//! Configured cursor archive bytes prepared on demand for both hosts.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

/// Cache-relative resource path, independent of the installed archive filename.
pub const ASSET_PATH: &str = "cursor/pointers.cursor";

/// Original GXA bytes. Cursor descriptors remain unsupported until their representation is resolved.
#[derive(Debug, Serialize, Deserialize)]
pub struct CursorArchive {
    pub version: u32,
    /// Resolved install-relative archive path from SYSTEM.INI.
    pub source: String,
    pub bytes: Vec<u8>,
}

impl CursorArchive {
    /// Validate cache identity and payload presence without interpreting cursor descriptors.
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.version == 1,
            "cursor cache schema changed; prepare it again"
        );
        crate::validate_source_path(&self.source).context("cursor source")?;
        ensure!(
            !self.bytes.is_empty(),
            "{}: empty cursor archive",
            self.source
        );
        Ok(())
    }
}

/// Resolve the authored archive and check its GXA container using preservation.
#[cfg(feature = "parsers")]
pub fn prepare(source: &dyn crate::source::ReadAssets) -> Result<CursorArchive> {
    let system = rgpre::import::system_ini::SystemIni::parse(&crate::preparation::text(
        source,
        "SYSTEM.INI",
    )?)
    .context("cursor: SYSTEM.INI")?;
    let configured = system
        .system
        .pointers
        .context("SYSTEM.INI [system] pointers missing")?;
    let path = crate::preparation::path(source, &configured)
        .with_context(|| format!("SYSTEM.INI [system] pointers={configured}"))?;
    let bytes = source
        .read(&path)
        .with_context(|| format!("{path}: reading SYSTEM.INI [system] pointers archive"))?;
    rgpre::import::gxa::parse_gxa_file(&bytes)
        .with_context(|| format!("{path}: cursor archive from SYSTEM.INI [system] pointers"))?;
    let asset = CursorArchive {
        version: 1,
        source: path,
        bytes,
    };
    asset.validate()?;
    Ok(asset)
}

/// Prepare the same record as the browser worker, writing only outside the install.
#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
pub fn export(install: &crate::install::GameInstall, cache: &std::path::Path) -> Result<()> {
    let cache = install.cache_path(cache)?;
    let source = crate::native::Files::scan(install, false)?;
    let asset = prepare(&source)?;
    crate::native::publish(&cache.join(ASSET_PATH), &serde_json::to_vec(&asset)?)
}
