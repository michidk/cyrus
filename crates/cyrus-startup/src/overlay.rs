//! Installed GXA images that a running `ViewGXA` task draws over the world frame.
use bevy::prelude::*;
use cyrus_game::{AppState, script::ScriptActors};

#[derive(Component)]
struct Panel;
#[derive(Component)]
struct Picture;

/// Draws the image the script world currently asks for, centered on the 640 by 480 screen.
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
        .with_children(|parent| {
            parent.spawn((
                Picture,
                ImageNode::new(Handle::default()),
                Node {
                    height: percent(100),
                    max_width: percent(100),
                    aspect_ratio: Some(640. / 480.),
                    ..default()
                },
            ));
        });
}

fn cleanup(mut commands: Commands, panels: Query<Entity, With<Panel>>) {
    for panel in &panels {
        commands.entity(panel).despawn();
    }
}

fn draw(
    server: Res<AssetServer>,
    actors: Option<Res<ScriptActors>>,
    mut panels: Query<&mut Visibility, With<Panel>>,
    mut pictures: Query<&mut ImageNode, With<Picture>>,
    mut shown: Local<Option<(String, i32)>>,
) {
    let wanted = actors
        .as_deref()
        .and_then(|actors| actors.current.as_ref())
        .and_then(|world| world.gxa_overlay())
        .map(|overlay| (overlay.name.clone(), overlay.frame));
    let (Ok(mut visibility), Ok(mut picture)) = (panels.single_mut(), pictures.single_mut()) else {
        return;
    };
    let Some(wanted) = wanted else {
        *visibility = Visibility::Hidden;
        *shown = None;
        return;
    };
    if shown.as_ref() != Some(&wanted) {
        match cyrus_assets::overlay::cache_path(&wanted.0, wanted.1) {
            Some(path) => picture.image = server.load(format!("cache://{path}")),
            None => {
                error!(
                    "ViewGXA: unsupported image {:?} frame {}",
                    wanted.0, wanted.1
                );
                picture.image = Handle::default();
            }
        }
        *shown = Some(wanted);
    }
    // The original loads synchronously; wait for the cache rather than flash an empty panel.
    *visibility = if server.is_loaded(&picture.image) {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
}

#[cfg(test)]
mod tests {
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
    }
}
