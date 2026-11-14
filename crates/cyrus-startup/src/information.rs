//! Fixed F1 information and F10 credits screens, returning to the suspended world.
use super::{confirmation::LastGameConfirmation, credits};
use bevy::{
    asset::RenderAssetUsages,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use cyrus_assets::{logbook::LogbookArtwork, menu::MenuAsset};
use cyrus_game::{AppState, GameplayOverlay};
use cyrus_script::{
    clock::{SimulationClock, bios_tick},
    player::input::{Action, Inputs},
    script::CreditsPlaying,
};

#[derive(Component)]
struct Panel;

#[derive(Component)]
struct Picture;

#[derive(Resource)]
struct Presentation {
    menu: Handle<MenuAsset>,
    reader: Handle<LogbookArtwork>,
    image: Handle<Image>,
    scroll: Option<(credits::Position, f64)>,
    credits_exit: credits::CreditsExit,
    confirmed: bool,
    painted: bool,
    time_was_paused: bool,
}

pub(super) fn register(app: &mut App) {
    app.add_systems(OnEnter(AppState::GameplayOverlay), setup)
        .add_systems(Update, drive.run_if(in_state(AppState::GameplayOverlay)))
        .add_systems(OnExit(AppState::GameplayOverlay), cleanup);
}

fn setup(
    mut commands: Commands,
    server: Res<AssetServer>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut time: ResMut<Time<Virtual>>,
) {
    let time_was_paused = time.is_paused();
    time.pause();
    keys.reset(KeyCode::F1);
    keys.reset(KeyCode::F10);
    commands.insert_resource(Presentation {
        menu: server.load("cache://menu/book.menu"),
        reader: server.load("cache://logbook/reader.logbook"),
        image: Handle::default(),
        scroll: None,
        credits_exit: credits::CreditsExit::default(),
        confirmed: false,
        painted: false,
        time_was_paused,
    });
    commands
        .spawn((
            Panel,
            GlobalZIndex(110),
            Node {
                width: percent(100),
                height: percent(100),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
        ))
        .with_children(|root| {
            root.spawn((
                Picture,
                ImageNode::default(),
                Node {
                    height: percent(100),
                    max_width: percent(100),
                    aspect_ratio: Some(4.0 / 3.0),
                    ..default()
                },
            ));
        });
}

#[allow(clippy::too_many_arguments)] // The modal owns its input, assets, pixels and return transition.
fn drive(
    mut commands: Commands,
    screen: Res<GameplayOverlay>,
    mut active: ResMut<Presentation>,
    server: Res<AssetServer>,
    menus: Res<Assets<MenuAsset>>,
    readers: Res<Assets<LogbookArtwork>>,
    mut images: ResMut<Assets<Image>>,
    mut nodes: Query<&mut ImageNode, With<Picture>>,
    time: Res<Time<Real>>,
    mut input: ParamSet<(Inputs, ResMut<ButtonInput<KeyCode>>)>,
    mut next: ResMut<NextState<AppState>>,
) {
    let pixels = match *screen {
        GameplayOverlay::Version => {
            let Some(menu) = menus.get(&active.menu) else {
                if matches!(
                    server.get_load_state(&active.menu),
                    Some(bevy::asset::LoadState::Failed(_))
                ) {
                    error!("F1: installed menu artwork could not be loaded");
                    next.set(AppState::InGame);
                }
                return;
            };
            let input = input.p0();
            let confirming = input
                .devices()
                .keys
                .is_some_and(|keys| keys.pressed(KeyCode::Enter))
                || input.pressed(Action::Use);
            if active.confirmed && !confirming {
                next.set(AppState::InGame);
                return;
            }
            active.confirmed |= confirming;
            if active.painted {
                return;
            }
            // Compatibility versions displayed by the GOG original, including its save and console formats.
            match LastGameConfirmation::paint_information(
                &menu.confirmation,
                "Redguard v1.0\nBuild 1.0\nSaveGame v1.31\nConsole v2.0",
            ) {
                Ok(image) => image.into_raw(),
                Err(error) => {
                    error!("F1 version dialog: {error:#}");
                    next.set(AppState::InGame);
                    return;
                }
            }
        }
        GameplayOverlay::Credits => {
            let Some(data) = readers.get(&active.reader) else {
                if matches!(
                    server.get_load_state(&active.reader),
                    Some(bevy::asset::LoadState::Failed(_))
                ) {
                    warn!("F10: installed credits artwork could not be loaded");
                    next.set(AppState::InGame);
                }
                return;
            };
            let Some(records) = data.credits.as_deref() else {
                next.set(AppState::InGame);
                return;
            };
            let Some(style) = credits::style_of(&data.styles, 0) else {
                error!("F10: reader style 0 is not initialised");
                next.set(AppState::InGame);
                return;
            };
            let line = i32::from(data.fonts[style.font].line_height);
            let (mut position, mut due) = active.scroll.unwrap_or_else(|| {
                commands.insert_resource(CreditsPlaying);
                (credits::Position::start(line), 0.0)
            });
            match active.credits_exit.poll(&mut input.p1()) {
                credits::CreditsExitState::Scrolling => {}
                credits::CreditsExitState::WaitingForRelease => return,
                credits::CreditsExitState::Finished => {
                    next.set(AppState::InGame);
                    return;
                }
            }
            let mut done = false;
            due += time.delta_secs_f64() * credits::PIXELS_PER_SECOND;
            while !done && due >= 1.0 {
                due -= 1.0;
                done = position.advance(records, line);
            }
            if done {
                next.set(AppState::InGame);
                return;
            }
            active.scroll = Some((position, due));
            credits::paint(
                &data.fonts,
                &data.styles,
                &data.palette,
                records,
                &position,
                line,
            )
        }
    };
    let image = Image::new(
        Extent3d {
            width: 640,
            height: 480,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        pixels,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    if active.image == Handle::default() {
        active.image = images.add(image);
        for mut node in &mut nodes {
            node.image = active.image.clone();
        }
    } else if let Some(mut previous) = images.get_mut(&active.image) {
        *previous = image;
    }
    active.painted = true;
}

fn cleanup(
    mut commands: Commands,
    panels: Query<Entity, With<Panel>>,
    time: Res<Time<Real>>,
    clock: Option<ResMut<SimulationClock>>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    active: Res<Presentation>,
    mut virtual_time: ResMut<Time<Virtual>>,
) {
    if !active.time_was_paused {
        virtual_time.unpause();
    }
    for panel in &panels {
        commands.entity(panel).despawn();
    }
    if let Some(mut clock) = clock {
        clock.rebase(bios_tick(time.elapsed()));
    }
    keys.reset(KeyCode::Escape);
    keys.reset(KeyCode::F1);
    keys.reset(KeyCode::F10);
    commands.remove_resource::<Presentation>();
    commands.remove_resource::<CreditsPlaying>();
    commands.remove_resource::<GameplayOverlay>();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modal_lifecycle_retains_world_time_and_drains_return_keys() {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            bevy::state::app::StatesPlugin,
        ))
        .insert_state(AppState::InGame)
        .init_asset::<MenuAsset>()
        .init_asset::<LogbookArtwork>()
        .init_resource::<ButtonInput<KeyCode>>()
        .add_systems(OnEnter(AppState::GameplayOverlay), setup)
        .add_systems(OnExit(AppState::GameplayOverlay), cleanup);
        let world_entity = app
            .world_mut()
            .spawn(Transform::from_xyz(3.0, 4.0, 5.0))
            .id();
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::ArrowUp);
        for screen in [GameplayOverlay::Version, GameplayOverlay::Credits] {
            app.insert_resource(screen);
            app.world_mut()
                .resource_mut::<NextState<AppState>>()
                .set(AppState::GameplayOverlay);
            app.update();
            assert!(app.world().resource::<Time<Virtual>>().is_paused());
            assert!(app.world().contains_resource::<Presentation>());
            let before = app.world().resource::<Time<Virtual>>().elapsed();
            app.update();
            assert_eq!(app.world().resource::<Time<Virtual>>().elapsed(), before);
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::Escape);
            app.world_mut()
                .resource_mut::<NextState<AppState>>()
                .set(AppState::InGame);
            app.update();
            assert!(!app.world().resource::<Time<Virtual>>().is_paused());
            assert!(!app.world().contains_resource::<GameplayOverlay>());
            assert!(!app.world().contains_resource::<Presentation>());
            assert!(
                !app.world()
                    .resource::<ButtonInput<KeyCode>>()
                    .pressed(KeyCode::Escape)
            );
            assert!(app.world().get_entity(world_entity).is_ok());
            assert!(
                app.world()
                    .resource::<ButtonInput<KeyCode>>()
                    .pressed(KeyCode::ArrowUp)
            );
            assert_eq!(
                app.world_mut()
                    .query_filtered::<Entity, With<Panel>>()
                    .iter(app.world())
                    .count(),
                0
            );
        }
    }

    #[test]
    fn installed_version_dialog_ignores_escape_and_waits_for_confirmation_release() {
        let root = std::env::var_os("REDGUARD_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../redguard")
            });
        let Ok(install) = cyrus_assets::install::GameInstall::open(&root) else {
            eprintln!("skipped: no install (F1 version dialog)");
            return;
        };
        let cache =
            std::env::temp_dir().join(format!("cyrus-version-dialog-{}", std::process::id()));
        cyrus_assets::startup::export_menu(&install, &cache).unwrap();
        let menu: MenuAsset =
            serde_json::from_slice(&std::fs::read(cache.join("menu/book.menu")).unwrap()).unwrap();
        std::fs::remove_dir_all(cache).unwrap();
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<MenuAsset>()
            .init_asset::<LogbookArtwork>()
            .init_asset::<Image>()
            .init_resource::<ButtonInput<KeyCode>>()
            .insert_resource(NextState::<AppState>::Unchanged)
            .insert_resource(GameplayOverlay::Version)
            .add_systems(Update, drive);
        let menu = app
            .world_mut()
            .resource_mut::<Assets<MenuAsset>>()
            .add(menu);
        app.insert_resource(Presentation {
            menu,
            reader: Handle::default(),
            image: Handle::default(),
            scroll: None,
            credits_exit: credits::CreditsExit::default(),
            confirmed: false,
            painted: false,
            time_was_paused: false,
        });
        app.world_mut().spawn((Picture, ImageNode::default()));
        app.update();
        assert!(app.world().resource::<Presentation>().painted);
        let handle = app.world().resource::<Presentation>().image.clone();
        let image = app
            .world()
            .resource::<Assets<Image>>()
            .get(&handle)
            .unwrap();
        assert_eq!(image.size(), UVec2::new(640, 480));
        assert!(
            image
                .data
                .as_ref()
                .unwrap()
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[3] > 0)
        );
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        app.update();
        assert!(matches!(
            app.world().resource::<NextState<AppState>>(),
            NextState::Unchanged
        ));
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Enter);
        app.update();
        assert!(matches!(
            app.world().resource::<NextState<AppState>>(),
            NextState::Unchanged
        ));
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(KeyCode::Enter);
        app.update();
        assert!(matches!(
            app.world().resource::<NextState<AppState>>(),
            NextState::Pending(AppState::InGame)
        ));
    }
    #[test]
    fn credits_present_reader_records_and_escape_returns_without_a_movie() {
        use cyrus_assets::{
            logbook::{CreditLine, ReaderStyle},
            menu::{MenuModel, MenuPicture},
        };
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<MenuAsset>()
            .init_asset::<LogbookArtwork>()
            .init_asset::<Image>()
            .init_resource::<ButtonInput<KeyCode>>()
            .insert_resource(NextState::<AppState>::Unchanged)
            .insert_resource(GameplayOverlay::Credits)
            .add_systems(Update, drive);
        let reader = app
            .world_mut()
            .resource_mut::<Assets<LogbookArtwork>>()
            .add(LogbookArtwork {
                paper: MenuPicture {
                    width: 1,
                    height: 1,
                    rgba: vec![0; 4],
                },
                scroll: MenuModel {
                    position: [0.0; 3],
                    frames: vec![],
                    faces: vec![],
                },
                scroll_normals: vec![],
                scroll_brightness: vec![],
                textures: vec![],
                palette: vec![],
                fonts: vec![cyrus_text::Font {
                    start: 0,
                    line_height: 26,
                    glyphs: vec![],
                    palette: vec![],
                }],
                styles: vec![Some(ReaderStyle {
                    font: 0,
                    scale: 255,
                    alpha: 255,
                    page_palette: false,
                })],
                credits: Some(vec![CreditLine::default()]),
                credits_exported: true,
            });
        app.insert_resource(Presentation {
            menu: Handle::default(),
            reader,
            image: Handle::default(),
            scroll: None,
            credits_exit: credits::CreditsExit::default(),
            confirmed: false,
            painted: false,
            time_was_paused: false,
        });
        app.world_mut().spawn((Picture, ImageNode::default()));
        app.update();
        assert!(app.world().resource::<Presentation>().painted);
        assert!(app.world().contains_resource::<CreditsPlaying>());
        assert!(
            !app.world()
                .contains_resource::<super::super::movie::MoviePlayback>()
        );
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        app.update();
        assert!(matches!(
            app.world().resource::<NextState<AppState>>(),
            NextState::Unchanged
        ));
        let before = app.world().resource::<Presentation>().scroll.unwrap();
        app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            std::time::Duration::from_secs(2),
        ));
        app.update();
        let after = app.world().resource::<Presentation>().scroll.unwrap();
        assert_eq!(
            before, after,
            "Escape release wait freezes the credits scroll"
        );
        assert!(app.world().contains_resource::<CreditsPlaying>());
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(KeyCode::Escape);
        app.update();
        assert!(matches!(
            app.world().resource::<NextState<AppState>>(),
            NextState::Pending(AppState::InGame)
        ));
    }
}
