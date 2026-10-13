//! Read-only browser File handles with bounded reads and content fingerprints.
use anyhow::{Context, Result, ensure};
use cyrus_assets::source::{FileInfo, Manifest};
use std::collections::BTreeMap;
use wasm_bindgen::{JsCast, JsValue};

use super::js_error;

pub(super) struct Files {
    files: BTreeMap<String, web_sys::File>,
    manifest: Manifest,
    reader: web_sys::FileReaderSync,
}

impl Files {
    pub(super) fn new(value: JsValue, progress: &js_sys::Function) -> Result<Self> {
        let map = value
            .dyn_into::<js_sys::Map>()
            .map_err(|_| anyhow::anyhow!("expected a local file map or install URL"))?;
        let mut files = BTreeMap::new();
        for entry in map.entries() {
            let entry = js_sys::Array::from(&entry.map_err(js_error)?);
            let path = entry
                .get(0)
                .as_string()
                .context("local file path must be text")?;
            cyrus_assets::validate_source_path(&path)?;
            let file = entry
                .get(1)
                .dyn_into::<web_sys::File>()
                .map_err(|_| anyhow::anyhow!("{path}: expected a browser File"))?;
            ensure!(
                file.size().is_finite() && file.size() <= 9_007_199_254_740_991.,
                "{path}: invalid file size"
            );
            files.insert(path, file);
        }
        let reader = web_sys::FileReaderSync::new().map_err(js_error)?;
        let mut source = Self {
            files,
            reader,
            manifest: Manifest { files: Vec::new() },
        };
        let total: f64 = source.files.values().map(|file| file.size()).sum();
        let mut done = 0_f64;
        for (path, file) in &source.files {
            let size = file.size() as u64;
            let mut hash = blake3::Hasher::new();
            for start in (0..size).step_by(1024 * 1024) {
                let length = (size - start).min(1024 * 1024) as usize;
                hash.update(&source.read_range(path, start, length)?);
                done += length as f64;
                let _ = progress.call3(
                    &JsValue::NULL,
                    &done.into(),
                    &total.into(),
                    &format!("Checking local files: {path}").into(),
                );
            }
            source.manifest.files.push(FileInfo {
                path: path.clone(),
                bytes: size,
                digest: hash.finalize().to_hex().to_string(),
            });
        }
        source.manifest.validate()?;
        Ok(source)
    }

    pub(super) fn manifest(&self) -> Manifest {
        self.manifest.clone()
    }

    pub(super) fn read_range(&self, path: &str, start: u64, length: usize) -> Result<Vec<u8>> {
        let read = || -> Result<Vec<u8>> {
            let file = self.files.get(path).context("selected file is missing")?;
            let size = file.size() as u64;
            ensure!(
                start <= size && length as u64 <= size - start,
                "range is outside selected file"
            );
            let mut bytes = Vec::with_capacity(length);
            for offset in (0..length).step_by(1024 * 1024) {
                let count = (length - offset).min(1024 * 1024);
                let start = start as f64 + offset as f64;
                let blob = file
                    .slice_with_f64_and_f64(start, start + count as f64)
                    .map_err(js_error)?;
                let chunk = js_sys::Uint8Array::new(
                    &self
                        .reader
                        .read_as_array_buffer(&blob)
                        .map_err(js_error)?
                        .into(),
                );
                ensure!(
                    chunk.length() as usize == count,
                    "expected {count} bytes, received {}",
                    chunk.length()
                );
                bytes.extend(chunk.to_vec());
            }
            Ok(bytes)
        };
        read().with_context(|| {
            format!("read local {path}; reselect the folder if files moved or changed")
        })
    }
}
