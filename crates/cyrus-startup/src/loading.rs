//! The installed title stays visible until menu or world assets are ready.
use super::{StartGame, StartupContent, StartupPresentation};
use bevy::world_serialization::WorldInstance;
use bevy::{asset::RecursiveDependencyLoadState, prelude::*};
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
    clock: Option<Res<'w, cyrus_game::clock::SimulationClock>>,
    fades: Query<'w, 's, &'static super::fade::Fade>,
}

pub(super) fn register(app: &mut App) {
    app.add_systems(Update, poll.run_if(in_state(AppState::Loading)))
        .add_systems(Update, super::fade::advance)
        .add_systems(OnEnter(AppState::InGame), fade_in_world)
        .add_systems(OnExit(AppState::Loading), cleanup);
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
                        .load::<cyrus_game::player::data::PlayerScene>(prepared.player_path.clone())
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
    for y in 448..454 {
        for x in 32..608 {
            let at = (y * 640 + x) * 4;
            pixels[at..at + 4].copy_from_slice(if x < 32 + filled {
                &[252, 0, 0, 255]
            } else {
                &[0, 0, 0, 255]
            });
        }
    }
    font.draw_shadowed(pixels, 640, &format!("{percent}%"), [32, 451], [255, 180]);
}

fn cleanup(mut commands: Commands) {
    commands.remove_resource::<WorldAssets>();
    commands.remove_resource::<StartGame>();
}

#[cfg(test)]
mod tests {
    use super::*;
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
        assert_eq!(&pixels[(457 * 640 + 32) * 4..][..4], &[252, 156, 4, 255]);
        assert_eq!(&pixels[(448 * 640 + 127) * 4..][..4], &[252, 0, 0, 255]);
        assert_eq!(&pixels[(448 * 640 + 128) * 4..][..4], &[0, 0, 0, 255]);
        for y in 454..480 {
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
        assert_eq!(pixel(&pixels, 319, 448), [252, 0, 0, 255]);
        assert_eq!(pixel(&pixels, 320, 448), [0, 0, 0, 255]);
        assert_eq!(pixel(&pixels, 607, 453), [0, 0, 0, 255]);
        assert_eq!(pixel(&pixels, 608, 453), [17, 18, 19, 255]);
        assert_eq!(pixel(&pixels, 32, 454), [17, 18, 19, 255]);
        paint_progress(&mut pixels, &default(), 1, 6);
        assert_eq!(pixel(&pixels, 127, 448), [252, 0, 0, 255]);
        assert_eq!(pixel(&pixels, 128, 448), [0, 0, 0, 255]);
        paint_progress(&mut pixels, &default(), 2, 2);
        assert_eq!(pixel(&pixels, 607, 453), [252, 0, 0, 255]);
    }
}
