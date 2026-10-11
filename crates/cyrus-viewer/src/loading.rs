//! Asynchronous catalog and preview loading with selection-owned results.
use crate::ui::Viewer;
use bevy::{
    asset::{AssetLoader, LoadContext, LoadState, io::Reader},
    prelude::*,
};
use cyrus_assets::{Catalog, Preview};

#[derive(Asset, TypePath)]
pub struct Blob(pub Vec<u8>);
#[derive(Default, TypePath)]
pub struct BlobLoader;
impl AssetLoader for BlobLoader {
    type Asset = Blob;
    type Settings = ();
    type Error = anyhow::Error;
    async fn load(
        &self,
        reader: &mut dyn Reader,
        _: &(),
        _: &mut LoadContext<'_>,
    ) -> Result<Blob, Self::Error> {
        use bevy::tasks::futures_lite::AsyncReadExt;
        const LIMIT: u64 = 512 * 1024 * 1024;
        let mut bytes = Vec::new();
        reader.take(LIMIT + 1).read_to_end(&mut bytes).await?;
        anyhow::ensure!(
            bytes.len() as u64 <= LIMIT,
            "preview exceeds 512 MiB loading limit"
        );
        Ok(Blob(bytes))
    }
    fn extensions(&self) -> &[&str] {
        &["json", "preview", "smk"]
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Resource)]
pub struct NativeSource(pub cyrus_assets::install::GameInstall);
#[cfg(not(target_arch = "wasm32"))]
#[derive(Resource)]
pub struct NativeLibrary(pub std::sync::Arc<cyrus_assets::native::Library>);
#[cfg(not(target_arch = "wasm32"))]
#[derive(Resource, Default, Clone)]
pub struct ScanProgress(std::sync::Arc<std::sync::Mutex<Option<(usize, usize, String)>>>);

#[cfg(not(target_arch = "wasm32"))]
#[derive(Resource)]
pub struct Preparation(pub bevy::tasks::Task<Result<Prepared, String>>);
#[cfg(not(target_arch = "wasm32"))]
pub enum Prepared {
    Catalog(Box<cyrus_assets::native::Library>),
    Expanded(Catalog),
    Preview(String, String),
}

pub fn start(
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut viewer: ResMut<Viewer>,
    mut progress: ResMut<cyrus_app::LoadingProgress>,
    #[cfg(not(target_arch = "wasm32"))] source: Res<NativeSource>,
    #[cfg(target_arch = "wasm32")] worker: NonSend<crate::browser::BrowserWorker>,
) {
    progress.visible = true;
    progress.label = if cfg!(target_arch = "wasm32") {
        "Loading asset library…"
    } else {
        "Reading original assets…"
    }
    .into();
    #[cfg(not(target_arch = "wasm32"))]
    {
        let install = source.0.clone();
        let scan = ScanProgress::default();
        commands.insert_resource(scan.clone());
        commands.insert_resource(Preparation(bevy::tasks::IoTaskPool::get().spawn(
            async move {
                cyrus_assets::native::Library::open(
                    install,
                    cyrus_assets::CACHE_DIR.into(),
                    |done, total, name| {
                        if let Ok(mut progress) = scan.0.lock() {
                            *progress = Some((done, total, name.into()));
                        }
                    },
                )
                .map(|library| Prepared::Catalog(Box::new(library)))
                .map_err(|e| format!("{e:#}"))
            },
        )));
    }
    #[cfg(target_arch = "wasm32")]
    {
        viewer.loading_catalog = true;
        if let Err(error) = worker.open() {
            viewer.error = Some(format!("{error:#}"));
            progress.visible = false;
        }
    }
    let _ = (&mut commands, &assets, &mut viewer);
}

pub fn select(
    index: usize,
    viewer: &mut Viewer,
    commands: &mut Commands,
    assets: &AssetServer,
    #[cfg(not(target_arch = "wasm32"))] library: Option<&NativeLibrary>,
    #[cfg(target_arch = "wasm32")] worker: &crate::browser::BrowserWorker,
) {
    let Some(catalog) = &viewer.catalog else {
        return;
    };
    let Some(entry) = catalog.entries.get(index) else {
        return;
    };
    let archive = matches!(entry.source, cyrus_assets::Source::Archive);
    if archive
        || matches!(
            entry.source,
            cyrus_assets::Source::Image(_)
                | cyrus_assets::Source::Model(_)
                | cyrus_assets::Source::Effect(_)
                | cyrus_assets::Source::Speech(_)
        )
    {
        viewer.archive = Some(entry.path.clone());
    } else {
        viewer.archive = None;
    }
    if archive {
        viewer.search.clear();
    }
    viewer.selected = Some(index);
    viewer.preview = None;
    viewer.pending = None;
    viewer.textures.clear();
    viewer.font_textures.clear();
    viewer.texture_key = None;
    viewer.movie = None;
    viewer.movie_blob = None;
    viewer.error = None;
    viewer.frame = 0;
    viewer.palette = None;
    viewer.playing = false;
    viewer.position = 0.;
    viewer.scene_dirty = true;
    viewer.audio_dirty = true;
    viewer.zoom = 1.;
    viewer.needs_frame = false;
    viewer.decoding = false;
    viewer.seek_dirty = false;
    viewer.preparing = !archive;
    viewer.indexing = archive;
    #[cfg(not(target_arch = "wasm32"))]
    let path = catalog.preview_path(entry);
    #[cfg(target_arch = "wasm32")]
    {
        if let Err(error) = worker.select(entry) {
            viewer.error = Some(format!("{error:#}"));
            viewer.preparing = false;
            viewer.indexing = false;
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(library) = library {
        let library = library.0.clone();
        let id = entry.id.clone();
        let source = entry.path.clone();
        commands.insert_resource(Preparation(bevy::tasks::IoTaskPool::get().spawn(
            async move {
                if archive {
                    return library
                        .expand(&source)
                        .map(Prepared::Expanded)
                        .map_err(|e| format!("{e:#}"));
                }
                library
                    .prepare(&id)
                    .map(|_| Prepared::Preview(id, path))
                    .map_err(|e| format!("{e:#}"))
            },
        )));
    }
    let _ = (assets, commands);
}

pub fn poll(
    mut commands: Commands,
    assets: Res<AssetServer>,
    blobs: Res<Assets<Blob>>,
    mut viewer: ResMut<Viewer>,
    mut progress: ResMut<cyrus_app::LoadingProgress>,
    #[cfg(not(target_arch = "wasm32"))] task: Option<ResMut<Preparation>>,
    #[cfg(not(target_arch = "wasm32"))] scan: Option<Res<ScanProgress>>,
) {
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(scan) = scan
        && let Ok(mut latest) = scan.0.lock()
        && let Some((done, total, name)) = latest.take()
    {
        progress.fraction = Some(done as f32 / total.max(1) as f32);
        progress.label = format!("{name} ({done}/{total})");
    }
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(mut task) = task
        && let Some(result) = bevy::tasks::futures::check_ready(&mut task.0)
    {
        commands.remove_resource::<Preparation>();
        match result {
            Ok(Prepared::Catalog(library)) => {
                viewer.set_catalog(library.catalog.clone());
                commands.insert_resource(NativeLibrary(std::sync::Arc::from(library)));
                progress.visible = false;
            }
            Ok(Prepared::Expanded(catalog)) => {
                viewer.set_catalog(catalog);
                viewer.indexing = false;
            }
            Ok(Prepared::Preview(id, path)) => {
                viewer.preparing = false;
                if viewer
                    .selected
                    .and_then(|i| viewer.catalog.as_ref()?.entries.get(i))
                    .is_some_and(|e| e.id == id)
                {
                    viewer.pending = Some(assets.load(format!("cache://{path}")));
                }
            }
            Err(error) => {
                viewer.preparing = false;
                viewer.error = Some(error);
                viewer.indexing = false;
                progress.visible = false;
            }
        }
    }
    if let Some(handle) = viewer.pending.as_ref() {
        if let Some(blob) = blobs.get(handle) {
            let result = if viewer.loading_catalog {
                serde_json::from_slice::<Catalog>(&blob.0)
                    .map_err(anyhow::Error::from)
                    .and_then(|c| {
                        c.validate()?;
                        viewer.set_catalog(c);
                        Ok(())
                    })
            } else {
                serde_json::from_slice::<Preview>(&blob.0)
                    .map_err(anyhow::Error::from)
                    .and_then(|p| {
                        p.validate()?;
                        viewer.preview = Some(p);
                        viewer.scene_dirty = true;
                        viewer.audio_dirty = true;
                        Ok(())
                    })
            };
            if let Err(e) = result {
                viewer.error = Some(format!("{e:#}"));
            }
            viewer.preparing = false;
            viewer.pending = None;
            viewer.loading_catalog = false;
            progress.visible = false;
        } else if let Some(LoadState::Failed(error)) = assets.get_load_state(handle.id()) {
            viewer.error = Some(format!("Asset load failed: {error}"));
            viewer.preparing = false;
            viewer.pending = None;
            progress.visible = false;
        }
    }
    let _ = &mut commands;
}
