//! Runtime title labels share one background and one mutable presentation image.
use super::Playback;
use bevy::{
    asset::{AssetLoader, LoadContext, io::Reader},
    prelude::*,
};
use cyrus_assets::title::TitleLabels;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum Status {
    #[default]
    Loading,
    #[cfg(any(not(target_arch = "wasm32"), test))]
    Preparing,
    Menu,
    #[cfg(any(target_arch = "wasm32", test))]
    Sound,
}

impl Status {
    fn text(self) -> &'static str {
        match self {
            Self::Loading => "Loading...",
            #[cfg(any(not(target_arch = "wasm32"), test))]
            Self::Preparing => "Preparing menu and HUD...",
            Self::Menu => "Loading menu...",
            #[cfg(any(target_arch = "wasm32", test))]
            Self::Sound => "Loading sound...",
        }
    }
}

#[derive(Default, TypePath)]
struct Loader;
impl AssetLoader for Loader {
    type Asset = TitleLabels;
    type Settings = ();
    type Error = anyhow::Error;
    async fn load(
        &self,
        reader: &mut dyn Reader,
        _: &(),
        _: &mut LoadContext<'_>,
    ) -> anyhow::Result<TitleLabels> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        Ok(serde_json::from_slice(&bytes)?)
    }
    fn extensions(&self) -> &[&str] {
        &["title"]
    }
}

pub(super) fn register(app: &mut App) {
    app.init_asset::<TitleLabels>()
        .init_asset_loader::<Loader>();
}

#[derive(Resource)]
pub(super) struct Drawing {
    background: Handle<Image>,
    labels: Handle<TitleLabels>,
    rendered: Handle<Image>,
    displayed: Option<Status>,
}

impl Drawing {
    pub(super) fn new(assets: &AssetServer) -> Self {
        Self {
            background: assets.load("cache://startup/title.png"),
            labels: assets.load("cache://startup/labels.title"),
            rendered: default(),
            displayed: None,
        }
    }
}

pub(super) fn present(
    drawing: Option<ResMut<Drawing>>,
    mut playback: ResMut<Playback>,
    labels: Res<Assets<TitleLabels>>,
    mut images: ResMut<Assets<Image>>,
    assets: Res<AssetServer>,
    mut startup: ResMut<super::StartupPresentation>,
) {
    let Some(mut drawing) = drawing else {
        return;
    };
    if drawing.displayed == Some(playback.title_status) {
        return;
    }
    for id in [
        drawing.background.id().untyped(),
        drawing.labels.id().untyped(),
    ] {
        if let Some(bevy::asset::LoadState::Failed(error)) = assets.get_load_state(id) {
            startup.error = Some(format!("Startup title labels unavailable: {error}"));
            return;
        }
    }
    let (Some(background), Some(labels)) =
        (images.get(&drawing.background), labels.get(&drawing.labels))
    else {
        return;
    };
    let mut image = background.clone();
    let size = image.size();
    let Some(bytes) = image.data.as_mut() else {
        return;
    };
    labels.draw(
        playback.title_status.text(),
        size.to_array(),
        |x, y, color| {
            let offset = ((y * size.x + x) * 4) as usize;
            bytes[offset..offset + 4].copy_from_slice(&color);
        },
    );
    if drawing.rendered == Handle::default() {
        drawing.rendered = images.add(image);
    } else {
        images
            .insert(drawing.rendered.id(), image)
            .expect("retained title image handle");
    }
    playback.title = drawing.rendered.clone();
    drawing.displayed = Some(playback.title_status);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_changes_repaint_one_image_without_retaining_old_lettering() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()));
        app.init_asset::<Image>();
        register(&mut app);
        let background = Image::new_fill(
            bevy::render::render_resource::Extent3d {
                width: 1820,
                height: 1024,
                depth_or_array_layers: 1,
            },
            bevy::render::render_resource::TextureDimension::D2,
            &[1, 2, 3, 255],
            bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
            default(),
        );
        let source = background.data.clone();
        let background_handle = app
            .world_mut()
            .resource_mut::<Assets<Image>>()
            .add(background);
        let labels = TitleLabels {
            font: cyrus_text::Font {
                start: 32,
                line_height: 1,
                glyphs: vec![
                    cyrus_text::Glyph {
                        enabled: true,
                        width: 1,
                        height: 1,
                        offset: [0, 0],
                        pixels: vec![1],
                    };
                    96
                ],
                palette: vec![],
            },
            version_colour: [11, 22, 33],
            status_colour: [44, 55, 66],
        };
        let labels_handle = app
            .world_mut()
            .resource_mut::<Assets<TitleLabels>>()
            .add(labels);
        app.insert_resource(Drawing {
            background: background_handle.clone(),
            labels: labels_handle.clone(),
            rendered: default(),
            displayed: None,
        });
        app.insert_resource(Playback {
            count: 0,
            live: None,
            title: default(),
            title_status: default(),
            title_fills_window: true,
            elapsed: 0.0,
            failed: false,
        });
        app.init_resource::<super::super::StartupPresentation>();
        app.add_systems(Update, present);
        let mut handle = None;
        for (status, text) in [
            (Status::Loading, "Loading..."),
            (Status::Preparing, "Preparing menu and HUD..."),
            (Status::Menu, "Loading menu..."),
            (Status::Sound, "Loading sound..."),
            (Status::Loading, "Loading..."),
        ] {
            app.world_mut().resource_mut::<Playback>().title_status = status;
            app.update();
            let actual_handle = app.world().resource::<Playback>().title.clone();
            if let Some(handle) = &handle {
                assert_eq!(&actual_handle, handle);
            }
            handle = Some(actual_handle.clone());
            let images = app.world().resource::<Assets<Image>>();
            assert_eq!(images.len(), 2, "one background and one runtime image");
            let original = images.get(&background_handle).unwrap();
            assert_eq!(original.data, source, "background stays unlabelled");
            let mut expected = original.data.clone().unwrap();
            app.world()
                .resource::<Assets<TitleLabels>>()
                .get(&labels_handle)
                .unwrap()
                .draw(text, [1820, 1024], |x, y, color| {
                    let offset = ((y * 1820 + x) * 4) as usize;
                    expected[offset..offset + 4].copy_from_slice(&color);
                });
            assert_eq!(
                images.get(&actual_handle).unwrap().data.as_ref().unwrap(),
                &expected
            );
        }
    }
    #[test]
    fn installed_runtime_title_images_match_prior_exports() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../redguard");
        let Ok(install) = cyrus_assets::install::GameInstall::open(&root) else {
            eprintln!("skipped: no install (runtime title labels)");
            return;
        };
        let source = cyrus_assets::native::Files::open(&install).unwrap();
        let prepared = cyrus_assets::game::prepare(&source, "startup/labels.title").unwrap();
        let labels =
            serde_json::from_slice::<TitleLabels>(&prepared["startup/labels.title"]).unwrap();
        let background = image::load_from_memory(&prepared["startup/title.png"])
            .unwrap()
            .to_rgba8();
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()));
        app.init_asset::<Image>();
        register(&mut app);
        let background_handle = app
            .world_mut()
            .resource_mut::<Assets<Image>>()
            .add(Image::new(
                bevy::render::render_resource::Extent3d {
                    width: background.width(),
                    height: background.height(),
                    depth_or_array_layers: 1,
                },
                bevy::render::render_resource::TextureDimension::D2,
                background.into_raw(),
                bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
                default(),
            ));
        let labels_handle = app
            .world_mut()
            .resource_mut::<Assets<TitleLabels>>()
            .add(labels);
        app.insert_resource(Drawing {
            background: background_handle,
            labels: labels_handle,
            rendered: default(),
            displayed: None,
        });
        app.insert_resource(Playback {
            count: 0,
            live: None,
            title: default(),
            title_status: default(),
            title_fills_window: true,
            elapsed: 0.0,
            failed: false,
        });
        app.init_resource::<super::super::StartupPresentation>();
        app.add_systems(Update, present);
        for (status, name) in [
            (Status::Loading, "title.png"),
            (Status::Preparing, "title-preparing.png"),
            (Status::Menu, "title-menu.png"),
            (Status::Sound, "title-sound.png"),
        ] {
            app.world_mut().resource_mut::<Playback>().title_status = status;
            app.update();
            let handle = &app.world().resource::<Playback>().title;
            let images = app.world().resource::<Assets<Image>>();
            let actual = images.get(handle).expect("production title was presented");
            let pixels = image::RgbaImage::from_raw(
                actual.width(),
                actual.height(),
                actual.data.clone().unwrap(),
            )
            .unwrap();
            if let Some(directory) = std::env::var_os("CYRUS_TITLE_REFERENCE_DIR") {
                let expected = image::open(std::path::Path::new(&directory).join(name))
                    .unwrap()
                    .to_rgba8();
                assert_eq!(pixels, expected, "runtime {name} differs from prior export");
            }
            if let Some(directory) = std::env::var_os("CYRUS_TITLE_CAPTURE_DIR") {
                std::fs::create_dir_all(&directory).unwrap();
                pixels
                    .save(std::path::Path::new(&directory).join(name))
                    .unwrap();
            }
            assert_eq!(images.len(), 2);
        }
    }
}
