//! Progress overlays driven by the host, without application transitions.
use bevy::prelude::*;

/// Progress has no application-state transition or filesystem dependency.
#[derive(Resource, Default, Clone)]
pub struct LoadingProgress {
    pub visible: bool,
    pub label: String,
    pub fraction: Option<f32>,
}
#[cfg(not(target_arch = "wasm32"))]
#[derive(Component)]
struct LoadingUi;
#[cfg(not(target_arch = "wasm32"))]
#[derive(Component)]
struct Bar;
#[cfg(not(target_arch = "wasm32"))]
#[derive(Component)]
struct Label;
pub struct LoadingOverlayPlugin;
impl Plugin for LoadingOverlayPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LoadingProgress>()
            .add_systems(PostUpdate, loading);
    }
}
#[cfg(target_arch = "wasm32")]
fn loading(progress: Res<LoadingProgress>) {
    if !progress.is_changed() {
        return;
    }
    let Some(document) = web_sys::window().and_then(|window| window.document()) else {
        return;
    };
    if let Some(root) = document.get_element_by_id("loading") {
        let _ = root.set_attribute("style", if progress.visible { "" } else { "display:none" });
    }
    if let Some(label) = document.get_element_by_id("label") {
        label.set_text_content(Some(&progress.label));
    }
    if let Some(fill) = document.get_element_by_id("fill") {
        let _ = fill.set_attribute(
            "class",
            if progress.fraction.is_none() {
                "indeterminate"
            } else {
                ""
            },
        );
        let _ = fill.set_attribute(
            "style",
            &format!(
                "width:{}%",
                progress.fraction.unwrap_or(0.).clamp(0., 1.) * 100.
            ),
        );
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn loading(
    mut commands: Commands,
    progress: Res<LoadingProgress>,
    time: Res<Time>,
    roots: Query<Entity, With<LoadingUi>>,
    mut bars: Query<&mut Node, With<Bar>>,
    mut labels: Query<&mut Text, With<Label>>,
) {
    if !progress.is_changed() && !(progress.visible && progress.fraction.is_none()) {
        return;
    }
    if !progress.visible {
        for entity in &roots {
            commands.entity(entity).despawn();
        }
        return;
    }
    if roots.is_empty() {
        commands.spawn((
            LoadingUi,
            GlobalZIndex(100),
            Node {
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                row_gap: px(12),
                ..default()
            },
            BackgroundColor(Color::BLACK),
            children![
                (
                    Label,
                    Text::new(&progress.label),
                    TextFont {
                        font_size: FontSize::Px(18.),
                        ..default()
                    }
                ),
                (
                    Node {
                        width: px(400),
                        height: px(16),
                        border: UiRect::all(px(1)),
                        ..default()
                    },
                    BorderColor::all(Color::WHITE),
                    children![(
                        Bar,
                        Node {
                            width: percent(
                                progress.fraction.map_or(25., |fraction| fraction * 100.)
                            ),
                            height: percent(100),
                            ..default()
                        },
                        BackgroundColor(Color::WHITE)
                    )]
                )
            ],
        ));
    } else {
        for mut bar in &mut bars {
            bar.width = percent(progress.fraction.map_or(25., |fraction| fraction * 100.));
            bar.margin.left = percent(if progress.fraction.is_none() {
                (time.elapsed_secs() * 0.75).sin().mul_add(0.5, 0.5) * 75.
            } else {
                0.
            });
        }
        for mut label in &mut labels {
            label.0.clone_from(&progress.label);
        }
    }
}
