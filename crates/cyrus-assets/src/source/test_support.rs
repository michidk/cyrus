//! In-memory original-file access for parser tests and specialized test wrappers.
use super::{FileInfo, Manifest, ReadAssets};
use anyhow::{Context, Result};
use std::collections::BTreeMap;

/// An owned, read-only byte source with a validated manifest.
///
/// Manifest entries retain construction order and spelling, with actual byte lengths
/// and BLAKE3 content digests. Reads match paths case-insensitively and reject
/// missing files, overflowing ranges and ranges beyond the supplied bytes.
/// This fixture is available only with the `test-support` feature or in unit tests.
#[derive(Clone, Debug)]
pub struct MemoryAssets {
    manifest: Manifest,
    files: BTreeMap<String, Vec<u8>>,
}

impl Default for MemoryAssets {
    fn default() -> Self {
        Self {
            manifest: Manifest { files: Vec::new() },
            files: BTreeMap::new(),
        }
    }
}

impl MemoryAssets {
    /// Build a fixture from portable source paths and their complete file bytes.
    /// Invalid paths and case-insensitive duplicate paths return contextual errors.
    pub fn new<P: Into<String>, B: Into<Vec<u8>>>(
        files: impl IntoIterator<Item = (P, B)>,
    ) -> Result<Self> {
        let files: Vec<(String, Vec<u8>)> = files
            .into_iter()
            .map(|(path, bytes)| (path.into(), bytes.into()))
            .collect();
        let manifest = Manifest {
            files: files
                .iter()
                .map(|(path, bytes)| FileInfo {
                    path: path.clone(),
                    bytes: bytes.len() as u64,
                    digest: blake3::hash(bytes).to_hex().to_string(),
                })
                .collect(),
        };
        manifest.validate()?;
        Ok(Self {
            manifest,
            files: files
                .into_iter()
                .map(|(path, bytes)| (path.to_ascii_lowercase(), bytes))
                .collect(),
        })
    }
}

impl ReadAssets for MemoryAssets {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn read_range(&self, path: &str, start: u64, length: usize) -> Result<Vec<u8>> {
        let bytes = self
            .files
            .get(&path.to_ascii_lowercase())
            .with_context(|| format!("missing original file {path}"))?;
        let start = usize::try_from(start)
            .with_context(|| format!("{path}: range start {start} cannot be addressed"))?;
        let end = start
            .checked_add(length)
            .with_context(|| format!("{path}: range length {length} overflows at {start}"))?;
        bytes
            .get(start..end)
            .map(<[u8]>::to_vec)
            .with_context(|| format!("{path}: range {start}..{end} outside {} bytes", bytes.len()))
    }
}
