//! The installed title stays visible until menu or world assets are ready.
use super::{StartGame, StartupContent, StartupPresentation};
use bevy::asset::{AssetLoader, LoadContext, io::Reader};
use bevy::world_serialization::WorldInstance;
use bevy::{asset::RecursiveDependencyLoadState, prelude::*};
use cyrus_assets::loading_artwork::LoadingArtwork;
use cyrus_assets::menu::MenuAsset;
use cyrus_game::AppState;
use cyrus_world::lifecycle::{PreparationProgress, PreparedWorld, SceneLoadError};

#[derive(Resource)]
struct WorldAssets(Vec<UntypedHandle>);
/// Six sidecar handles plus the spawn and entry steps counted after preparation.
const WORLD_ASSET_STEPS: usize = 8;

#[derive(bevy::ecs::system::SystemParam)]
struct WorldEntry<'w, 's> {
    game: Option<ResMut<'w, StartGame>>,
    playback: ResMut<'w, super::Playback>,
    prepared: Option<Res<'w, PreparedWorld>>,
    preparation: Option<Res<'w, PreparationProgress>>,
    world_assets: Option<Res<'w, WorldAssets>>,
    scene_error: Option<Res<'w, SceneLoadError>>,
    #[cfg(not(target_arch = "wasm32"))]
    menu_prepared: Option<Res<'w, crate::preparation::MenuPrepared>>,
    #[cfg(target_arch = "wasm32")]
    sound_bank: Option<Res<'w, cyrus_audio::SoundBankHandle>>,
    #[cfg(target_arch = "wasm32")]
    sound_settings: Option<Res<'w, cyrus_audio::sound::SoundSettings>>,
    spawner: Res<'w, WorldInstanceSpawner>,
    instances: Query<'w, 's, Option<&'static WorldInstance>, With<WorldAssetRoot>>,
    clock: Option<Res<'w, cyrus_script::clock::SimulationClock>>,
    fades: Query<'w, 's, &'static super::fade::Fade>,
}

#[derive(Default, TypePath)]
struct LoadingArtworkLoader;
impl AssetLoader for LoadingArtworkLoader {
    type Asset = LoadingArtwork;
    type Settings = ();
    type Error = anyhow::Error;
    async fn load(
        &self,
        reader: &mut dyn Reader,
        _: &(),
        _: &mut LoadContext<'_>,
    ) -> Result<LoadingArtwork, Self::Error> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        let artwork: LoadingArtwork = serde_json::from_slice(&bytes)?;
        artwork.validate()?;
        Ok(artwork)
    }
    fn extensions(&self) -> &[&str] {
        &["loading"]
    }
}

pub(super) fn register(app: &mut App) {
    app.init_asset::<LoadingArtwork>()
        .init_asset_loader::<LoadingArtworkLoader>();
    app.add_systems(Update, begin_requested_world)
        .add_systems(OnEnter(AppState::Loading), suspend_world_cameras)
        .add_systems(OnExit(AppState::Loading), resume_world_cameras)
        .add_systems(Update, poll.run_if(in_state(AppState::Loading)))
        .add_systems(
            Update,
            super::fade::advance.after(cyrus_script::script::OrdinaryUpdates),
        )
        .add_systems(OnEnter(AppState::InGame), fade_in_world)
        .add_systems(OnExit(AppState::Loading), cleanup);
}

#[derive(bevy::ecs::system::SystemParam)]
struct ArtworkAssets<'w, 's> {
    images: Res<'w, Assets<LoadingArtwork>>,
    pending: Local<'s, Option<(String, Handle<LoadingArtwork>)>>,
    #[cfg(not(target_arch = "wasm32"))]
    install: Option<Res<'w, cyrus_assets::install::GameInstall>>,
    #[cfg(not(target_arch = "wasm32"))]
    preparation: Local<'s, Option<(String, bevy::tasks::Task<anyhow::Result<()>>)>>,
}

fn begin_requested_world(
    mut commands: Commands,
    request: Option<Res<super::WorldLoadRequest>>,
    assets: (
        Res<AssetServer>,
        Res<Assets<MenuAsset>>,
        ResMut<Assets<Image>>,
    ),
    mut artwork: ArtworkAssets,
    mut startup: ResMut<StartupPresentation>,
    mut next: ResMut<NextState<AppState>>,
    timing: (Res<State<AppState>>, Res<Time>),
) {
    let (assets, menus, mut images) = assets;
    let (state, time) = timing;
    let Some(request) = request else { return };
    if !matches!(state.get(), AppState::InGame | AppState::Menu) {
        return;
    }
    if startup.menu == Handle::default() {
        startup.menu = assets.load("cache://menu/book.menu");
    }
    let Some(book) = menus.get(&startup.menu) else {
        if let Some(bevy::asset::LoadState::Failed(error)) =
            assets.get_load_state(startup.menu.id())
        {
            let message = format!("World {} loading artwork: {error}", request.0);
            error!("{message}");
            commands.insert_resource(SceneLoadError(message));
            commands.remove_resource::<super::WorldLoadRequest>();
        }
        return;
    };
    let (world, pictures) = if request.0 == book.new_game.world.to_string() {
        (book.new_game.world, &book.new_game.loading)
    } else {
        if artwork
            .pending
            .as_ref()
            .is_none_or(|(world, _)| *world != request.0)
        {
            #[cfg(not(target_arch = "wasm32"))]
            if let Some(install) = artwork.install.as_ref() {
                use bevy::tasks::{IoTaskPool, block_on, poll_once};
                let install = (**install).clone();
                if artwork
                    .preparation
                    .as_ref()
                    .is_none_or(|(world, _)| *world != request.0)
                {
                    let name = request.0.clone();
                    *artwork.preparation = Some((
                        name.clone(),
                        IoTaskPool::get().spawn(async move {
                            cyrus_assets::loading_artwork::export(
                                &install,
                                &name,
                                cyrus_config::cache_dir(),
                            )
                        }),
                    ));
                }
                let Some(result) =
                    block_on(poll_once(&mut artwork.preparation.as_mut().unwrap().1))
                else {
                    return;
                };
                *artwork.preparation = None;
                if let Err(error) = result {
                    commands.insert_resource(SceneLoadError(format!(
                        "World {} loading artwork: {error:#}",
                        request.0
                    )));
                    commands.remove_resource::<super::WorldLoadRequest>();
                    return;
                }
            }
            *artwork.pending = Some((
                request.0.clone(),
                assets.load(format!("cache://loading/{}.loading", request.0)),
            ));
        }
        let handle = &artwork.pending.as_ref().unwrap().1;
        let Some(artwork) = artwork.images.get(handle) else {
            if let Some(bevy::asset::LoadState::Failed(error)) = assets.get_load_state(handle.id())
            {
                commands.insert_resource(SceneLoadError(format!(
                    "World {} loading artwork: {error}",
                    request.0
                )));
                commands.remove_resource::<super::WorldLoadRequest>();
            }
            return;
        };
        (artwork.world, &artwork.pictures)
    };
    commands.insert_resource(cyrus_world::lifecycle::SceneRequest(request.0.clone()));
    commands.insert_resource(cyrus_world::lifecycle::ReloadScene);
    commands.insert_resource(cyrus_script::player::placement::MarkerEntry::default());
    commands.remove_resource::<super::WorldLoadRequest>();
    commands.remove_resource::<cyrus_game::PauseBookEntry>();
    commands.remove_resource::<cyrus_script::PausedGameplay>();
    super::menu::begin_world_loading(
        &mut commands,
        &mut images,
        &mut next,
        book,
        world,
        pictures,
        time.elapsed_secs_f64(),
    );
}

#[derive(Component)]
struct LoadingPausedCamera;

fn suspend_world_cameras(
    mut commands: Commands,
    mut cameras: Query<(Entity, &mut Camera), super::view3d::GameplayCameras>,
) {
    for (entity, mut camera) in &mut cameras {
        if camera.is_active {
            camera.is_active = false;
            commands.entity(entity).insert(LoadingPausedCamera);
        }
    }
}

fn resume_world_cameras(
    mut commands: Commands,
    mut cameras: Query<(Entity, &mut Camera), With<LoadingPausedCamera>>,
) {
    for (entity, mut camera) in &mut cameras {
        camera.is_active = true;
        commands.entity(entity).remove::<LoadingPausedCamera>();
    }
}

fn poll(
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut startup: ResMut<StartupPresentation>,
    input: (Res<ButtonInput<KeyCode>>, ResMut<NextState<AppState>>),
    entry: WorldEntry,
    rendering: (
        ResMut<Assets<Image>>,
        Query<Entity, With<super::menu::MenuNotice>>,
    ),
) {
    let (keys, mut next) = input;
    let WorldEntry {
        mut game,
        mut playback,
        prepared,
        preparation,
        world_assets,
        scene_error,
        #[cfg(not(target_arch = "wasm32"))]
        menu_prepared,
        #[cfg(target_arch = "wasm32")]
        sound_bank,
        #[cfg(target_arch = "wasm32")]
        sound_settings,
        spawner,
        instances,
        clock,
        fades,
    } = entry;
    let (mut images, notices) = rendering;
    if !startup.title_visible {
        return;
    }
    // Direct inspections show the title without menu-entry fades.
    let fade_clock = clock
        .filter(|_| {
            game.as_ref()
                .is_some_and(|game| game.image != Handle::default())
        })
        .map(|clock| clock.settings());
    if let (Some(game), Some(settings)) = (game.as_mut(), fade_clock)
        && !game.faded_in
    {
        super::fade::spawn(&mut commands, settings, true);
        game.faded_in = true;
    }
    if game.is_some() {
        if let Some(error) = scene_error {
            startup.error = Some(error.0.clone());
        }
        let offset = preparation.as_ref().map_or(0, |p| p.total);
        if prepared.is_none()
            && startup.error.is_none()
            && let Some(game) = game.as_mut()
        {
            let (done, total) = preparation
                .as_ref()
                .map_or((0, WORLD_ASSET_STEPS), |stage| {
                    (stage.done, offset + WORLD_ASSET_STEPS)
                });
            update_progress(game, &mut images, done, total);
        }
        if startup.error.is_none()
            && let Some(prepared) = prepared
        {
            if let Some(handles) = world_assets {
                let mut loaded = 0;
                for handle in &handles.0 {
                    if let Some(bevy::asset::LoadState::Failed(error)) =
                        assets.get_load_state(handle.id())
                    {
                        startup.error = Some(format!("World {}: {error}", prepared.requested_name));
                        break;
                    }
                    if let Some(RecursiveDependencyLoadState::Failed(error)) =
                        assets.get_recursive_dependency_load_state(handle.id())
                    {
                        startup.error = Some(format!("World {}: {error}", prepared.requested_name));
                        break;
                    }
                    if assets.is_loaded_with_dependencies(handle.id()) {
                        loaded += 1;
                    }
                }
                let spawned = !instances.is_empty()
                    && instances.iter().all(|instance| {
                        instance.is_some_and(|instance| spawner.instance_is_ready(**instance))
                    });
                if let Some(game) = game.as_mut() {
                    let total = offset + handles.0.len() + 2;
                    let done = offset + loaded + 1 + usize::from(spawned);
                    update_progress(game, &mut images, done, total);
                }
                if loaded == handles.0.len() && spawned && startup.error.is_none() {
                    let fade_out = game.as_ref().and_then(|game| game.fade_out);
                    match (fade_out, fade_clock) {
                        (None, Some(settings)) => {
                            if let Some(game) = game.as_mut() {
                                game.fade_out =
                                    Some(super::fade::spawn(&mut commands, settings, false));
                            }
                        }
                        (Some(fade), _) if !fades.get(fade).is_ok_and(|f| f.black()) => {}
                        _ => {
                            commands.remove_resource::<StartGame>();
                            next.set(AppState::InGame);
                        }
                    }
                }
            } else {
                let handles = vec![
                    prepared.animations.clone().untyped(),
                    prepared.scripts.clone().untyped(),
                    assets
                        .load::<cyrus_script::player::data::PlayerScene>(
                            prepared.player_path.clone(),
                        )
                        .untyped(),
                    assets
                        .load::<cyrus_assets::environment::Environment>(
                            prepared.environment_path.clone(),
                        )
                        .untyped(),
                    assets.load::<Gltf>(prepared.scene_path.clone()).untyped(),
                    assets
                        .load::<cyrus_assets::hud::HudAsset>("cache://hud/overlay.hud")
                        .untyped(),
                ];
                for handle in &handles {
                    if (matches!(
                        assets.get_load_state(handle.id()),
                        Some(bevy::asset::LoadState::Failed(_))
                    ) || matches!(
                        assets.get_recursive_dependency_load_state(handle.id()),
                        Some(RecursiveDependencyLoadState::Failed(_))
                    )) && let Some(path) = assets.get_path(handle.id())
                    {
                        assets.reload(path);
                    }
                }
                if let Some(game) = game.as_mut() {
                    update_progress(game, &mut images, offset + 1, offset + handles.len() + 2);
                }
                commands.insert_resource(WorldAssets(handles));
            }
        }
        if startup.error.is_some() && keys.just_pressed(KeyCode::Escape) {
            commands.remove_resource::<StartGame>();
            commands.remove_resource::<SceneLoadError>();
            next.set(AppState::Menu);
        }
    } else {
        #[cfg(not(target_arch = "wasm32"))]
        let title = if menu_prepared.is_some() {
            "cache://startup/title-menu.png"
        } else {
            "cache://startup/title-preparing.png"
        };
        #[cfg(target_arch = "wasm32")]
        let title = "cache://startup/title-menu.png";
        playback.title = assets.load(title);
        #[cfg(not(target_arch = "wasm32"))]
        if menu_prepared.is_none() && startup.error.is_none() {
            return;
        }
        if startup.error.is_none() {
            if startup.menu == Handle::default() {
                startup.menu = assets.load::<MenuAsset>("cache://menu/book.menu");
            }
            match assets.get_recursive_dependency_load_state(startup.menu.id()) {
                Some(RecursiveDependencyLoadState::Loaded) => {
                    #[cfg(target_arch = "wasm32")]
                    {
                        if sound_settings.is_none() {
                            playback.title = assets.load("cache://startup/title-sound.png");
                            let Some(bank) = sound_bank else {
                                return;
                            };
                            if let Some(bevy::asset::LoadState::Failed(error)) =
                                assets.get_load_state(bank.0.id())
                            {
                                startup.error = Some(format!("Menu effects unavailable: {error}"));
                            } else if let Some(RecursiveDependencyLoadState::Failed(error)) =
                                assets.get_recursive_dependency_load_state(bank.0.id())
                            {
                                startup.error = Some(format!("Menu effects unavailable: {error}"));
                            } else {
                                return;
                            }
                        }
                    }
                    if startup.error.is_none() {
                        next.set(AppState::Menu);
                    }
                }
                Some(RecursiveDependencyLoadState::Failed(error)) => {
                    startup.error = Some(format!(
                        "Menu assets unavailable: {error}. Re-export startup assets."
                    ))
                }
                _ => {
                    if let Some(bevy::asset::LoadState::Failed(error)) =
                        assets.get_load_state(startup.menu.id())
                    {
                        startup.error = Some(format!(
                            "Menu assets unavailable: {error}. Re-export startup assets."
                        ));
                    }
                }
            }
        }
    }
    if let Some(error) = &startup.error
        && notices.is_empty()
    {
        commands.spawn((
            StartupContent,
            super::menu::MenuNotice,
            Text::new(if game.is_some() {
                format!("{error}\nPress Escape to return to the menu.")
            } else {
                error.clone()
            }),
        ));
    }
}

/// The first world view fades in from the black left by the loading fade-out.
fn fade_in_world(mut fades: Query<&mut super::fade::Fade>) {
    for mut fade in &mut fades {
        if fade.black() {
            fade.reverse();
        }
    }
}

fn update_progress(game: &mut StartGame, images: &mut Assets<Image>, done: usize, total: usize) {
    let percent = ((done * 100 + total / 2) / total) as u8;
    let (previous_done, previous_total) = game.progress;
    let previous_percent =
        ((previous_done * 100 + previous_total / 2) / previous_total.max(1)) as u8;
    if percent <= previous_percent || game.background.is_empty() {
        return;
    }
    if let Some(mut image) = images.get_mut(&game.image) {
        let mut pixels = game.background.clone();
        game.progress = (done, total);
        paint_progress(&mut pixels, &game.font, game.progress.0, game.progress.1);
        image.data = Some(pixels);
    }
}

/// Preservation's world-loading viewport and REDSEL label, independent of host speed.
fn paint_progress(pixels: &mut [u8], font: &cyrus_text::Font, done: usize, total: usize) {
    if done == 0 {
        return;
    }
    let filled = (576 * done + total / 2) / total;
    let percent = (100 * done + total / 2) / total;
    // The fill is the system palette block's entry 147 (54, 5, 4) scaled by four, which the
    // original's nearest-colour lookup returns for its (63, 0, 0) request (it dithers between
    // (214..222, 20..24, 8..16); the mean is (218, 21, 14)). Both rectangles use inclusive edges:
    // a 577 by 7 track, and a fill reaching `filled` pixels past its left edge (measured from
    // captures of the original).
    for y in 448..=454 {
        for x in 32..=608 {
            let at = (y * 640 + x) * 4;
            if x <= 32 + filled {
                pixels[at..at + 4].copy_from_slice(&[216, 20, 16, 255]);
            } else {
                // The unfilled track darkens the picture: a 180/255 blend toward (9, 9, 10),
                // fitted to captures of the original (mean error 2.5 levels per channel).
                for (value, dark) in pixels[at..at + 3].iter_mut().zip([9u32, 9, 10]) {
                    *value = ((u32::from(*value) * 75 + dark * 180 + 127) / 255) as u8;
                }
            }
        }
    }
    // Alphas fitted to captures of the original (mean error 2.5 levels against 10.8 for the
    // previous 255 and 180): the label is translucent like the track.
    font.draw_shadowed(pixels, 640, &format!("{percent}%"), [32, 451], [180, 140]);
}

fn cleanup(mut commands: Commands) {
    commands.remove_resource::<WorldAssets>();
    commands.remove_resource::<StartGame>();
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn debug_world_entry_uses_loading_artwork_and_recreates_the_player() {
        world_entry_recreates_player(false, 0);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn world_entry_from_paused_book_recreates_the_retained_player() {
        world_entry_recreates_player(true, 0);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn destination_world_entry_presents_its_configured_artwork() {
        world_entry_recreates_player(false, 2);
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn world_entry_recreates_player(from_book: bool, destination: u32) {
        use cyrus_script::player::{Player, PlayerPlugin};
        let Ok(install) = cyrus_assets::install::GameInstall::locate(None) else {
            eprintln!("skipped: no install (debug world loading)");
            return;
        };
        let cache = std::env::temp_dir().join(format!(
            "cyrus-debug-loading-{}-{from_book}-{destination}",
            std::process::id()
        ));
        cyrus_assets::startup::export_menu(&install, &cache).unwrap();
        let book: MenuAsset =
            serde_json::from_slice(&std::fs::read(cache.join("menu/book.menu")).unwrap()).unwrap();
        cyrus_assets::loading_artwork::export(&install, &destination.to_string(), &cache).unwrap();
        let destination_artwork: LoadingArtwork = serde_json::from_slice(
            &std::fs::read(cache.join(format!("loading/{destination}.loading"))).unwrap(),
        )
        .unwrap();
        std::fs::remove_dir_all(cache).unwrap();
        let artwork = destination_artwork
            .pictures
            .iter()
            .map(|picture| picture.rgba.clone())
            .collect::<Vec<_>>();
        let new_game_artwork = book
            .new_game
            .loading
            .iter()
            .map(|picture| picture.rgba.clone())
            .collect::<Vec<_>>();
        let mut app = App::new();
        let directory = bevy::asset::io::memory::Dir::default();
        directory.insert_asset(
            std::path::Path::new(&format!("loading/{destination}.loading")),
            serde_json::to_vec(&destination_artwork).unwrap(),
        );
        app.register_asset_source(
            "cache",
            bevy::asset::io::AssetSourceBuilder::new(move || {
                Box::new(bevy::asset::io::memory::MemoryAssetReader {
                    root: directory.clone(),
                })
            }),
        );
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            bevy::state::app::StatesPlugin,
        ))
        .insert_state(AppState::InGame)
        .init_asset::<MenuAsset>()
        .init_asset::<LoadingArtwork>()
        .init_asset::<Image>()
        .init_resource::<cyrus_game::fidelity::Fidelity>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<cyrus_script::player::input::PlayerBindings>()
        .add_plugins(PlayerPlugin)
        .add_systems(Update, begin_requested_world)
        .add_systems(OnEnter(AppState::Loading), suspend_world_cameras)
        .add_systems(OnExit(AppState::Loading), resume_world_cameras);
        let menu = app
            .world_mut()
            .resource_mut::<Assets<MenuAsset>>()
            .add(book);
        app.insert_resource(StartupPresentation { menu, ..default() });
        app.init_asset_loader::<LoadingArtworkLoader>();
        let camera = app
            .world_mut()
            .spawn((Camera3d::default(), Camera::default()))
            .id();
        app.update();
        let old_player = app
            .world_mut()
            .query_filtered::<Entity, With<Player>>()
            .single(app.world())
            .unwrap();
        if from_book {
            app.insert_state(AppState::Menu);
            app.insert_resource(cyrus_game::PauseBookEntry { page: 0 });
        }
        app.insert_resource(cyrus_script::player::placement::MarkerEntry {
            marker: 9,
            heading: 512,
        });
        app.insert_resource(super::super::WorldLoadRequest(destination.to_string()));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while app.world().get_resource::<StartGame>().is_none()
            && std::time::Instant::now() < deadline
        {
            app.update();
            std::thread::yield_now();
        }
        assert_eq!(
            app.world().resource::<StartGame>().background.len(),
            640 * 480 * 4
        );
        assert!(artwork.contains(&app.world().resource::<StartGame>().background));
        assert_eq!(
            app.world().resource::<StartGame>().artwork_world(),
            Some(destination)
        );
        if destination != 0 {
            assert!(!new_game_artwork.contains(&app.world().resource::<StartGame>().background));
        }
        assert_eq!(
            app.world()
                .resource::<cyrus_world::lifecycle::SceneRequest>()
                .0,
            destination.to_string()
        );
        assert_eq!(
            *app.world()
                .resource::<cyrus_script::player::placement::MarkerEntry>(),
            default()
        );
        app.update();
        assert_eq!(
            *app.world().resource::<State<AppState>>().get(),
            AppState::Loading
        );
        assert!(app.world().get_entity(old_player).is_err());
        assert!(!app.world().get::<Camera>(camera).unwrap().is_active);
        app.world_mut()
            .resource_mut::<NextState<AppState>>()
            .set(AppState::InGame);
        app.update();
        let new_player = app
            .world_mut()
            .query_filtered::<Entity, With<Player>>()
            .single(app.world())
            .unwrap();
        assert_ne!(old_player, new_player);
        assert!(app.world().get::<Camera>(camera).unwrap().is_active);
    }

    #[test]
    fn world_loading_draws_only_the_bar_and_percentage() {
        let mut images = Assets::<Image>::default();
        let background = [17, 18, 19, 255].repeat(640 * 480);
        let image = images.add(Image::new(
            bevy::render::render_resource::Extent3d {
                width: 640,
                height: 480,
                depth_or_array_layers: 1,
            },
            bevy::render::render_resource::TextureDimension::D2,
            background.clone(),
            bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
            bevy::asset::RenderAssetUsages::default(),
        ));
        let mut game = StartGame {
            image,
            background: background.clone(),
            font: cyrus_text::Font {
                start: b'%'.into(),
                glyphs: vec![
                    cyrus_text::Glyph {
                        enabled: true,
                        width: 2,
                        height: 1,
                        pixels: vec![1, 1],
                        offset: [0, 6],
                    };
                    usize::from(b'9' - b'%' + 1)
                ],
                palette: vec![[0; 3], [63, 39, 1]],
                ..default()
            },
            ..default()
        };
        update_progress(&mut game, &mut images, 0, 6);
        assert_eq!(
            images.get(&game.image).unwrap().data.as_ref().unwrap(),
            &background
        );
        update_progress(&mut game, &mut images, 1, 6);
        let pixels = images.get(&game.image).unwrap().data.as_ref().unwrap();
        // Glyph colour (252, 156, 4) at alpha 180 over the (17, 18, 19) artwork.
        assert_eq!(&pixels[(457 * 640 + 32) * 4..][..4], &[183, 115, 8, 255]);
        assert_eq!(&pixels[(448 * 640 + 128) * 4..][..4], &[216, 20, 16, 255]);
        assert_eq!(&pixels[(448 * 640 + 129) * 4..][..4], &[11, 12, 13, 255]);
        for y in 455..480 {
            assert_eq!(
                &pixels[(y * 640 + 320) * 4..(y * 640 + 640) * 4],
                &background[(y * 640 + 320) * 4..(y * 640 + 640) * 4],
            );
        }
        let before = pixels.clone();
        update_progress(&mut game, &mut images, 0, 6);
        assert_eq!(game.progress, (1, 6));
        assert_eq!(
            images.get(&game.image).unwrap().data.as_ref().unwrap(),
            &before
        );
    }

    #[test]
    fn progress_uses_the_original_rectangle_and_preserves_artwork() {
        let mut pixels = [17, 18, 19, 255].repeat(640 * 480);
        let pixel = |pixels: &[u8], x: usize, y: usize| {
            pixels[(y * 640 + x) * 4..(y * 640 + x + 1) * 4].to_vec()
        };
        paint_progress(&mut pixels, &default(), 0, 2);
        assert_eq!(pixel(&pixels, 32, 448), [17, 18, 19, 255]);
        paint_progress(&mut pixels, &default(), 1, 2);
        assert_eq!(pixel(&pixels, 31, 448), [17, 18, 19, 255]);
        assert_eq!(pixel(&pixels, 32, 447), [17, 18, 19, 255]);
        assert_eq!(pixel(&pixels, 320, 448), [216, 20, 16, 255]);
        assert_eq!(pixel(&pixels, 321, 448), [11, 12, 13, 255]);
        assert_eq!(pixel(&pixels, 608, 454), [11, 12, 13, 255]);
        assert_eq!(pixel(&pixels, 609, 454), [17, 18, 19, 255]);
        assert_eq!(pixel(&pixels, 608, 455), [17, 18, 19, 255]);
        assert_eq!(pixel(&pixels, 32, 455), [17, 18, 19, 255]);
        // Each update paints onto a fresh copy of the artwork, as `update_progress` does.
        let mut pixels = [17, 18, 19, 255].repeat(640 * 480);
        paint_progress(&mut pixels, &default(), 1, 6);
        assert_eq!(pixel(&pixels, 128, 448), [216, 20, 16, 255]);
        assert_eq!(pixel(&pixels, 129, 448), [11, 12, 13, 255]);
        let mut pixels = [17, 18, 19, 255].repeat(640 * 480);
        paint_progress(&mut pixels, &default(), 2, 2);
        assert_eq!(pixel(&pixels, 608, 454), [216, 20, 16, 255]);
        assert_eq!(pixel(&pixels, 609, 454), [17, 18, 19, 255]);
    }
}
