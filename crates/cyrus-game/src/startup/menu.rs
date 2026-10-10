//! MENU.INI-derived presentation data, shared by native and browser builds.
use super::{StartupContent, fit_image};
use crate::AppState;
use bevy::{
    asset::{AssetLoader, LoadContext, io::Reader},
    prelude::*,
};
use serde::{Deserialize, Serialize};

#[derive(Asset, TypePath, Serialize, Deserialize)]
pub(super) struct MenuAsset {
    pub entries: Vec<MenuItem>,
    pub images: Vec<String>,
    pub selected: Option<usize>,
    pub repeat_seconds: f32,
    pub activation_key: Option<KeyCode>,
}

/// One installed entry. Action IDs select handlers independently of labels.
#[derive(Clone, Serialize, Deserialize)]
pub struct MenuItem {
    pub text: String,
    pub action: i32,
    pub selectable: bool,
    pub grayed: bool,
    pub implemented: bool,
}

impl MenuItem {
    fn eligible(&self) -> bool {
        self.selectable && !self.grayed && !self.text.is_empty()
    }
}

/// Current installed menu selection and explicitly unsupported activation.
#[derive(Resource, Default, Serialize)]
pub struct MenuState {
    /// True once every selection image is loaded and input can be accepted.
    pub ready: bool,
    pub entries: Vec<MenuItem>,
    pub selected: Option<usize>,
    pub unavailable: Option<String>,
    #[serde(skip)]
    images: Vec<Handle<Image>>,
    #[serde(skip)]
    asset: Handle<MenuAsset>,
    #[serde(skip)]
    repeat_seconds: f32,
    #[serde(skip)]
    held_seconds: f32,
    #[serde(skip)]
    activation_key: Option<KeyCode>,
}

impl MenuState {
    fn navigate(&mut self, direction: isize) {
        let Some(selected) = self.selected else {
            return;
        };
        let len = self.entries.len();
        for step in 1..=len {
            let index =
                (selected as isize + direction * step as isize).rem_euclid(len as isize) as usize;
            if self.entries[index].eligible() {
                self.selected = Some(index);
                self.unavailable = None;
                return;
            }
        }
    }
}

#[derive(Default, TypePath)]
struct MenuLoader;
impl AssetLoader for MenuLoader {
    type Asset = MenuAsset;
    type Settings = ();
    type Error = anyhow::Error;
    async fn load(
        &self,
        reader: &mut dyn Reader,
        _: &(),
        _: &mut LoadContext<'_>,
    ) -> Result<MenuAsset, Self::Error> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        let menu: MenuAsset = serde_json::from_slice(&bytes)?;
        anyhow::ensure!(
            !menu.entries.is_empty() && menu.images.len() == menu.entries.len(),
            "menu: invalid entry/image count"
        );
        anyhow::ensure!(
            menu.repeat_seconds.is_finite() && menu.repeat_seconds > 0.0,
            "menu: invalid repeat delay"
        );
        anyhow::ensure!(
            menu.selected
                .is_none_or(|index| menu.entries.get(index).is_some_and(MenuItem::eligible)),
            "menu: invalid initial selection"
        );
        anyhow::ensure!(
            menu.images.iter().all(|name| name.starts_with("menu/")
                && !name.contains("..")
                && !name.contains('\\')),
            "menu: invalid image path"
        );
        Ok(menu)
    }
    fn extensions(&self) -> &[&str] {
        &["menu"]
    }
}

pub(super) fn register(app: &mut App) {
    app.init_asset::<MenuAsset>()
        .init_asset_loader::<MenuLoader>()
        .add_systems(OnEnter(AppState::Menu), setup)
        .add_systems(Update, (fit_image, drive).run_if(in_state(AppState::Menu)))
        .add_systems(OnExit(AppState::Menu), cleanup);
}

fn setup(mut commands: Commands, assets: Res<AssetServer>) {
    commands.spawn((Camera2d, StartupContent));
    commands.spawn((Sprite::default(), StartupContent));
    commands.insert_resource(MenuState {
        asset: assets.load("cache://menu/main.menu"),
        ..default()
    });
}

fn drive(
    assets: Res<AssetServer>,
    definitions: Res<Assets<MenuAsset>>,
    input: (Res<Time>, Res<ButtonInput<KeyCode>>),
    mut menu: ResMut<MenuState>,
    mut sprite: Query<&mut Sprite, With<StartupContent>>,
    mut output: (MessageWriter<AppExit>, Query<&mut Text, With<MenuNotice>>),
    mut commands: Commands,
) {
    if menu.images.is_empty() {
        if let Some(bevy::asset::LoadState::Failed(error)) = assets.get_load_state(menu.asset.id())
        {
            if menu.unavailable.is_none() {
                let text = format!("Menu assets unavailable: {error}");
                error!("{text}");
                menu.unavailable = Some(text.clone());
                notice(&mut commands, &assets, text);
            }
            return;
        }
        let Some(definition) = definitions.get(&menu.asset) else {
            return;
        };
        menu.entries = definition.entries.clone();
        menu.selected = definition.selected;
        menu.repeat_seconds = definition.repeat_seconds;
        menu.activation_key = definition.activation_key;
        menu.images = definition
            .images
            .iter()
            .map(|name| assets.load(format!("cache://{name}")))
            .collect();
    }
    for handle in &menu.images {
        if let Some(bevy::asset::LoadState::Failed(error)) = assets.get_load_state(handle.id()) {
            if menu.unavailable.is_none() {
                let text = format!("Menu image unavailable: {error}");
                error!("{text}");
                notice(&mut commands, &assets, text.clone());
                menu.unavailable = Some(text);
            }
            return;
        }
        if !assets.is_loaded_with_dependencies(handle.id()) {
            return;
        }
    }
    menu.ready = true;
    let direction = match (
        input.1.pressed(KeyCode::ArrowUp),
        input.1.pressed(KeyCode::ArrowDown),
    ) {
        (true, false) => -1,
        (false, true) => 1,
        _ => 0,
    };
    let pressed =
        input.1.just_pressed(KeyCode::ArrowUp) || input.1.just_pressed(KeyCode::ArrowDown);
    if direction == 0 {
        menu.held_seconds = 0.0;
    } else if pressed {
        menu.navigate(direction);
        menu.held_seconds = 0.0;
    } else {
        menu.held_seconds += input.0.delta_secs();
        if menu.held_seconds >= menu.repeat_seconds {
            menu.navigate(direction);
            menu.held_seconds %= menu.repeat_seconds;
        }
    }
    if let Ok(mut sprite) = sprite.single_mut() {
        sprite.image = menu.images[menu.selected.unwrap_or(0)].clone();
    }
    if (input.1.just_pressed(KeyCode::Enter)
        || menu
            .activation_key
            .is_some_and(|key| input.1.just_pressed(key)))
        && let Some(index) = menu.selected
    {
        let entry = &menu.entries[index];
        if entry.action == -2 {
            output.0.write(AppExit::Success);
        } else {
            let text = format!("{} is not implemented yet.", entry.text);
            menu.unavailable = Some(text.clone());
            if let Ok(mut message) = output.1.single_mut() {
                **message = text;
            } else {
                notice(&mut commands, &assets, text);
            }
        }
    }
    if menu.unavailable.is_none()
        && let Ok(mut message) = output.1.single_mut()
    {
        message.clear();
    }
}

#[derive(Component)]
struct MenuNotice;
fn notice(commands: &mut Commands, assets: &AssetServer, text: String) {
    commands.spawn((
        StartupContent,
        MenuNotice,
        Text::new(text),
        TextFont {
            font: assets.load("cache://menu/font.ttf").into(),
            font_size: FontSize::Px(20.0),
            ..default()
        },
        Node {
            position_type: PositionType::Absolute,
            bottom: px(8),
            left: px(8),
            ..default()
        },
    ));
}
fn cleanup(mut commands: Commands) {
    commands.remove_resource::<MenuState>();
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn navigation_wraps_and_skips_data_defined_disabled_entries() {
        let mut menu = MenuState {
            entries: vec![
                MenuItem {
                    text: "First".into(),
                    action: 72,
                    selectable: true,
                    grayed: false,
                    implemented: false,
                },
                MenuItem {
                    text: "Disabled".into(),
                    action: 80,
                    selectable: true,
                    grayed: true,
                    implemented: false,
                },
                MenuItem {
                    text: "Last".into(),
                    action: -2,
                    selectable: true,
                    grayed: false,
                    implemented: true,
                },
            ],
            selected: Some(0),
            ..default()
        };
        menu.navigate(1);
        assert_eq!(menu.selected, Some(2));
        menu.navigate(1);
        assert_eq!(menu.selected, Some(0));
        menu.navigate(-1);
        assert_eq!(menu.selected, Some(2));
        menu.entries[2].selectable = false;
        menu.navigate(1);
        assert_eq!(menu.selected, Some(0));
    }
}
