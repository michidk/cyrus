//! Background original-file decoding and selection-owned in-memory media.
use crate::ui::Viewer;
use anyhow::Result;
use bevy::{
    asset::io::{
        AssetSourceBuilder,
        memory::{Dir, MemoryAssetReader},
    },
    prelude::*,
};
use cyrus_assets::{Catalog, Preview};
use std::{
    path::Path,
    sync::{Arc, Mutex},
};
use wasm_bindgen::{JsCast, prelude::*};

enum Message {
    Progress(usize, usize, String),
    Catalog(Catalog),
    Preview(String, Preview, Vec<(String, Vec<u8>)>),
    Error(Option<String>, String),
}
#[derive(Resource, Default, Clone)]
struct Inbox(Arc<Mutex<Vec<Message>>>);
#[derive(Resource)]
struct Media {
    root: Dir,
    paths: Vec<String>,
}
pub struct BrowserWorker {
    worker: web_sys::Worker,
    source: JsValue,
    _message: Closure<dyn FnMut(web_sys::MessageEvent)>,
    _error: Closure<dyn FnMut(web_sys::ErrorEvent)>,
}
impl BrowserWorker {
    pub fn open(&self) -> Result<()> {
        let message = js_sys::Object::new();
        js_sys::Reflect::set(&message, &"kind".into(), &"open".into()).map_err(js_error)?;
        js_sys::Reflect::set(&message, &"source".into(), &self.source).map_err(js_error)?;
        self.worker.post_message(&message).map_err(js_error)
    }
    pub fn select(&self, entry: &cyrus_assets::Entry, palette: Option<&str>) -> Result<()> {
        if matches!(entry.source, cyrus_assets::Source::Archive) {
            self.send("expand", "path", &entry.path)?;
        } else {
            let message = js_sys::Object::new();
            js_sys::Reflect::set(&message, &"kind".into(), &"preview".into()).map_err(js_error)?;
            js_sys::Reflect::set(&message, &"id".into(), &entry.id.clone().into())
                .map_err(js_error)?;
            js_sys::Reflect::set(
                &message,
                &"palette".into(),
                &palette.map(JsValue::from_str).unwrap_or(JsValue::NULL),
            )
            .map_err(js_error)?;
            self.worker.post_message(&message).map_err(js_error)?;
        }
        if let Some(window) = web_sys::window() {
            let path = String::from(js_sys::encode_uri_component(&entry.path));
            let mut hash = format!("#asset={}&file={path}", entry.id);
            if let Some(palette) = palette {
                hash.push_str(&format!(
                    "&palette={}",
                    String::from(js_sys::encode_uri_component(palette))
                ));
            }
            if window.location().hash().map_err(js_error)? != hash {
                window.location().set_hash(&hash).map_err(js_error)?;
            }
        }
        Ok(())
    }
    fn resolve(&self, id: &str, path: Option<String>) -> Result<()> {
        self.send(
            "resolve",
            "selection",
            &serde_json::json!({"id":id,"path":path}).to_string(),
        )
    }
    fn send(&self, kind: &str, key: &str, value: &str) -> Result<()> {
        let message = js_sys::Object::new();
        js_sys::Reflect::set(&message, &"kind".into(), &kind.into()).map_err(js_error)?;
        js_sys::Reflect::set(&message, &key.into(), &value.into()).map_err(js_error)?;
        self.worker.post_message(&message).map_err(js_error)
    }
}
fn js_error(value: JsValue) -> anyhow::Error {
    anyhow::anyhow!("{value:?}")
}
fn string(value: &JsValue, field: &str) -> Result<String> {
    js_sys::Reflect::get(value, &field.into())
        .map_err(js_error)?
        .as_string()
        .ok_or_else(|| anyhow::anyhow!("worker message missing {field}"))
}
fn decode(value: JsValue) -> Result<Message> {
    Ok(match string(&value, "kind")?.as_str() {
        "progress" => {
            let number = |field: &str| -> Result<usize> {
                let number = js_sys::Reflect::get(&value, &field.into())
                    .map_err(js_error)?
                    .as_f64()
                    .ok_or_else(|| anyhow::anyhow!("invalid progress"))?;
                anyhow::ensure!(number.is_finite() && number >= 0., "invalid progress");
                Ok(number as usize)
            };
            Message::Progress(number("done")?, number("total")?, string(&value, "name")?)
        }
        "catalog" => {
            let catalog: Catalog = serde_json::from_str(&string(&value, "catalog")?)?;
            catalog.validate()?;
            Message::Catalog(catalog)
        }
        "preview" => {
            let preview: Preview = serde_json::from_str(&string(&value, "preview")?)?;
            preview.validate()?;
            let files = js_sys::Array::from(
                &js_sys::Reflect::get(&value, &"files".into()).map_err(js_error)?,
            );
            let mut media = Vec::new();
            for file in files {
                let file = js_sys::Array::from(&file);
                let path = file
                    .get(0)
                    .as_string()
                    .ok_or_else(|| anyhow::anyhow!("media missing path"))?;
                cyrus_assets::validate_path(&path)?;
                media.push((path, js_sys::Uint8Array::new(&file.get(1)).to_vec()));
            }
            Message::Preview(string(&value, "id")?, preview, media)
        }
        "error" => Message::Error(string(&value, "id").ok(), string(&value, "message")?),
        kind => anyhow::bail!("unknown worker message {kind}"),
    })
}

/// Registers media before AssetPlugin. Only generated in-memory files use cache://.
pub fn install(app: &mut App) -> Result<()> {
    let root = Dir::default();
    let reader_root = root.clone();
    app.register_asset_source(
        "cache",
        AssetSourceBuilder::new(move || {
            Box::new(MemoryAssetReader {
                root: reader_root.clone(),
            })
        }),
    );
    app.insert_resource(Media {
        root,
        paths: Vec::new(),
    });
    let options = web_sys::WorkerOptions::new();
    options.set_type(web_sys::WorkerType::Module);
    let worker =
        web_sys::Worker::new_with_options("./asset-worker.js", &options).map_err(js_error)?;
    let inbox = Inbox::default();
    let sink = inbox.clone();
    let message = Closure::new(move |event: web_sys::MessageEvent| {
        let decoded =
            decode(event.data()).unwrap_or_else(|error| Message::Error(None, format!("{error:#}")));
        if let Ok(mut queue) = sink.0.lock() {
            queue.push(decoded);
        }
    });
    worker.set_onmessage(Some(message.as_ref().unchecked_ref()));
    let sink = inbox.clone();
    let error = Closure::new(move |event: web_sys::ErrorEvent| {
        if let Ok(mut queue) = sink.0.lock() {
            queue.push(Message::Error(None, event.message()));
        }
    });
    worker.set_onerror(Some(error.as_ref().unchecked_ref()));
    let source =
        js_sys::Reflect::get(&js_sys::global(), &"cyrusInstallSource".into()).map_err(js_error)?;
    anyhow::ensure!(
        !source.is_undefined(),
        "select a game installation before starting the viewer"
    );
    app.insert_resource(inbox);
    app.world_mut().insert_non_send(BrowserWorker {
        worker,
        source,
        _message: message,
        _error: error,
    });
    app.add_systems(
        Update,
        (poll, follow_link).chain().before(crate::loading::poll),
    );
    Ok(())
}

fn poll(
    inbox: Res<Inbox>,
    mut media: ResMut<Media>,
    mut viewer: ResMut<Viewer>,
    mut progress: ResMut<cyrus_app::LoadingProgress>,
) {
    let Ok(mut queue) = inbox.0.lock() else {
        return;
    };
    for message in queue.drain(..) {
        let selected = viewer
            .selected
            .and_then(|index| viewer.catalog.as_ref()?.entries.get(index))
            .map(|entry| entry.id.as_str());
        match message {
            Message::Progress(done, total, name) => {
                viewer.load_status = format!("{name} ({done}/{total})");
                viewer.load_fraction = (total > 0).then(|| done as f32 / total as f32);
                progress.label = viewer.load_status.clone();
                progress.fraction = viewer.load_fraction;
            }
            Message::Catalog(catalog) => {
                viewer.set_catalog(catalog);
                viewer.loading_catalog = false;
                viewer.indexing = false;
                progress.visible = false;
            }
            Message::Preview(id, preview, files) if selected == Some(&id) => {
                for path in std::mem::take(&mut media.paths) {
                    media.root.remove_asset(Path::new(&path));
                }
                for (path, bytes) in files {
                    media.root.insert_asset(Path::new(&path), bytes);
                    media.paths.push(path);
                }
                viewer.set_preview(preview);
                viewer.preparing = false;
                viewer.scene_dirty = true;
                viewer.audio_dirty = true;
            }
            Message::Error(id, error) if id.is_none() || selected == id.as_deref() => {
                viewer.error = Some(error);
                viewer.preparing = false;
                viewer.loading_catalog = false;
                viewer.indexing = false;
                progress.visible = false;
            }
            _ => {}
        }
    }
}

fn follow_link(
    mut commands: Commands,
    assets: Res<AssetServer>,
    worker: NonSend<BrowserWorker>,
    mut last_hash: Local<String>,
    mut requested_hash: Local<String>,
    mut viewer: ResMut<Viewer>,
) {
    let Some(catalog) = &viewer.catalog else {
        return;
    };
    let hash = web_sys::window()
        .and_then(|window| window.location().hash().ok())
        .unwrap_or_default();
    if hash == *last_hash {
        return;
    }
    if let Some(selection) = hash.strip_prefix("#asset=") {
        let (selection, palette) = selection
            .split_once("&palette=")
            .map_or((selection, None), |(selection, palette)| {
                (selection, Some(palette))
            });
        let palette = match palette.map(js_sys::decode_uri_component).transpose() {
            Ok(palette) => palette.map(String::from),
            Err(error) => {
                viewer.error = Some(format!("Invalid palette link: {error:?}"));
                *last_hash = hash;
                return;
            }
        };
        let (id, path) = selection
            .split_once("&file=")
            .map_or((selection, None), |(id, path)| (id, Some(path)));
        if let Some(index) = catalog.entries.iter().position(|entry| entry.id == id) {
            *last_hash = hash;
            if viewer.selected != Some(index) || viewer.model_palette != palette {
                viewer.format = catalog.entries[index].format_label().into();
                viewer.model_palette = palette;
                viewer.search.clear();
                crate::loading::select(index, &mut viewer, &mut commands, &assets, &worker);
            }
        } else if hash != *requested_hash {
            *requested_hash = hash.clone();
            let path = path.map(js_sys::decode_uri_component).transpose();
            let result = path
                .map_err(js_error)
                .and_then(|path| worker.resolve(id, path.map(String::from)));
            viewer.indexing = result.is_ok();
            viewer.error = result.err().map(|error| format!("{error:#}"));
        } else if !viewer.indexing {
            *last_hash = hash;
        }
    } else {
        *last_hash = hash.clone();
        if hash.is_empty() {
            viewer.selected = None;
            viewer.archive = None;
            viewer.preview = None;
            viewer.preparing = false;
            viewer.scene_dirty = true;
            viewer.audio_dirty = true;
        }
    }
}
