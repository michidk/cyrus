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
}
impl ReadAssets for HttpFiles {
    fn manifest(&self) -> &Manifest {
        &self.manifest
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
        request
            .open_with_async("GET", &url, false)
            .map_err(js_error)?;
        request.set_response_type(web_sys::XmlHttpRequestResponseType::Arraybuffer);
        if partial {
            request
                .set_request_header("Range", &format!("bytes={start}-{end}"))
                .map_err(js_error)?;
        }
        request
            .send()
            .map_err(js_error)
            .with_context(|| format!("fetch original {path}"))?;
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
        let bytes = js_sys::Uint8Array::new(&request.response().map_err(js_error)?).to_vec();
        ensure!(
            bytes.len() == length,
            "{path}: expected {length} bytes, received {}",
            bytes.len()
        );
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
        let library = Library::open(HttpFiles { base, manifest }, |done, total, name| {
            let _ = progress.call3(
                &JsValue::NULL,
                &JsValue::from_f64(done as f64),
                &JsValue::from_f64(total as f64),
                &JsValue::from_str(name),
            );
        })
        .map_err(|error| bridge_error(format!("{error:#}")))?;
        Ok(Self { library })
    }
    pub fn catalog(&self) -> std::result::Result<String, JsValue> {
        serde_json::to_string(&self.library.catalog).map_err(bridge_error)
    }
    pub fn preview(&mut self, id: &str) -> std::result::Result<JsValue, JsValue> {
        let prepared = self
            .library
            .prepare(id)
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
