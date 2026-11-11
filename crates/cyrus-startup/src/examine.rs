//! The examine view's model over the suspended world. Pose, camera and lights follow
//! preservation's `engine/hud.md` examine view and `engine/model-lighting.md`; the textures are
//! decoded here with the loaded world's palette.
use super::view3d::{LitAssets, LitFace, LitModel, LitScene, target_image, view_camera};
use bevy::{
    asset::{AssetLoader, LoadContext, io::Reader},
    prelude::*,
};
use cyrus_assets::examine::{ExamineArtwork, ExamineModel};
use cyrus_game::AppState;
use cyrus_script::item_viewer::{ExamineLook, ExamineScreen, ViewerPose};

#[derive(Default, TypePath)]
struct Loader;
impl AssetLoader for Loader {
    type Asset = ExamineArtwork;
    type Settings = ();
    type Error = anyhow::Error;
    async fn load(
        &self,
        reader: &mut dyn Reader,
        _: &(),
        _: &mut LoadContext<'_>,
    ) -> anyhow::Result<Self::Asset> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        let artwork: ExamineArtwork = serde_json::from_slice(&bytes)?;
        artwork.validate()?;
        Ok(artwork)
    }
    fn extensions(&self) -> &[&str] {
        &["models"]
    }
}

#[derive(Resource)]
struct Artwork(Handle<ExamineArtwork>);
#[derive(Component)]
struct Panel;
#[derive(Component)]
struct Canvas;

/// Presentation of [`ExamineScreen`]; adding it switches the view on.
pub struct ExaminePlugin;
impl Plugin for ExaminePlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<ExamineArtwork>()
            .init_asset_loader::<Loader>()
            .init_resource::<ExamineScreen>()
            .add_systems(OnEnter(AppState::InGame), setup)
            .add_systems(OnExit(AppState::InGame), cleanup)
            .add_systems(Update, draw.run_if(in_state(AppState::InGame)));
    }
}

/// Render layer of the examine view's scene.
const EXAMINE_LAYER: usize = 7;

#[derive(Component)]
struct ExamineCamera;

/// The panel and camera `cleanup` removes.
type Teardown = Or<(With<Panel>, With<ExamineCamera>)>;

fn setup(
    mut commands: Commands,
    server: Res<AssetServer>,
    mut screen: ResMut<ExamineScreen>,
    mut images: ResMut<Assets<Image>>,
) {
    commands.insert_resource(Artwork(server.load("cache://hud/examine.models")));
    screen.enabled = true;
    let target = images.add(target_image([0, 0, 0, 0]));
    commands.spawn((
        ExamineCamera,
        view_camera(target.clone(), EXAMINE_LAYER, Color::NONE),
    ));
    commands
        .spawn((
            Panel,
            GlobalZIndex(105),
            Visibility::Hidden,
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
                Canvas,
                ImageNode::new(target),
                Node {
                    height: percent(100),
                    max_width: percent(100),
                    aspect_ratio: Some(640. / 480.),
                    ..default()
                },
            ));
        });
}

fn cleanup(
    mut commands: Commands,
    panels: Query<Entity, Teardown>,
    mut screen: ResMut<ExamineScreen>,
) {
    for entity in &panels {
        commands.entity(entity).despawn();
    }
    screen.enabled = false;
}

/// The scene's textures change with the model and the world palette only.
fn texture_key(name: &str, look: &ExamineLook) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    name.hash(&mut hasher);
    look.palette.hash(&mut hasher);
    hasher.finish()
}

#[allow(clippy::too_many_arguments)] // Reads one asset and updates its presentation node and scene.
fn draw(
    artwork: Option<Res<Artwork>>,
    assets: Res<Assets<ExamineArtwork>>,
    mut screen: ResMut<ExamineScreen>,
    mut commands: Commands,
    mut assets_3d: LitAssets,
    mut panels: Query<&mut Visibility, With<Panel>>,
    mut cameras: Query<&mut Camera, With<ExamineCamera>>,
    mut model: Local<LitModel>,
    mut previous: Local<Option<(String, ViewerPose)>>,
) {
    let Ok(mut panel) = panels.single_mut() else {
        return;
    };
    *panel = if screen.open {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
    for mut camera in &mut cameras {
        camera.is_active = screen.open;
    }
    if !screen.open {
        *previous = None;
        model.clear(&mut commands);
        return;
    }
    let (Some(artwork), Some(name), Some(pose)) = (artwork, screen.model.clone(), screen.pose)
    else {
        return;
    };
    let current = (name.clone(), pose);
    if previous.as_ref() == Some(&current) && model.is_shown() {
        return;
    }
    let (Some(art), Some(look)) = (assets.get(&artwork.0), screen.look.as_ref()) else {
        return;
    };
    let built = art
        .models
        .get(&name)
        .ok_or_else(|| anyhow::anyhow!("examine: model {name} is not in the exported artwork"))
        .and_then(|found| build_scene(art, found, &pose, look));
    match built {
        Ok(scene) => model.show(
            &mut commands,
            &mut assets_3d,
            &scene,
            EXAMINE_LAYER,
            texture_key(&name, look),
        ),
        Err(error) => {
            warn!("examine view: {error:#}");
            screen.error = Some(format!("{error:#}"));
            // Retrying every update would repeat the failure; keep the request's pose unchanged.
        }
    }
    *previous = Some(current);
}

type Matrix = [[f32; 3]; 3];

fn multiply(a: &Matrix, b: &Matrix) -> Matrix {
    std::array::from_fn(|row| {
        std::array::from_fn(|column| (0..3).map(|k| a[row][k] * b[k][column]).sum())
    })
}

/// The model's orientation: turn about the vertical axis, then pitch about the horizontal axis,
/// then roll about the view axis, each multiplied onto the identity from the right (2048 angle
/// units per turn).
fn orientation(pose: &ViewerPose) -> Matrix {
    let sin_cos = |angle: u16| {
        let radians = f64::from(angle & 0x7ff) * std::f64::consts::TAU / 2048.0;
        (radians.sin() as f32, radians.cos() as f32)
    };
    let (s, c) = sin_cos(pose.turn);
    let turn = [[c, 0.0, s], [0.0, 1.0, 0.0], [-s, 0.0, c]];
    let (s, c) = sin_cos(pose.pitch);
    let pitch = [[1.0, 0.0, 0.0], [0.0, c, -s], [0.0, s, c]];
    let (s, c) = sin_cos(pose.roll);
    let roll = [[c, -s, 0.0], [s, c, 0.0], [0.0, 0.0, 1.0]];
    multiply(&multiply(&turn, &pitch), &roll)
}

/// The three camera-space lights the view registers, each with radius 6400 and an RGB strength.
const LIGHTS: [([f32; 3], [f32; 3]); 3] = [
    ([0.0, 42768.0, 65535.0], [180.0; 3]),
    ([0.0, -256.0, -200.0], [255.0; 3]),
    ([0.0, 256.0, -200.0], [255.0; 3]),
];
const LIGHT_RADIUS: f32 = 6400.0;
/// Directional strength scale; the light colour is stored 0 to 255.
const DIRECTIONAL_SCALE: f32 = 1.0;

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// The lighting level of each channel at one corner, clamped to 0..255: ambient plus the face
/// brightness plus the directional term plus each selected local light (preservation
/// `engine/model-lighting.md`). `vertex` and `normal` are in model space; `matrix` maps model to
/// world axes and `origin` is the model's position.
#[allow(clippy::too_many_arguments)] // One calculation over the fixed lighting inputs.
fn corner_level(
    look: &ExamineLook,
    matrix: &Matrix,
    origin: [f32; 3],
    radius: f32,
    vertex: [f32; 3],
    normal: [f32; 3],
    brightness: u8,
) -> [f32; 3] {
    let transpose = |v: [f32; 3]| -> [f32; 3] {
        std::array::from_fn(|i| (0..3).map(|j| matrix[j][i] * v[j]).sum())
    };
    let length = dot(look.direction, look.direction).sqrt();
    let direction = if length > 0.0 {
        transpose(look.direction.map(|c| c / length))
    } else {
        [0.0; 3]
    };
    let facing = (-dot(direction, normal)).max(0.0) * DIRECTIONAL_SCALE;
    let mut level: [f32; 3] = std::array::from_fn(|channel| {
        look.ambient[channel] + f32::from(brightness) + facing * look.directional[channel]
    });
    let numerator = (LIGHT_RADIUS * LIGHT_RADIUS + 3.0 * LIGHT_RADIUS) / 256.0;
    for (position, rgb) in LIGHTS {
        let offset = std::array::from_fn(|axis| position[axis] - origin[axis]);
        if dot(offset, offset) - radius * radius > LIGHT_RADIUS * LIGHT_RADIUS {
            continue;
        }
        let light = transpose(offset);
        let away: [f32; 3] = std::array::from_fn(|axis| vertex[axis] - light[axis]);
        let q = dot(away, away);
        let d = q.sqrt();
        if d == 0.0 {
            continue;
        }
        let weight = (-dot(away.map(|c| c / d), normal)).max(0.0) * numerator / (q + 3.0 * d);
        for channel in 0..3 {
            level[channel] += weight * rgb[channel];
        }
    }
    level.map(|c| c.clamp(0.0, 255.0))
}

/// The model at `pose` as lit triangles in view space, its textures decoded with the world palette.
fn build_scene(
    artwork: &ExamineArtwork,
    model: &ExamineModel,
    pose: &ViewerPose,
    look: &ExamineLook,
) -> anyhow::Result<LitScene> {
    anyhow::ensure!(
        look.palette.len() == 256,
        "examine: the world palette is absent from this cache; re-export the world"
    );
    let matrix = orientation(pose);
    let origin = pose.position;
    let mut scene = LitScene::default();
    let mut local = std::collections::BTreeMap::new();
    for face in &model.faces {
        let (texture, bias) = match face.texture {
            Some(index) => {
                let texture = *local.entry(index).or_insert_with(|| {
                    let source = &artwork.textures[index];
                    let rgba = source
                        .indices
                        .iter()
                        .flat_map(|&entry| {
                            let [r, g, b] = look.palette[usize::from(entry)];
                            [r, g, b, if entry == 0 { 0 } else { 255 }]
                        })
                        .collect();
                    scene.textures.push((source.width, source.height, rgba));
                    scene.textures.len() - 1
                });
                (texture, 1.0)
            }
            None => {
                let colour = face.solid.map_or([0; 4], |index| {
                    let [r, g, b] = look.palette[usize::from(index)];
                    [r, g, b, 255]
                });
                (scene.solid_texture(colour), 0.0)
            }
        };
        let corners: Vec<([f32; 3], [f32; 2], [f32; 3])> = face
            .corners
            .iter()
            .zip(&face.normals)
            .map(|(corner, normal)| {
                let vertex = model.vertices[corner.vertex];
                let world: [f32; 3] = std::array::from_fn(|row| {
                    (0..3).map(|k| matrix[row][k] * vertex[k]).sum::<f32>() + origin[row]
                });
                let level = corner_level(
                    look,
                    &matrix,
                    origin,
                    model.radius,
                    vertex,
                    *normal,
                    face.brightness,
                );
                (world, corner.uv, level)
            })
            .collect();
        for index in 1..corners.len().saturating_sub(1) {
            let triangle = [corners[0], corners[index], corners[index + 1]];
            scene.faces.push(LitFace {
                vertices: triangle.map(|corner| corner.0),
                uv: triangle.map(|corner| corner.1),
                levels: triangle.map(|corner| corner.2),
                bias,
                texture,
            });
        }
    }
    Ok(scene)
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use cyrus_assets::{
        examine::{ExamineFace, ExamineTexture},
        menu::MenuCorner,
    };

    fn look() -> ExamineLook {
        ExamineLook {
            ambient: [64.0; 3],
            directional: [0.0; 3],
            direction: [0.0, 0.0, 1.0],
            palette: (0..=255).map(|i| [i as u8, 0, 255 - i as u8]).collect(),
        }
    }

    fn quad(brightness: u8) -> (ExamineArtwork, ExamineModel) {
        let corner = |vertex, u, v| MenuCorner { vertex, uv: [u, v] };
        let model = ExamineModel {
            radius: 100.0,
            vertices: vec![
                [-50.0, -50.0, 0.0],
                [50.0, -50.0, 0.0],
                [50.0, 50.0, 0.0],
                [-50.0, 50.0, 0.0],
            ],
            faces: vec![ExamineFace {
                corners: vec![
                    corner(0, 0.0, 0.0),
                    corner(3, 0.0, 4.0),
                    corner(2, 4.0, 4.0),
                    corner(1, 4.0, 0.0),
                ],
                texture: Some(0),
                solid: None,
                normals: vec![[0.0, 0.0, 1.0]; 4],
                brightness,
            }],
        };
        let artwork = ExamineArtwork {
            models: Default::default(),
            textures: vec![ExamineTexture {
                width: 4,
                height: 4,
                indices: vec![200; 16],
            }],
        };
        (artwork, model)
    }

    fn pose() -> ViewerPose {
        ViewerPose {
            turn: 0,
            pitch: 0,
            roll: 0,
            position: [0.0, 0.0, 400.0],
        }
    }

    #[test]
    fn the_orientation_is_the_identity_at_zero_and_a_half_turn_flips_the_view_axis() {
        let identity = orientation(&pose());
        for (row, values) in identity.iter().enumerate() {
            for (column, value) in values.iter().enumerate() {
                assert!((value - f32::from(u8::from(row == column))).abs() < 1e-6);
            }
        }
        let half = orientation(&ViewerPose {
            turn: 1024,
            ..pose()
        });
        assert!((half[0][0] + 1.0).abs() < 1e-5 && (half[2][2] + 1.0).abs() < 1e-5);
    }

    /// Screen position of a view-space point under the original's projection.
    fn project(point: [f32; 3]) -> [f32; 2] {
        let scale = 320.0 / 0.788;
        [
            320.0 + point[0] / point[2] * scale,
            240.0 + point[1] / point[2] * scale,
        ]
    }

    /// Signed screen area of a triangle; only negative area (screen Y down) is visible.
    fn area(face: &LitFace) -> f32 {
        let [a, b, c] = face.vertices.map(project);
        (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
    }

    #[test]
    fn a_textured_model_draws_through_the_world_palette_and_projects_about_the_centre() {
        let (artwork, model) = quad(0);
        let scene = build_scene(&artwork, &model, &pose(), &look()).unwrap();
        // Palette entry 200 is (200, 0, 55). The normals face away from the lights, so only the
        // ambient level 64 (+1 for a textured face) over 256 reaches it.
        assert_eq!(scene.textures[0].2[..4], [200, 0, 55, 255]);
        assert_eq!(scene.faces.len(), 2);
        assert!(scene.faces.iter().all(|face| face.bias == 1.0));
        assert_eq!(scene.faces[0].levels[0], [64.0; 3]);
        // 100 units across at depth 400 span about 100 / 400 * 406 = 101 pixels about the centre.
        let xs: Vec<f32> = scene
            .faces
            .iter()
            .flat_map(|face| face.vertices.map(|vertex| project(vertex)[0]))
            .collect();
        assert!((xs.iter().cloned().fold(f32::MAX, f32::min) - 269.0).abs() < 1.0);
        assert!((xs.iter().cloned().fold(f32::MIN, f32::max) - 371.0).abs() < 1.0);
        assert!(scene.faces.iter().all(|face| area(face) < 0.0));
        // The face brightness byte adds to the ambient level.
        let (artwork, model) = quad(100);
        let bright = build_scene(&artwork, &model, &pose(), &look()).unwrap();
        assert!(bright.faces[0].levels[0][0] > 64.0);
    }

    #[test]
    fn a_face_turned_towards_the_registered_lights_is_lit_by_them() {
        let (artwork, mut model) = quad(0);
        let away = build_scene(&artwork, &model, &pose(), &look()).unwrap();
        model.faces[0].normals = vec![[0.0, 0.0, -1.0]; 4];
        let towards = build_scene(&artwork, &model, &pose(), &look()).unwrap();
        assert!(towards.faces[0].levels[0][0] > away.faces[0].levels[0][0] + 50.0);
    }

    /// Every installed inventory model projects visible geometry at its resting pose with the
    /// installed extents, so a draw that culls or misplaces real geometry is caught.
    #[test]
    fn every_installed_inventory_model_is_visible_at_its_resting_pose() {
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
        let files = cyrus_assets::game::prepare(&source, "hud/overlay.hud").unwrap();
        let artwork: ExamineArtwork = serde_json::from_slice(&files["hud/examine.models"]).unwrap();
        artwork.validate().unwrap();
        let hud: serde_json::Value = serde_json::from_slice(&files["hud/overlay.hud"]).unwrap();
        let look = ExamineLook {
            ambient: [64.0; 3],
            directional: [128.0; 3],
            direction: [0.3, -0.6, 0.7],
            palette: (0..=255).map(|i| [i as u8; 3]).collect(),
        };
        let mut drawn = 0;
        for (id, item) in hud["inventory"].as_object().unwrap() {
            let Some(name) = item["inventory_model"]["Model"].as_str() else {
                continue;
            };
            let extents = item["inventory_extents"].as_array().unwrap();
            let depth = extents[2].as_i64().unwrap() as f32 + 180.0;
            let pose = ViewerPose {
                position: [0.0, 0.0, depth],
                ..pose()
            };
            let scene = build_scene(&artwork, &artwork.models[name], &pose, &look).unwrap();
            let visible: f32 = scene
                .faces
                .iter()
                .map(area)
                .filter(|area| *area < 0.0)
                .map(|area| -area)
                .sum();
            assert!(
                visible > 200.0,
                "item {id} model {name}: {visible} visible square pixels"
            );
            drawn += 1;
        }
        assert!(drawn > 20, "only {drawn} models drawn");
    }

    #[test]
    fn faces_with_positive_screen_winding_are_culled() {
        let (artwork, mut model) = quad(0);
        model.faces[0].corners.reverse();
        model.faces[0].normals.reverse();
        let scene = build_scene(&artwork, &model, &pose(), &look()).unwrap();
        assert!(scene.faces.iter().all(|face| area(face) > 0.0));
    }

    #[test]
    fn a_missing_world_palette_is_a_contextual_error() {
        let (artwork, model) = quad(0);
        let mut look = look();
        look.palette.clear();
        let error = build_scene(&artwork, &model, &pose(), &look).err().unwrap();
        assert!(format!("{error:#}").contains("re-export the world"));
    }
}
