//! Native book-boundary framebuffer capture for complete original-slot assembly.
use bevy::{
    prelude::*,
    render::view::screenshot::{Screenshot, ScreenshotCaptured},
};
use cyrus::original_slot::capture::{CapturedFrame, CapturedImages, capture_images};
use cyrus_game::AppState;
use cyrus_world::lifecycle::{CurrentScene, PreparedWorld};
use serde_json::{Value, json};

#[derive(Resource, Default)]
pub(crate) struct OriginalImages {
    pending: Option<Entity>,
    source: Option<PreparedWorld>,
    pub(crate) images: Option<CapturedImages>,
    rejection: Option<String>,
}

impl OriginalImages {
    /// Admit and read the retained image group without recapture or filesystem writes.
    pub(crate) fn read(
        &self,
        state: AppState,
        source: &PreparedWorld,
        path: &std::path::Path,
    ) -> anyhow::Result<Value> {
        use anyhow::Context;
        use rgpre::import::savegame::SaveImageKind;
        anyhow::ensure!(state == AppState::Menu, "active book visit required");
        anyhow::ensure!(self.pending.is_none(), "native readback pending");
        let images = self
            .images
            .as_ref()
            .context("native captured images unavailable")?;
        anyhow::ensure!(
            self.source.as_ref() == Some(source),
            "prepared source changed since readback"
        );
        anyhow::ensure!(
            source.world == Some(images.world()),
            "capture world differs from prepared world"
        );
        let bytes = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
        let current = rgpre::import::savegame::parse_savegame_file(&bytes)
            .with_context(|| format!("parse {}", path.display()))?;
        let _admitted = images.admit(&current)?;
        let (screen, thumbnail) = images.records();
        Ok(
            json!({"world": images.world(), "encoding": "rgb565_le", "complete_slot_export": false,
            "screen": {"record": "SCR3", "width": SaveImageKind::Screen.width(), "height": SaveImageKind::Screen.height(), "data": screen.data},
            "thumbnail": {"record": "THMB", "width": SaveImageKind::Thumbnail.width(), "height": SaveImageKind::Thumbnail.height(), "data": thumbnail.data}}),
        )
    }

    pub(crate) fn status(&self) -> Value {
        json!({
            "pending": self.pending.is_some(),
            "world": self.images.as_ref().map(CapturedImages::world),
            "captured": self.images.is_some(),
            "rejection": self.rejection,
            "complete_slot_export": false,
        })
    }
}

#[derive(Component)]
struct CaptureSource(PreparedWorld);

pub(crate) fn register(app: &mut App) {
    app.init_resource::<OriginalImages>()
        .add_observer(receive)
        .add_systems(
            Last,
            request
                .after(cyrus_startup::GameplayCaptureRequest)
                .run_if(in_state(AppState::InGame)),
        )
        .add_systems(OnEnter(AppState::Loading), clear)
        .add_systems(OnExit(AppState::Menu), clear);
}

fn clear(mut capture: ResMut<OriginalImages>) {
    *capture = OriginalImages::default();
}

#[derive(bevy::ecs::system::SystemParam)]
struct CaptureWorld<'w> {
    prepared: Option<Res<'w, PreparedWorld>>,
    current: Option<Res<'w, CurrentScene>>,
    ready: Option<Res<'w, cyrus_render::ReadyScene>>,
}

impl CaptureWorld<'_> {
    fn identity(&self) -> anyhow::Result<&PreparedWorld> {
        let prepared = self
            .prepared
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("prepared world absent"))?;
        let world = prepared
            .world
            .ok_or_else(|| anyhow::anyhow!("resolved world absent"))?;
        anyhow::ensure!(
            self.current.as_ref().and_then(|scene| scene.world) == Some(world),
            "active world differs from prepared world {world}"
        );
        anyhow::ensure!(
            self.ready
                .as_ref()
                .is_some_and(|ready| ready.0 == prepared.animations),
            "world {world}: renderer not ready"
        );
        Ok(prepared)
    }
}

fn request(
    mut commands: Commands,
    next: Res<NextState<AppState>>,
    ending: Option<Res<cyrus_script::script::EndingEntry>>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    source: CaptureWorld,
    screenshots: Query<Entity, (With<cyrus_startup::GameplayScreenshot>, With<Screenshot>)>,
    mut capture: ResMut<OriginalImages>,
) {
    if !matches!(*next, NextState::Pending(AppState::Menu)) {
        return;
    }
    *capture = OriginalImages::default();
    let result = (|| -> anyhow::Result<_> {
        let prepared = source.identity()?;
        anyhow::ensure!(ending.is_none(), "ending book has no gameplay picture");
        anyhow::ensure!(
            windows.single().is_ok(),
            "primary window readback unavailable"
        );
        let entity = screenshots.single().map_err(|error| {
            anyhow::anyhow!("book gameplay screenshot unavailable or ambiguous: {error}")
        })?;
        Ok((prepared.clone(), entity))
    })();
    match result {
        Ok((prepared, entity)) => {
            commands.entity(entity).insert(CaptureSource(prepared));
            capture.pending = Some(entity);
        }
        Err(error) => {
            capture.rejection = Some(format!(
                "SAVEGAME.SAV world {:?} SCR3/THMB native capture: {error:#}",
                source.current.as_ref().and_then(|scene| scene.world)
            ))
        }
    }
}

fn receive(
    event: On<ScreenshotCaptured>,
    requests: Query<&CaptureSource>,
    source: CaptureWorld,
    state: Res<State<AppState>>,
    mut capture: ResMut<OriginalImages>,
) {
    if capture.pending != Some(event.entity) {
        return;
    }
    capture.pending = None;
    let result = (|| -> anyhow::Result<_> {
        let requested = &requests.get(event.entity)?.0;
        let prepared = source.identity()?;
        anyhow::ensure!(
            requested == prepared,
            "prepared source changed during readback"
        );
        anyhow::ensure!(
            matches!(state.get(), AppState::InGame | AppState::Menu),
            "book capture lifecycle ended"
        );
        anyhow::ensure!(
            matches!(
                event.image.texture_descriptor.format,
                bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb
                    | bevy::render::render_resource::TextureFormat::Bgra8UnormSrgb
            ),
            "unsupported framebuffer format {:?}: encoded RGBA8 readback required",
            event.image.texture_descriptor.format
        );
        let image = event.image.clone().try_into_dynamic()?.into_rgba8();
        let images = capture_images(
            prepared
                .world
                .ok_or_else(|| anyhow::anyhow!("capture active world absent"))?,
            Some(CapturedFrame {
                world: requested
                    .world
                    .ok_or_else(|| anyhow::anyhow!("capture requested world absent"))?,
                width: image.width(),
                height: image.height(),
                rgba: image.as_raw(),
            }),
        )?;
        Ok((images, prepared.clone()))
    })();
    match result {
        Ok((images, source)) => {
            capture.source = Some(source);
            capture.images = Some(images);
            capture.rejection = None;
        }
        Err(error) => {
            capture.images = None;
            capture.rejection = Some(format!(
                "SAVEGAME.SAV world {:?} SCR3/THMB native capture: {error:#}",
                source.current.as_ref().and_then(|scene| scene.world)
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{
        asset::RenderAssetUsages,
        ecs::system::RunSystemOnce,
        render::render_resource::{Extent3d, TextureDimension, TextureFormat},
        state::app::StatesPlugin,
    };

    fn app(world: u32) -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, StatesPlugin))
            .insert_state(AppState::InGame);
        app.insert_resource(PreparedWorld {
            requested_name: world.to_string(),
            world: Some(world),
            animations: Handle::default(),
            scripts: Handle::default(),
            player_path: "cache://player".into(),
            scene_path: "cache://scene".into(),
            environment_path: "cache://environment".into(),
        });
        app.insert_resource(CurrentScene {
            name: world.to_string(),
            world: Some(world),
        });
        app.insert_resource(cyrus_render::ReadyScene(Handle::default()));
        app.world_mut()
            .spawn((Window::default(), bevy::window::PrimaryWindow));
        register(&mut app);
        app
    }

    fn request_frame(app: &mut App) -> Entity {
        app.world_mut().spawn((
            Screenshot::primary_window(),
            cyrus_startup::GameplayScreenshot,
        ));
        app.world_mut()
            .resource_mut::<NextState<AppState>>()
            .set(AppState::Menu);
        app.world_mut().run_system_once(request).unwrap();
        app.world().resource::<OriginalImages>().pending.unwrap()
    }

    fn complete(app: &mut App, entity: Entity) {
        let image = Image::new_fill(
            Extent3d {
                width: 640,
                height: 480,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            &[255, 0, 0, 255],
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::MAIN_WORLD,
        );
        app.world_mut()
            .trigger(ScreenshotCaptured { entity, image });
    }

    #[test]
    fn actual_screenshot_event_retains_requested_world_and_packed_images() {
        for world in [1, 20] {
            let mut app = app(world);
            let entity = request_frame(&mut app);
            assert_eq!(
                app.world_mut()
                    .query::<&Screenshot>()
                    .iter(app.world())
                    .count(),
                1
            );
            complete(&mut app, entity);
            let capture = app.world().resource::<OriginalImages>();
            assert!(capture.pending.is_none());
            let images = capture.images.as_ref().unwrap();
            assert_eq!(images.world(), world);
            assert_eq!(images.records().0.data.len(), 614400);
            assert_eq!(&images.records().1.data[..2], &0xf800u16.to_le_bytes());
            assert_eq!(capture.status()["complete_slot_export"], false);
        }
    }

    #[test]
    fn retained_image_read_rejects_missing_pending_late_and_replaced_sources_before_io() {
        let mut app = app(1);
        let source = app.world().resource::<PreparedWorld>().clone();
        let path = std::path::Path::new("absent-metadata/SAVEGAME.SAV");
        let error = |app: &App, state, source: &PreparedWorld| {
            format!(
                "{:#}",
                app.world()
                    .resource::<OriginalImages>()
                    .read(state, source, path)
                    .unwrap_err()
            )
        };
        assert!(error(&app, AppState::Menu, &source).contains("unavailable"));
        let entity = request_frame(&mut app);
        assert!(error(&app, AppState::Menu, &source).contains("pending"));
        complete(&mut app, entity);
        assert!(error(&app, AppState::InGame, &source).contains("book visit"));
        let mut replacement = source.clone();
        replacement.requested_name = "replacement".into();
        assert!(error(&app, AppState::Menu, &replacement).contains("source changed"));
        assert!(error(&app, AppState::Menu, &source).contains("read absent-metadata"));
        assert!(app.world().resource::<OriginalImages>().images.is_some());
        let invalid = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(invalid.path(), b"unsupported metadata").unwrap();
        let error = app
            .world()
            .resource::<OriginalImages>()
            .read(AppState::Menu, &source, invalid.path())
            .unwrap_err();
        assert!(format!("{error:#}").contains("parse"));
        assert_eq!(
            std::fs::read(invalid.path()).unwrap(),
            b"unsupported metadata"
        );
    }

    #[test]
    fn installed_metadata_admits_actual_retained_records_only_for_matching_world() {
        let Some(install) = std::env::var_os("REDGUARD_ORIGINAL_SAVE_INSTALL") else {
            eprintln!("SKIP retained image read: REDGUARD_ORIGINAL_SAVE_INSTALL required");
            return;
        };
        assert!(
            std::env::var_os("REDGUARD_DIR").is_some(),
            "linked install required"
        );
        let mut worlds = std::collections::BTreeSet::new();
        for entry in std::fs::read_dir(std::path::Path::new(&install).join("SAVEGAME")).unwrap() {
            let path = entry.unwrap().path().join("SAVEGAME.SAV");
            let bytes = std::fs::read(&path).unwrap();
            let saved = rgpre::import::savegame::parse_savegame_file(&bytes).unwrap();
            let world = saved.metadata.world_id;
            if ![1, 20].contains(&world) {
                continue;
            }
            let mut captured = app(world);
            let entity = request_frame(&mut captured);
            complete(&mut captured, entity);
            let source = captured.world().resource::<PreparedWorld>();
            let capture = captured.world().resource::<OriginalImages>();
            let read = capture.read(AppState::Menu, source, &path).unwrap();
            assert_eq!(read["world"], world);
            assert_eq!(read["encoding"], "rgb565_le");
            assert_eq!(read["screen"]["data"].as_array().unwrap().len(), 614400);
            assert_eq!(read["thumbnail"]["data"].as_array().unwrap().len(), 98304);
            assert_eq!(read["screen"]["data"][0], 0);
            assert_eq!(read["screen"]["data"][1], 248);
            assert_eq!(read["complete_slot_export"], false);
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
            let mut wrong = app(if world == 1 { 20 } else { 1 });
            let entity = request_frame(&mut wrong);
            complete(&mut wrong, entity);
            let error = wrong
                .world()
                .resource::<OriginalImages>()
                .read(
                    AppState::Menu,
                    wrong.world().resource::<PreparedWorld>(),
                    &path,
                )
                .unwrap_err();
            assert!(format!("{error:#}").contains("capture world"));
            worlds.insert(world);
        }
        assert_eq!(
            worlds,
            std::collections::BTreeSet::from([1, 20]),
            "both authentic worlds required"
        );
    }

    #[test]
    fn stale_callback_cannot_publish_after_source_replacement_or_book_exit() {
        for leave in [false, true] {
            let mut app = app(1);
            let entity = request_frame(&mut app);
            if leave {
                app.world_mut().run_system_once(clear).unwrap();
            } else {
                app.world_mut()
                    .resource_mut::<PreparedWorld>()
                    .requested_name = "replacement".into();
            }
            complete(&mut app, entity);
            assert!(app.world().resource::<OriginalImages>().images.is_none());
            if !leave {
                assert!(
                    app.world()
                        .resource::<OriginalImages>()
                        .rejection
                        .as_ref()
                        .unwrap()
                        .contains("source changed")
                );
            }
        }
    }

    #[test]
    fn wrong_world_and_unsupported_readback_cannot_become_saved_images() {
        for wrong_world in [false, true] {
            let mut app = app(20);
            let entity = request_frame(&mut app);
            if wrong_world {
                app.world_mut().resource_mut::<CurrentScene>().world = Some(1);
                complete(&mut app, entity);
            } else {
                let image = Image::new_fill(
                    Extent3d {
                        width: 640,
                        height: 480,
                        depth_or_array_layers: 1,
                    },
                    TextureDimension::D2,
                    &[255, 0, 0, 255],
                    TextureFormat::Rgba8Unorm,
                    RenderAssetUsages::MAIN_WORLD,
                );
                app.world_mut()
                    .trigger(ScreenshotCaptured { entity, image });
            }
            let capture = app.world().resource::<OriginalImages>();
            assert!(capture.images.is_none());
            let error = capture.rejection.as_ref().unwrap();
            assert!(
                error.contains(if wrong_world {
                    "active world differs"
                } else {
                    "unsupported framebuffer format"
                }),
                "{error}"
            );
        }
    }

    #[test]
    fn missing_headless_window_and_unready_world_are_explicit_rejections() {
        for missing_window in [false, true] {
            let mut app = app(20);
            if missing_window {
                let entity = app
                    .world_mut()
                    .query_filtered::<Entity, With<bevy::window::PrimaryWindow>>()
                    .single(app.world())
                    .unwrap();
                app.world_mut().despawn(entity);
            } else {
                app.world_mut()
                    .remove_resource::<cyrus_render::ReadyScene>();
            }
            app.world_mut()
                .resource_mut::<NextState<AppState>>()
                .set(AppState::Menu);
            app.world_mut().run_system_once(request).unwrap();
            let capture = app.world().resource::<OriginalImages>();
            assert!(capture.pending.is_none() && capture.images.is_none());
            let error = capture.rejection.as_ref().unwrap();
            assert!(
                error.contains(if missing_window {
                    "readback unavailable"
                } else {
                    "renderer not ready"
                }),
                "{error}"
            );
        }
    }
}
