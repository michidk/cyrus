//! Configured cursor archive bytes prepared on demand for both hosts.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

/// Cache-relative resource path, independent of the installed archive filename.
pub const ASSET_PATH: &str = "cursor/pointers.cursor";

/// Original GXA bytes, retained independently of the software descriptor representation.
#[derive(Debug, Serialize, Deserialize)]
pub struct CursorArchive {
    pub version: u32,
    /// Resolved install-relative archive path from SYSTEM.INI.
    pub source: String,
    pub bytes: Vec<u8>,
}

/// Decoded software cursor, with palette-expanded pixels and an optional authored offset.
/// These offsets must never be used for Glide cursor preparation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoftwareCursor {
    /// Image width and height in pixels.
    pub size: [u16; 2],
    /// Top-to-bottom, straight-alpha RGBA pixels; index zero is transparent.
    pub rgba: Vec<u8>,
    /// Last index-16 marker coordinate, or unknown when no marker was authored.
    pub hotspot: Option<[i16; 2]>,
}

impl SoftwareCursor {
    /// Place the software image at pointer minus hotspot, wrapping each signed
    /// sixteen-bit coordinate as specified by preservation's cursor draw contract.
    /// Unknown authored offsets cannot produce an admitted placement.
    pub fn top_left(&self, pointer: [i16; 2]) -> Option<[i16; 2]> {
        self.hotspot.map(|hotspot| {
            [
                pointer[0].wrapping_sub(hotspot[0]),
                pointer[1].wrapping_sub(hotspot[1]),
            ]
        })
    }
}

impl CursorArchive {
    /// Decode the admitted software frame pairs using preservation's cursor rules.
    ///
    /// The original bytes remain authoritative. Missing hotspots remain unknown;
    /// this method neither selects a cursor nor substitutes software offsets for Glide.
    #[cfg(feature = "parsers")]
    pub fn software_frames(&self) -> Result<Vec<SoftwareCursor>> {
        self.validate()?;
        Ok(rgpre::import::gxa::parse_software_cursor_file(&self.bytes)
            .with_context(|| format!("{}: software cursor descriptors", self.source))?
            .into_iter()
            .map(|cursor| SoftwareCursor {
                size: [cursor.image.width, cursor.image.height],
                rgba: cursor.image.rgba,
                hotspot: cursor.hotspot,
            })
            .collect())
    }

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
