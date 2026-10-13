//! Original-file access. Browser workers and native installs share the same parsers.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::io::{Read, Seek, SeekFrom};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FileInfo {
    pub path: String,
    pub bytes: u64,
    /// Content digest supplied by the file listing, never a decoded asset.
    pub digest: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Manifest {
    pub files: Vec<FileInfo>,
}
impl Manifest {
    pub fn validate(&self) -> Result<()> {
        let mut paths = std::collections::HashSet::new();
        for file in &self.files {
            crate::validate_path(&file.path)?;
            ensure!(
                paths.insert(file.path.to_ascii_lowercase()),
                "duplicate source path {}",
                file.path
            );
            ensure!(
                file.digest.len() == 64 && file.digest.bytes().all(|c| c.is_ascii_hexdigit()),
                "{}: invalid content digest",
                file.path
            );
        }
        Ok(())
    }
    pub fn find(&self, path: &str) -> Option<&FileInfo> {
        self.files
            .iter()
            .find(|file| file.path.eq_ignore_ascii_case(path))
    }
}

/// A failure retained by asset preparation without changing its fallback behavior.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct PreparationDiagnostic {
    pub category: String,
    pub file: String,
    pub field: String,
    pub reason: String,
    /// Number of affected placements or asset references.
    pub count: usize,
}

/// Reads only files listed in the supplied manifest. Ranges are half-open byte ranges.
/// Implementations must reject truncated responses and keep the original files read-only.
pub trait ReadAssets {
    fn manifest(&self) -> &Manifest;
    /// Optional audit observer. Files and fields describe original inputs.
    fn diagnostic(&self, _diagnostic: PreparationDiagnostic) {}
    /// Optional progress for a selected resource, outside initial catalog loading.
    fn progress(&self, _done: usize, _total: usize, _label: &str) {}
    fn read_range(&self, path: &str, start: u64, length: usize) -> Result<Vec<u8>>;
    fn read(&self, path: &str) -> Result<Vec<u8>> {
        let file = self
            .manifest()
            .find(path)
            .with_context(|| format!("missing original file {path}"))?;
        ensure!(
            file.bytes <= 512 * 1024 * 1024,
            "{path}: file exceeds the 512 MiB decoding limit; use range access"
        );
        self.read_range(&file.path, 0, usize::try_from(file.bytes)?)
    }
    fn text(&self, path: &str) -> Result<String> {
        String::from_utf8(self.read(path)?)
            .with_context(|| format!("{path}: invalid text encoding"))
    }
}

/// Seekable original file with a bounded read-ahead buffer for CD-sector parsers.
pub struct AssetFile<'a> {
    source: &'a dyn ReadAssets,
    path: String,
    length: u64,
    position: u64,
    buffer_start: u64,
    buffer: Vec<u8>,
    read_ahead: bool,
}
impl<'a> AssetFile<'a> {
    pub fn open(source: &'a dyn ReadAssets, path: &str) -> Result<Self> {
        let file = source
            .manifest()
            .find(path)
            .with_context(|| format!("missing original file {path}"))?;
        Ok(Self {
            source,
            path: file.path.clone(),
            length: file.bytes,
            position: 0,
            buffer_start: 0,
            buffer: Vec::new(),
            read_ahead: true,
        })
    }
    /// Use exact requested ranges when reading sparse archive directories and members.
    pub fn without_read_ahead(mut self) -> Self {
        self.read_ahead = false;
        self
    }
}
impl Read for AssetFile<'_> {
    fn read(&mut self, destination: &mut [u8]) -> std::io::Result<usize> {
        if self.position >= self.length || destination.is_empty() {
            return Ok(0);
        }
        if self.position < self.buffer_start
            || self.position >= self.buffer_start + self.buffer.len() as u64
        {
            const BLOCK: u64 = 1024 * 1024;
            self.buffer_start = if self.read_ahead {
                self.position / BLOCK * BLOCK
            } else {
                self.position
            };
            let maximum = if self.read_ahead {
                BLOCK
            } else {
                destination.len() as u64
            };
            let length = (self.length - self.buffer_start).min(maximum) as usize;
            self.buffer = self
                .source
                .read_range(&self.path, self.buffer_start, length)
                .map_err(|error| std::io::Error::other(format!("{error:#}")))?;
            if self.buffer.len() != length {
                return Err(std::io::Error::other("truncated original-file range"));
            }
        }
        let offset = (self.position - self.buffer_start) as usize;
        let count = destination.len().min(self.buffer.len() - offset);
        destination[..count].copy_from_slice(&self.buffer[offset..offset + count]);
        self.position += count as u64;
        Ok(count)
    }
}
impl Seek for AssetFile<'_> {
    fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
        let position = match position {
            SeekFrom::Start(position) => i128::from(position),
            SeekFrom::Current(offset) => i128::from(self.position) + i128::from(offset),
            SeekFrom::End(offset) => i128::from(self.length) + i128::from(offset),
        };
        self.position = u64::try_from(position).map_err(std::io::Error::other)?;
        Ok(self.position)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Bytes {
        manifest: Manifest,
        truncate: bool,
    }
    impl ReadAssets for Bytes {
        fn manifest(&self) -> &Manifest {
            &self.manifest
        }
        fn read_range(&self, _: &str, start: u64, length: usize) -> Result<Vec<u8>> {
            let length = length - usize::from(self.truncate);
            Ok((start..start + length as u64)
                .map(|n| (n % 251) as u8)
                .collect())
        }
    }
    #[test]
    fn original_reader_seeks_across_blocks_and_rejects_truncation() {
        let mut source = Bytes {
            manifest: Manifest {
                files: vec![FileInfo {
                    path: "disc/game.gog".into(),
                    bytes: 2_000_000,
                    digest: "a".repeat(64),
                }],
            },
            truncate: false,
        };
        let mut file = AssetFile::open(&source, "disc/GAME.GOG").unwrap();
        file.seek(SeekFrom::Start(1_048_574)).unwrap();
        let mut bytes = [0; 8];
        file.read_exact(&mut bytes).unwrap();
        assert_eq!(bytes, [147, 148, 149, 150, 151, 152, 153, 154]);
        file.seek(SeekFrom::End(-2)).unwrap();
        assert_eq!(file.read(&mut bytes).unwrap(), 2);
        assert_eq!(file.read(&mut bytes).unwrap(), 0);
        assert!(file.seek(SeekFrom::Start(0)).is_ok());
        assert!(file.seek(SeekFrom::Current(-1)).is_err());
        source.truncate = true;
        assert!(
            AssetFile::open(&source, "disc/game.gog")
                .unwrap()
                .read(&mut bytes)
                .is_err()
        );
    }
}
