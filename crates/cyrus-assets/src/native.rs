//! Read-only filesystem adapter and optional disk cache for the shared runtime parsers.
use crate::{
    Catalog, Preview,
    install::GameInstall,
    runtime,
    source::{FileInfo, Manifest, ReadAssets},
};
use anyhow::{Context, Result, ensure};
use std::{
    collections::BTreeMap,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::Mutex,
};

pub mod geometry;

pub struct Files {
    manifest: Manifest,
    paths: BTreeMap<String, PathBuf>,
}
impl Files {
    pub fn open(install: &GameInstall) -> Result<Self> {
        Self::scan(install, true)
    }
    pub(crate) fn scan(install: &GameInstall, fingerprint: bool) -> Result<Self> {
        let mut paths = BTreeMap::new();
        for item in walkdir::WalkDir::new(&install.root).follow_links(false) {
            let item = item?;
            if item.file_type().is_file() {
                let relative = item
                    .path()
                    .strip_prefix(&install.root)?
                    .to_string_lossy()
                    .replace('\\', "/");
                paths.insert(relative, item.into_path());
            }
        }
        if let Some(parent) = install.root.parent() {
            for name in ["game.gog", "game.ins"] {
                let path = parent.join(name);
                if path.is_file() {
                    paths.insert(format!("disc/{name}"), path);
                }
            }
            if let Some(path) = [
                install.root.join("glide2x_emu.ovl"),
                parent.join("DOSBOX/glide2x_emu.ovl"),
            ]
            .into_iter()
            .find(|path| path.is_file())
            {
                paths.insert("splash/glide2x_emu.ovl".into(), path);
            }
        }
        if let Some(parent) = install.root.parent() {
            let path = parent.join("DOSBOX/3DfxSpl2.dll");
            if path.is_file() {
                paths.insert("splash/3DfxSpl2.dll".into(), path);
            }
        }
        let mut files = Vec::new();
        for (relative, path) in &paths {
            let mut hash = blake3::Hasher::new();
            if !fingerprint || relative.starts_with("disc/") {
                // Preparation scans and large CD images use size/mtime; catalog files use content.
                let metadata = path.metadata()?;
                hash.update(&metadata.len().to_le_bytes());
                let modified = metadata.modified()?.duration_since(std::time::UNIX_EPOCH)?;
                hash.update(&modified.as_nanos().to_le_bytes());
            } else {
                hash.update_reader(std::fs::File::open(path)?)?;
            }
            files.push(FileInfo {
                path: relative.clone(),
                bytes: path.metadata()?.len(),
                digest: hash.finalize().to_hex().to_string(),
            });
        }
        Ok(Self {
            manifest: Manifest { files },
            paths,
        })
    }
}
impl ReadAssets for Files {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }
    fn read_range(&self, path: &str, start: u64, length: usize) -> Result<Vec<u8>> {
        let info = self
            .manifest
            .find(path)
            .with_context(|| format!("unknown original file {path}"))?;
        ensure!(
            start <= info.bytes && length as u64 <= info.bytes - start,
            "{path}: range outside original file"
        );
        let path = self
            .paths
            .get(&info.path)
            .context("original source disappeared")?;
        let mut file = std::fs::File::open(path)?;
        file.seek(SeekFrom::Start(start))?;
        let mut bytes = vec![0; length];
        file.read_exact(&mut bytes)?;
        Ok(bytes)
    }
}

pub struct Library {
    pub install: GameInstall,
    pub catalog: Catalog,
    pub cache: PathBuf,
    runtime: Mutex<runtime::Library<Files>>,
}
impl Library {
    pub fn open(
        install: GameInstall,
        cache: PathBuf,
        progress: impl Fn(usize, usize, &str),
    ) -> Result<Self> {
        let cache = install.cache_path(&cache)?;
        let runtime = runtime::Library::open(Files::open(&install)?, progress)?;
        let catalog = runtime.catalog.clone();
        let bytes = serde_json::to_vec(&catalog)?;
        publish(
            &cache
                .join("viewer")
                .join(&catalog.generation)
                .join("catalog.json"),
            &bytes,
        )?;
        publish(&cache.join("viewer/catalog.json"), &bytes)?;
        Ok(Self {
            install,
            catalog,
            cache,
            runtime: Mutex::new(runtime),
        })
    }
    /// Reads an archive index and returns the updated runtime catalog.
    pub fn expand(&self, path: &str) -> Result<Catalog> {
        let mut runtime = self
            .runtime
            .lock()
            .map_err(|_| anyhow::anyhow!("asset decoder unavailable"))?;
        runtime.expand(path)?;
        Ok(runtime.catalog.clone())
    }
    /// Locates an asset by ID, reading archive indexes as needed, and returns the updated catalog.
    pub fn resolve(&self, id: &str, path: Option<&str>) -> Result<Catalog> {
        let mut runtime = self
            .runtime
            .lock()
            .map_err(|_| anyhow::anyhow!("asset decoder unavailable"))?;
        runtime.resolve(id, path)?;
        Ok(runtime.catalog.clone())
    }
    pub fn prepare(&self, id: &str, palette: Option<&str>) -> Result<PathBuf> {
        let mut runtime = self
            .runtime
            .lock()
            .map_err(|_| anyhow::anyhow!("asset decoder unavailable"))?;
        let entry = runtime
            .catalog
            .entries
            .iter()
            .find(|entry| entry.id == id)
            .context("unknown catalog item")?;
        let path = self.cache.join(self.catalog.preview_path(entry, palette));
        if let Ok(bytes) = std::fs::read(&path)
            && let Ok(preview) = serde_json::from_slice::<Preview>(&bytes)
            && preview.validate().is_ok()
            && !matches!(preview.content, crate::Content::Unavailable { .. })
            && preview.files().iter().all(|file| {
                self.cache
                    .join(file)
                    .metadata()
                    .is_ok_and(|meta| meta.len() > 0)
            })
        {
            return Ok(path);
        }
        let prepared = runtime.prepare(id, palette)?;
        for (file, bytes) in prepared.files {
            publish(&self.cache.join(file), &bytes)?;
        }
        publish(&path, &serde_json::to_vec(&prepared.preview)?)?;
        Ok(path)
    }
}

/// Rename publishes a complete file; temporary names do not collide between processes.
/// A file that already holds these bytes is left alone: replacing it is wasted work, and on
/// Windows the rename fails with "Access is denied" while another program has the file open.
pub fn publish(path: &Path, bytes: &[u8]) -> Result<()> {
    if std::fs::read(path).is_ok_and(|existing| existing == bytes) {
        return Ok(());
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    let temp = path.with_extension(format!(
        "tmp-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    ));
    std::fs::write(&temp, bytes).with_context(|| format!("write {}", temp.display()))?;
    if let Err(error) = std::fs::rename(&temp, path) {
        let _ = std::fs::remove_file(temp);
        return Err(error).with_context(|| format!("replace {}", path.display()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{Category, Source, runtime::entry};

    #[test]
    fn publish_replaces_changed_files_and_skips_identical_ones() {
        let dir = std::env::temp_dir().join(format!("cyrus-publish-{}", std::process::id()));
        let path = dir.join("nested/a.bin");
        super::publish(&path, b"one").unwrap();
        // An unchanged file is not rewritten, even when it is read-only (the stand-in for
        // "open in another program" that Linux can reproduce).
        let writable = std::fs::metadata(&path).unwrap().permissions();
        let mut locked = writable.clone();
        locked.set_readonly(true);
        std::fs::set_permissions(&path, locked).unwrap();
        let before = std::fs::metadata(&path).unwrap().modified().unwrap();
        super::publish(&path, b"one").unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().modified().unwrap(),
            before
        );
        std::fs::set_permissions(&path, writable).unwrap();
        super::publish(&path, b"two").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"two");
        let leftovers = std::fs::read_dir(path.parent().unwrap()).unwrap().count();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(leftovers, 1, "no temporary files remain");
    }
    #[test]
    fn identity_preserves_same_name_files_and_archive_members() {
        let a = entry("fxart/A.ROB", 0, Category::Models, Source::Model(0), "TREE");
        let b = entry("maps/A.ROB", 0, Category::Models, Source::Model(0), "TREE");
        let c = entry("fxart/A.ROB", 0, Category::Models, Source::Model(1), "TREE");
        assert_ne!(a.id, b.id);
        assert_ne!(a.id, c.id);
    }
}
