//! Installed GXA images that a running `ViewGXA` or `AnimateGXA` task draws over the world frame.
use bevy::prelude::*;
use cyrus_game::AppState;
use cyrus_script::script::ScriptActors;

#[derive(Component)]
struct Panel;
/// The 640 by 480 screen the slots are placed on.
#[derive(Component)]
struct Screen;
/// One of the two image slots; the second is drawn after, so over, the first.
#[derive(Component)]
struct Picture(usize);

/// Draws the images the script world currently asks for, each centered on the 640 by 480 screen.
pub struct OverlayPlugin;
impl Plugin for OverlayPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(AppState::InGame), setup)
            .add_systems(OnExit(AppState::InGame), cleanup)
            .add_systems(Update, draw.run_if(in_state(AppState::InGame)));
    }
}

fn setup(mut commands: Commands) {
    commands
        .spawn((
            Panel,
            GlobalZIndex(105),
            Visibility::Hidden,
            BackgroundColor(Color::BLACK),
            Node {
                width: percent(100),
                height: percent(100),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
        ))
        .with_children(|panel| {
            panel
                .spawn((
                    Screen,
                    Node {
                        height: percent(100),
                        max_width: percent(100),
                        aspect_ratio: Some(640. / 480.),
                        ..default()
                    },
                ))
                .with_children(|screen| {
                    for slot in 0..2 {
                        screen.spawn((
                            Picture(slot),
                            ImageNode::new(Handle::default()),
                            Visibility::Hidden,
                            Node {
                                position_type: PositionType::Absolute,
                                ..default()
                            },
                        ));
                    }
                });
        });
}

fn cleanup(mut commands: Commands, panels: Query<Entity, With<Panel>>) {
    for panel in &panels {
        commands.entity(panel).despawn();
    }
}

/// An image of the given pixel size centered on the 640 by 480 screen, as fractions of the screen:
/// `(left, top, width, height)`. Odd differences round down, as the original's halving does.
fn placement(width: u32, height: u32) -> (f32, f32, f32, f32) {
    let (left, top) = (
        (640 - width as i32).div_euclid(2),
        (480 - height as i32).div_euclid(2),
    );
    (
        left as f32 / 640.,
        top as f32 / 480.,
        width as f32 / 640.,
        height as f32 / 480.,
    )
}

/// The cache paths of the two slots the script world asks for; an unsupported image is `None`.
fn wanted(overlay: &cyrus_script::script::GxaOverlay) -> [Option<Option<String>>; 2] {
    let first = cyrus_assets::overlay::cache_path(&overlay.name, overlay.frame);
    let second = overlay.second_offset.map(|offset| {
        cyrus_assets::overlay::second_slot_cache_path(&overlay.name, overlay.frame, offset)
    });
    [Some(first), second]
}

fn draw(
    server: Res<AssetServer>,
    images: Res<Assets<Image>>,
    actors: Option<Res<ScriptActors>>,
    mut panels: Query<(&mut Visibility, &mut BackgroundColor), With<Panel>>,
    mut pictures: Query<(&Picture, &mut ImageNode, &mut Node, &mut Visibility), Without<Panel>>,
    mut shown: Local<[Option<Option<String>>; 2]>,
) {
    let overlay = actors
        .as_deref()
        .and_then(|actors| actors.current.as_ref())
        .and_then(|world| world.gxa_overlay());
    let Ok((mut panel, mut background)) = panels.single_mut() else {
        return;
    };
    let Some(overlay) = overlay else {
        *panel = Visibility::Hidden;
        *shown = [None, None];
        return;
    };
    // `ViewGXA` shows one full-screen image on black; `AnimateGXA` draws over the world frame.
    background.0 = if overlay.second_offset.is_some() {
        Color::NONE
    } else {
        Color::BLACK
    };
    let wanted = wanted(overlay);
    let mut any_visible = false;
    for (picture, mut node_image, mut node, mut visibility) in &mut pictures {
        let want = &wanted[picture.0];
        if shown[picture.0] != *want {
            node_image.image = match want {
                Some(Some(path)) => server.load(format!("cache://{path}")),
                Some(None) => {
                    error!(
                        "{}: unsupported image {:?} frame {} slot {}",
                        if overlay.second_offset.is_some() {
                            "AnimateGXA"
                        } else {
                            "ViewGXA"
                        },
                        overlay.name,
                        overlay.frame,
                        picture.0 + 1
                    );
                    Handle::default()
                }
                None => Handle::default(),
            };
            shown[picture.0] = want.clone();
        }
        // The original loads synchronously; wait for the cache rather than flash an empty slot.
        let size = want
            .as_ref()
            .and_then(|_| server.is_loaded(&node_image.image).then_some(()))
            .and_then(|()| images.get(&node_image.image))
            .map(Image::size);
        *visibility = match size {
            Some(size) => {
                let (left, top, width, height) = placement(size.x, size.y);
                node.left = percent(left * 100.);
                node.top = percent(top * 100.);
                node.width = percent(width * 100.);
                node.height = percent(height * 100.);
                any_visible = true;
                Visibility::Visible
            }
            None => Visibility::Hidden,
        };
    }
    *panel = if any_visible {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
}

#[cfg(test)]
mod tests {
    use super::placement;

    #[test]
    fn images_are_centered_on_the_screen_rounding_down() {
        assert_eq!(placement(640, 480), (0., 0., 1., 1.));
        assert_eq!(placement(320, 240), (0.25, 0.25, 0.5, 0.5));
        assert_eq!(
            placement(63, 41),
            (288. / 640., 219. / 480., 63. / 640., 41. / 480.)
        );
    }

    #[test]
    fn installed_images_export_as_full_screen_frames_for_every_script_name() {
        let root = std::env::var_os("REDGUARD_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../redguard")
            });
        let install = match cyrus_assets::install::GameInstall::open(&root) {
            Ok(install) => install,
            Err(error) => return eprintln!("skipped: no install ({error})"),
        };
        let source = cyrus_assets::native::Files::open(&install).unwrap();
        for name in [
            "system\\stmap01.gxa",
            "system\\stmap17.gxa",
            "system\\mirror3.gxa",
        ] {
            let path = cyrus_assets::overlay::cache_path(name, 0).unwrap();
            let files = cyrus_assets::game::prepare(&source, &path).unwrap();
            let image = image::load_from_memory(&files[&path]).unwrap();
            assert_eq!((image.width(), image.height()), (640, 480), "{name}");
        }

        // The second slot shows frame first + offset when the file has it, else first + 1.
        let name = "system\\compass2.gxa";
        let exported =
            |path: String| cyrus_assets::game::prepare(&source, &path).unwrap()[&path].clone();
        let frame = |frame| exported(cyrus_assets::overlay::cache_path(name, frame).unwrap());
        let second = |offset| {
            exported(cyrus_assets::overlay::second_slot_cache_path(name, 3, offset).unwrap())
        };
        assert_eq!(second(2), frame(5));
        assert_eq!(second(100), frame(4));
        assert_ne!(frame(5), frame(4));
    }
}
