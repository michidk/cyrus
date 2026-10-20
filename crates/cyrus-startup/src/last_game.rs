//! Live death confirmation: captured world backdrop, installed dialog and host load requests.
use super::{ConfirmationChoice, ConfirmationInput, LastGameConfirmation, StartupContent};
use bevy::{
    prelude::*,
    render::view::window::screenshot::{Screenshot, ScreenshotCaptured},
};
use cyrus_assets::menu::MenuAsset;
use cyrus_game::script::{
    DeathBookEntry, DeathDispatch, LastGameEntry, LastSaveSelection, ScriptActors,
};
use serde::Serialize;

#[derive(Default)]
enum LoadPhase {
    #[default]
    Idle,
    Queued(LastSaveSelection),
    Loading,
    Failed(String),
}

/// Installed by a host that can restore retained save slots. One request remains
/// outstanding through preparation and atomic session publication. Success must
/// accompany a committed session; the dialog never substitutes a fresh world.
#[derive(Resource, Default)]
pub struct LastGameLoader {
    phase: LoadPhase,
}

impl LastGameLoader {
    fn start(&mut self, save: LastSaveSelection) -> anyhow::Result<()> {
        anyhow::ensure!(
            matches!(self.phase, LoadPhase::Idle),
            "last-game loader already busy or result unread"
        );
        self.phase = LoadPhase::Queued(save);
        Ok(())
    }

    /// Admit the queued selection once; retain ownership until `finish`.
    pub fn take_request(&mut self) -> Option<LastSaveSelection> {
        match std::mem::take(&mut self.phase) {
            LoadPhase::Queued(save) => {
                self.phase = LoadPhase::Loading;
                Some(save)
            }
            phase => {
                self.phase = phase;
                None
            }
        }
    }

    /// Report committed restoration or a contextual failure to the dialog.
    /// Preparation alone must never report success.
    pub fn finish(&mut self, result: Result<(), String>) -> anyhow::Result<()> {
        anyhow::ensure!(
            matches!(self.phase, LoadPhase::Loading),
            "last-game loader has no admitted request"
        );
        self.phase = match result {
            Ok(()) => LoadPhase::Idle,
            Err(error) => LoadPhase::Failed(error),
        };
        Ok(())
    }

    fn take_error(&mut self) -> Option<String> {
        match std::mem::take(&mut self.phase) {
            LoadPhase::Failed(error) => Some(error),
            phase => {
                self.phase = phase;
                None
            }
        }
    }
}

#[derive(Resource)]
pub(super) struct CapturedFrame(pub Result<Image, String>);

#[derive(Component)]
pub(super) struct CapturedBackground;

#[derive(Serialize)]
pub(super) struct Presentation {
    pub slot: u16,
    pub selected: ConfirmationChoice,
    pub ready: bool,
    pub error: Option<String>,
    #[serde(skip)]
    save: LastSaveSelection,
    #[serde(skip)]
    retained_context: u8,
    #[serde(skip)]
    override_group: i32,
    #[serde(skip)]
    core: Option<LastGameConfirmation>,
    #[serde(skip)]
    graphics: bool,
    #[serde(skip)]
    returned: bool,
    #[serde(skip)]
    backdrop: Option<image::RgbaImage>,
    #[serde(skip)]
    dimensions: (u32, u32),
}

impl Presentation {
    pub(super) fn new(entry: &LastGameEntry) -> Self {
        Self {
            slot: entry.save.slot,
            selected: ConfirmationChoice::Yes,
            ready: false,
            error: None,
            save: entry.save.clone(),
            retained_context: entry.retained_book_context,
            override_group: entry.override_group,
            core: None,
            graphics: false,
            returned: false,
            backdrop: None,
            dimensions: (0, 0),
        }
    }
}

pub(super) fn request_capture(
    commands: &mut Commands,
    windows: &Query<&Window, With<bevy::window::PrimaryWindow>>,
) {
    if windows.single().is_err() {
        commands.insert_resource(CapturedFrame(Err(
            "last-game confirmation: primary window capture unavailable".into(),
        )));
        return;
    }
    commands.spawn(Screenshot::primary_window()).observe(
        |event: On<ScreenshotCaptured>,
         mut commands: Commands,
         entry: Option<Res<LastGameEntry>>| {
            if entry.is_some() {
                let mut image = event.image.clone();
                image.asset_usage = bevy::asset::RenderAssetUsages::default();
                commands.insert_resource(CapturedFrame(Ok(image)));
            }
        },
    );
}

/// Original description formatting searches spaces from character 13, then skips
/// 14 characters after each replacement; the final character remains untouched.
fn prompt(description: &str) -> String {
    let mut bytes = description.as_bytes().to_vec();
    let mut cursor = 13;
    while cursor + 1 < bytes.len() {
        if bytes[cursor] == b' ' {
            bytes[cursor] = b'\n';
            cursor += 14;
        } else {
            cursor += 1;
        }
    }
    format!(
        "Load Last Game?\n{}",
        String::from_utf8(bytes).expect("replacing ASCII spaces preserves UTF-8")
    )
}

type Input<'w> = (
    Res<'w, Time<Real>>,
    Res<'w, ButtonInput<KeyCode>>,
    Option<Res<'w, ButtonInput<bevy::input::mouse::MouseButton>>>,
    Res<'w, cyrus_game::player::input::PlayerBindings>,
);
type Runtime<'w, 's> = (
    Commands<'w, 's>,
    Option<ResMut<'w, DeathDispatch>>,
    Option<ResMut<'w, LastGameLoader>>,
    Option<ResMut<'w, ScriptActors>>,
);

type Rendering<'w, 's> = (
    ResMut<'w, Assets<Image>>,
    Query<'w, 's, &'static mut Sprite, With<CapturedBackground>>,
    Query<'w, 's, (Entity, &'static mut Camera), With<Camera3d>>,
    Query<'w, 's, &'static Window, With<bevy::window::PrimaryWindow>>,
    Query<'w, 's, Entity, With<CapturedBackground>>,
);

pub(super) fn drive(
    assets: (Res<AssetServer>, Res<Assets<MenuAsset>>),
    mut menu: ResMut<super::menu::MenuState>,
    capture: Option<Res<CapturedFrame>>,
    input: Input,
    rendering: Rendering,
    runtime: Runtime,
) {
    let (mut images, mut sprites, mut cameras, windows, backgrounds) = rendering;
    let Some(mut presentation) = menu.last_game.take() else {
        return;
    };
    let (mut commands, mut dispatch, mut loader, mut actors) = runtime;
    let result = (|| -> anyhow::Result<()> {
        if presentation.error.is_some() {
            return Ok(());
        }
        if let Some(bevy::asset::LoadState::Failed(error)) =
            assets.0.get_load_state(menu.asset.id())
        {
            anyhow::bail!("last-game confirmation: menu artwork: {error}");
        }
        let Some(book) = assets.1.get(&menu.asset) else {
            return Ok(());
        };
        let Some(capture) = capture.as_deref() else {
            return Ok(());
        };
        let captured = capture
            .0
            .as_ref()
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        let sample = cyrus_game::clock::bios_tick(input.0.elapsed());
        let core = presentation.core.get_or_insert_with(|| {
            LastGameConfirmation::displayed(presentation.retained_context, sample)
        });
        if !presentation.returned {
            let logical = input.3.sample_with_mouse(&input.1, input.2.as_deref());
            if let Some(choice) = core.update(
                sample,
                ConfirmationInput {
                    up: logical.forward,
                    down: logical.backward,
                    enter: input.1.pressed(KeyCode::Enter),
                    action_d: logical.action,
                },
            ) {
                presentation.returned = true;
                match choice {
                    ConfirmationChoice::No => {
                        let dispatch = dispatch.as_deref_mut().ok_or_else(|| {
                            anyhow::anyhow!("last-game confirmation: book dispatcher unavailable")
                        })?;
                        dispatch.book_context = Some(3);
                        commands.insert_resource(DeathBookEntry {
                            override_group: presentation.override_group,
                        });
                        commands.remove_resource::<LastGameEntry>();
                        commands.remove_resource::<CapturedFrame>();
                        for entity in &backgrounds {
                            commands.entity(entity).despawn();
                        }
                        commands.spawn((
                            Sprite::from_image(menu.image.clone()),
                            Transform {
                                scale: Vec3::splat(windows.single().map_or(1., |window| {
                                    super::viewport_scale(Vec2::new(
                                        window.width(),
                                        window.height(),
                                    ))
                                })),
                                ..default()
                            },
                            StartupContent,
                        ));
                        menu.context = super::menu::MenuContext::Death;
                        menu.main_disabled = [presentation.override_group != 0; 2];
                        return Ok(());
                    }
                    ConfirmationChoice::Yes => {
                        actors
                            .as_deref_mut()
                            .ok_or_else(|| {
                                anyhow::anyhow!(
                                    "last-game confirmation: selected actor unavailable"
                                )
                            })?
                            .finish_death_book()?;
                        loader
                            .as_deref_mut()
                            .ok_or_else(|| {
                                anyhow::anyhow!(
                                    "last-save slot {}: saved-game host unavailable",
                                    presentation.slot
                                )
                            })?
                            .start(presentation.save.clone())?;
                    }
                }
            }
        } else if let Some(error) = loader.as_deref_mut().and_then(LastGameLoader::take_error) {
            anyhow::bail!("last-save slot {}: {error}", presentation.slot);
        }
        let previous = presentation.selected;
        presentation.selected = core.choice();
        presentation.ready = core.accepting_input() && !presentation.returned;
        if presentation.backdrop.is_none() {
            presentation.backdrop = Some(
                captured
                    .clone()
                    .try_into_dynamic()
                    .map_err(|error| {
                        anyhow::anyhow!("last-game confirmation: captured image: {error}")
                    })?
                    .to_rgba8(),
            );
        }
        let backdrop = presentation.backdrop.as_ref().unwrap();
        let dimensions = windows.single().map_or(backdrop.dimensions(), |window| {
            (window.physical_width(), window.physical_height())
        });
        anyhow::ensure!(
            dimensions.0 > 0 && dimensions.1 > 0,
            "last-game confirmation: empty window viewport"
        );
        if !presentation.graphics
            || previous != presentation.selected
            || presentation.dimensions != dimensions
        {
            let overlay = core.paint(
                &book.confirmation,
                &image::RgbaImage::new(640, 480),
                &prompt(&presentation.save.description),
            )?;
            let pixels = composite(backdrop, &overlay, dimensions);
            let image = Image::new(
                bevy::render::render_resource::Extent3d {
                    width: dimensions.0,
                    height: dimensions.1,
                    depth_or_array_layers: 1,
                },
                bevy::render::render_resource::TextureDimension::D2,
                pixels.into_raw(),
                bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
                bevy::asset::RenderAssetUsages::default(),
            );
            if menu.image == Handle::default() {
                menu.image = images.add(image);
            } else {
                *images.get_mut(&menu.image).ok_or_else(|| {
                    anyhow::anyhow!("last-game confirmation: image unavailable")
                })? = image;
            }
            presentation.dimensions = dimensions;
        }
        if !presentation.graphics {
            for (entity, mut camera) in &mut cameras {
                if camera.is_active {
                    camera.is_active = false;
                    commands
                        .entity(entity)
                        .insert(super::menu::BookPausedCamera);
                }
            }
            let mut sprite = Sprite::from_image(menu.image.clone());
            if let Ok(window) = windows.single() {
                sprite.custom_size = Some(Vec2::new(window.width(), window.height()));
            }
            commands.spawn((
                sprite,
                Transform::default(),
                CapturedBackground,
                StartupContent,
            ));
            commands.spawn((Camera2d, StartupContent));
            presentation.graphics = true;
        } else if let Ok(window) = windows.single() {
            for mut sprite in &mut sprites {
                sprite.custom_size = Some(Vec2::new(window.width(), window.height()));
            }
        }
        Ok(())
    })();
    if let Err(error) = result {
        presentation.ready = false;
        let message = format!("{error:#}");
        presentation.error = Some(message.clone());
        menu.unavailable = Some(message.clone());
        super::menu::notice(&mut commands, &assets.0, message);
    }
    menu.ready = presentation.ready;
    if menu.context == super::menu::MenuContext::LastGame {
        menu.last_game = Some(presentation);
    }
}

// Composite byte colours before upload so the backing panel does not pass through
// the renderer's linear-light alpha blend. Filtering still needs rendered comparison.
fn composite(
    background: &image::RgbaImage,
    overlay: &image::RgbaImage,
    dimensions: (u32, u32),
) -> image::RgbaImage {
    let mut image = if background.dimensions() == dimensions {
        background.clone()
    } else {
        image::imageops::resize(
            background,
            dimensions.0,
            dimensions.1,
            image::imageops::FilterType::Triangle,
        )
    };
    for pixel in image.pixels_mut() {
        pixel[3] = 255;
    }
    let scale = (dimensions.0 as f64 / 640.).min(dimensions.1 as f64 / 480.);
    let width = (640. * scale).round().max(1.) as u32;
    let height = (480. * scale).round().max(1.) as u32;
    let overlay = image::imageops::resize(
        overlay,
        width,
        height,
        image::imageops::FilterType::Triangle,
    );
    let left = (dimensions.0 - width) / 2;
    let top = (dimensions.1 - height) / 2;
    for (x, y, source) in overlay.enumerate_pixels() {
        let destination = image.get_pixel_mut(left + x, top + y);
        let alpha = u32::from(source[3]);
        for c in 0..3 {
            destination[c] =
                ((u32::from(source[c]) * alpha + u32::from(destination[c]) * (255 - alpha) + 127)
                    / 255) as u8;
        }
    }
    image
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn description_uses_character_offsets_and_preserves_final_space() {
        assert_eq!(
            prompt("0123456789012 4567890123456 890 "),
            "Load Last Game?\n0123456789012\n4567890123456\n890 "
        );
        assert_eq!(
            prompt("a short description"),
            "Load Last Game?\na short description"
        );
        assert_eq!(prompt("0123456789012 "), "Load Last Game?\n0123456789012 ");
    }

    #[test]
    fn loader_admits_one_request_and_retains_failure_until_read() {
        let mut loader = LastGameLoader::default();
        let save = LastSaveSelection {
            slot: 17,
            description: "Example".into(),
        };
        assert!(loader.finish(Ok(())).is_err());
        loader.start(save.clone()).unwrap();
        assert!(loader.start(save.clone()).is_err());
        assert_eq!(loader.take_request().unwrap().slot, 17);
        assert!(loader.take_request().is_none());
        loader.finish(Err("bad slot".into())).unwrap();
        assert!(loader.start(save.clone()).is_err());
        assert_eq!(loader.take_error().unwrap(), "bad slot");
        loader.start(save.clone()).unwrap();
        loader.take_request().unwrap();
        loader.finish(Ok(())).unwrap();
        // Publication can leave the menu before its next update consumes a result.
        loader.start(save).unwrap();
    }
    #[test]
    fn wide_capture_composites_byte_colours_without_cropping_the_world() {
        let background = image::RgbaImage::from_pixel(960, 480, image::Rgba([100, 100, 100, 255]));
        let mut overlay = image::RgbaImage::new(640, 480);
        overlay.put_pixel(175, 140, image::Rgba([10, 10, 10, 120]));
        let output = composite(&background, &overlay, (960, 480));
        assert_eq!(output.dimensions(), (960, 480));
        assert_eq!(output.get_pixel(335, 140).0, [58, 58, 58, 255]);
        assert_eq!(output.get_pixel(334, 140).0, [100, 100, 100, 255]);
        assert_eq!(output.get_pixel(0, 0).0, [100, 100, 100, 255]);
        assert_eq!(output.get_pixel(959, 479).0, [100, 100, 100, 255]);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn installed_live_confirmation_captures_before_pausing_and_no_enters_death_book() {
        let root = std::env::var_os("REDGUARD_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../redguard")
            });
        let Ok(install) = cyrus_assets::install::GameInstall::open(&root) else {
            eprintln!("skipped: no install (live last-game confirmation)");
            return;
        };
        let cache =
            std::env::temp_dir().join(format!("cyrus-live-confirmation-{}", std::process::id()));
        cyrus_assets::startup::export_menu(&install, &cache).unwrap();
        let book: MenuAsset =
            serde_json::from_slice(&std::fs::read(cache.join("menu/book.menu")).unwrap()).unwrap();
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            bevy::state::app::StatesPlugin,
        ))
        .init_state::<cyrus_game::AppState>()
        .init_asset::<Image>()
        .init_asset::<Font>()
        .init_resource::<ButtonInput<KeyCode>>()
        .add_message::<AppExit>()
        .add_message::<cyrus_world::sound::SoundCue>()
        .insert_resource(cyrus_game::clock::SimulationClock::new(book.clock).unwrap())
        .insert_resource(
            cyrus_game::player::input::PlayerBindings::from_codes([17, 31, 30, 32, 42, 57, 38, 18])
                .unwrap(),
        )
        .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            std::time::Duration::from_millis(60),
        ));
        super::super::menu::register(&mut app);
        let asset = app
            .world_mut()
            .resource_mut::<Assets<MenuAsset>>()
            .add(book);
        app.insert_resource(super::super::StartupPresentation {
            menu: asset,
            ..default()
        });
        let save = LastSaveSelection {
            slot: 17,
            description: "Mixed case saved game".into(),
        };
        app.insert_resource(DeathDispatch {
            last_save: Some(save.clone()),
            book_context: Some(1),
        })
        .insert_resource(LastGameEntry {
            save,
            override_group: 74,
            retained_book_context: 1,
        });
        app.world_mut().spawn((
            Window {
                resolution: (960, 480).into(),
                ..default()
            },
            bevy::window::PrimaryWindow,
        ));
        let camera = app.world_mut().spawn(Camera3d::default()).id();
        app.world_mut()
            .resource_mut::<NextState<cyrus_game::AppState>>()
            .set(cyrus_game::AppState::Menu);
        app.update();
        assert!(app.world().get::<Camera>(camera).unwrap().is_active);
        assert_eq!(
            app.world_mut()
                .query::<&Camera2d>()
                .iter(app.world())
                .count(),
            0
        );
        assert_eq!(
            app.world().resource::<DeathDispatch>().book_context,
            Some(1)
        );
        let image = Image::new(
            bevy::render::render_resource::Extent3d {
                width: 960,
                height: 480,
                depth_or_array_layers: 1,
            },
            bevy::render::render_resource::TextureDimension::D2,
            vec![100; 960 * 480 * 4],
            bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
            bevy::asset::RenderAssetUsages::default(),
        );
        app.insert_resource(CapturedFrame(Ok(image)));
        app.update();
        assert!(!app.world().get::<Camera>(camera).unwrap().is_active);
        let state = app.world().resource::<super::super::menu::MenuState>();
        assert!(state.ready, "{:?}", state.unavailable);
        let overlay = app
            .world()
            .resource::<Assets<Image>>()
            .get(&state.image)
            .unwrap();
        assert_eq!(overlay.width(), 960);
        assert_eq!(overlay.data.as_ref().unwrap()[3], 255);
        let backdrop_handle = app
            .world_mut()
            .query_filtered::<&Sprite, With<CapturedBackground>>()
            .single(app.world())
            .unwrap()
            .image
            .clone();
        assert_eq!(
            app.world()
                .resource::<Assets<Image>>()
                .get(&backdrop_handle)
                .unwrap()
                .width(),
            960
        );
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyS);
        app.update();
        assert_eq!(
            app.world()
                .resource::<super::super::menu::MenuState>()
                .last_game
                .as_ref()
                .unwrap()
                .selected,
            ConfirmationChoice::No
        );
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Enter);
        app.update();
        assert!(!app.world().contains_resource::<DeathBookEntry>());
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(KeyCode::Enter);
        app.update();
        assert_eq!(
            app.world()
                .resource::<super::super::menu::MenuState>()
                .context,
            super::super::menu::MenuContext::Death
        );
        assert_eq!(
            app.world().resource::<DeathDispatch>().book_context,
            Some(3)
        );
        assert_eq!(app.world().resource::<DeathBookEntry>().override_group, 74);
        assert!(!app.world().contains_resource::<LastGameEntry>());
        assert!(
            !app.world()
                .contains_resource::<cyrus_world::lifecycle::SceneRequest>()
        );
        assert!(!app.world().contains_resource::<super::super::StartGame>());
        std::fs::remove_dir_all(cache).unwrap();
    }
}
