//! Worker-only WASM bridge. HTTP supplies original bytes; shared Rust parsers decode them.
#![cfg(target_arch = "wasm32")]
use anyhow::{Context, Result, ensure};
use cyrus_assets::{
    runtime::Library,
    source::{Manifest, ReadAssets},
};
use wasm_bindgen::prelude::*;

struct HttpFiles {
    base: String,
    manifest: Manifest,
    progress: js_sys::Function,
    reporting: bool,
}
impl ReadAssets for HttpFiles {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }
    fn progress(&self, done: usize, total: usize, label: &str) {
        if !self.reporting {
            return;
        }
        let _ = self.progress.call3(
            &JsValue::NULL,
            &JsValue::from_f64(done as f64),
            &JsValue::from_f64(total as f64),
            &JsValue::from_str(label),
        );
    }
    fn read_range(&self, path: &str, start: u64, length: usize) -> Result<Vec<u8>> {
        let file = self
            .manifest
            .find(path)
            .with_context(|| format!("unknown original file {path}"))?;
        ensure!(
            start <= file.bytes && length as u64 <= file.bytes - start,
            "{path}: requested range is outside the original file"
        );
        if length == 0 {
            return Ok(Vec::new());
        }
        let encoded = file
            .path
            .split('/')
            .map(|part| String::from(js_sys::encode_uri_component(part)))
            .collect::<Vec<_>>()
            .join("/");
        self.progress(0, length, &format!("Reading {path}"));
        const CHUNK: usize = 1024 * 1024;
        let mut bytes = vec![0; length];
        for offset in (0..length).step_by(CHUNK) {
            let start = start + offset as u64;
            let length = (length - offset).min(CHUNK);
            let mut url = format!(
                "{}/{encoded}?v={}",
                self.base.trim_end_matches('/'),
                file.digest
            );
            let partial = start != 0 || length as u64 != file.bytes;
            let end = start + length as u64 - 1;
            if partial {
                url.push_str(&format!("&range={start}-{end}"));
            }
            let request = web_sys::XmlHttpRequest::new().map_err(js_error)?;
            let fetch = || -> Result<()> {
                request
                    .open_with_async("GET", &url, false)
                    .map_err(js_error)?;
                request.set_timeout(30_000);
                request.set_response_type(web_sys::XmlHttpRequestResponseType::Arraybuffer);
                if partial {
                    request
                        .set_request_header("Range", &format!("bytes={start}-{end}"))
                        .map_err(js_error)?;
                }
                request.send().map_err(js_error)
            };
            // Digest-addressed original reads are safe to retry after a transport failure.
            fetch().or_else(|_| fetch()).with_context(|| {
                format!("fetch original {path}, bytes {start}-{end} failed after retry (30-second timeout per attempt)")
            })?;
            let status = request.status().map_err(js_error)?;
            ensure!(matches!(status, 200 | 206), "{path}: HTTP {status}");
            if partial {
                let expected = format!("bytes {start}-{end}/{}", file.bytes);
                ensure!(
                    request
                        .get_response_header("Content-Range")
                        .map_err(js_error)?
                        .as_deref()
                        == Some(&expected),
                    "{path}: server must support byte-range requests"
                );
            }
            let chunk = js_sys::Uint8Array::new(&request.response().map_err(js_error)?);
            ensure!(
                chunk.length() as usize == length,
                "{path}: expected {length} bytes, received {}",
                chunk.length()
            );
            chunk.copy_to(&mut bytes[offset..offset + length]);
            self.progress(offset + length, bytes.len(), &format!("Reading {path}"));
        }
        Ok(bytes)
    }
}
fn js_error(value: JsValue) -> anyhow::Error {
    anyhow::anyhow!("{value:?}")
}
fn bridge_error(error: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&error.to_string())
}

#[wasm_bindgen]
pub struct AssetLibrary {
    library: Library<HttpFiles>,
}
#[wasm_bindgen]
impl AssetLibrary {
    #[wasm_bindgen(constructor)]
    pub fn new(
        base: String,
        manifest: &str,
        progress: js_sys::Function,
    ) -> std::result::Result<AssetLibrary, JsValue> {
        let manifest: Manifest = serde_json::from_str(manifest).map_err(bridge_error)?;
        let mut library = Library::open(
            HttpFiles {
                base,
                manifest,
                progress: progress.clone(),
                reporting: false,
            },
            |done, total, name| {
                let _ = progress.call3(
                    &JsValue::NULL,
                    &JsValue::from_f64(done as f64),
                    &JsValue::from_f64(total as f64),
                    &JsValue::from_str(name),
                );
            },
        )
        .map_err(|error| bridge_error(format!("{error:#}")))?;
        library.source.reporting = true;
        Ok(Self { library })
    }
    pub fn catalog(&self) -> std::result::Result<String, JsValue> {
        serde_json::to_string(&self.library.catalog).map_err(bridge_error)
    }
    pub fn expand(&mut self, path: &str) -> std::result::Result<String, JsValue> {
        self.library
            .expand(path)
            .map_err(|error| bridge_error(format!("{path}: {error:#}")))?;
        self.catalog()
    }
    pub fn resolve(
        &mut self,
        id: &str,
        path: Option<String>,
    ) -> std::result::Result<String, JsValue> {
        self.library
            .resolve(id, path.as_deref())
            .map_err(|error| bridge_error(format!("{error:#}")))?;
        self.catalog()
    }
    pub fn preview(
        &mut self,
        id: &str,
        palette: Option<String>,
    ) -> std::result::Result<JsValue, JsValue> {
        let prepared = self
            .library
            .prepare(id, palette.as_deref())
            .map_err(|error| bridge_error(format!("{error:#}")))?;
        let message = js_sys::Object::new();
        js_sys::Reflect::set(
            &message,
            &"preview".into(),
            &serde_json::to_string(&prepared.preview)
                .map_err(bridge_error)?
                .into(),
        )?;
        let files = js_sys::Array::new();
        for (path, bytes) in prepared.files {
            let file = js_sys::Array::new();
            file.push(&path.into());
            file.push(&js_sys::Uint8Array::from(bytes.as_slice()));
            files.push(&file);
        }
        js_sys::Reflect::set(&message, &"files".into(), &files)?;
        Ok(message.into())
    }
}
