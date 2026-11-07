//! The examine view's model over the suspended world. Pose, camera and lights follow
//! preservation's `engine/hud.md` examine view and `engine/model-lighting.md`; the textures are
//! decoded here with the loaded world's palette.
use bevy::{
    asset::{AssetLoader, LoadContext, RenderAssetUsages, io::Reader},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use cyrus_assets::{
    examine::{ExamineArtwork, ExamineModel},
    menu::MenuFace,
};
use cyrus_game::AppState;
use cyrus_script::item_viewer::{ExamineLook, ExamineScreen, ViewerPose};
use image::{Rgba, RgbaImage};

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

fn setup(mut commands: Commands, server: Res<AssetServer>, mut screen: ResMut<ExamineScreen>) {
    commands.insert_resource(Artwork(server.load("cache://hud/examine.models")));
    screen.enabled = true;
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

fn cleanup(
    mut commands: Commands,
    panels: Query<Entity, With<Panel>>,
    mut screen: ResMut<ExamineScreen>,
) {
    for panel in &panels {
        commands.entity(panel).despawn();
    }
    screen.enabled = false;
}

#[allow(clippy::too_many_arguments)] // Reads one asset and updates its presentation node and image.
fn draw(
    artwork: Option<Res<Artwork>>,
    assets: Res<Assets<ExamineArtwork>>,
    mut screen: ResMut<ExamineScreen>,
    mut images: ResMut<Assets<Image>>,
    mut panels: Query<&mut Visibility, With<Panel>>,
    mut canvases: Query<&mut ImageNode, With<Canvas>>,
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
    if !screen.open {
        *previous = None;
        return;
    }
    let (Some(artwork), Some(name), Some(pose)) = (artwork, screen.model.clone(), screen.pose)
    else {
        return;
    };
    let current = (name.clone(), pose);
    if previous.as_ref() == Some(&current)
        && canvases
            .single()
            .is_ok_and(|canvas| canvas.image != Handle::default())
    {
        return;
    }
    let (Some(art), Some(look)) = (assets.get(&artwork.0), screen.look.as_ref()) else {
        return;
    };
    let painted = art
        .models
        .get(&name)
        .ok_or_else(|| anyhow::anyhow!("examine: model {name} is not in the exported artwork"))
        .and_then(|model| paint(art, model, &pose, look));
    match painted {
        Ok(pixels) => {
            let image = Image::new(
                Extent3d {
                    width: 640,
                    height: 480,
                    depth_or_array_layers: 1,
                },
                TextureDimension::D2,
                pixels.into_raw(),
                TextureFormat::Rgba8UnormSrgb,
                RenderAssetUsages::default(),
            );
            if let Ok(mut canvas) = canvases.single_mut() {
                if canvas.image == Handle::default() {
                    canvas.image = images.add(image);
                } else if let Some(mut existing) = images.get_mut(&canvas.image) {
                    *existing = image;
                } else {
                    canvas.image = images.add(image);
                }
            }
            *previous = Some(current);
        }
        Err(error) => {
            warn!("examine view: {error:#}");
            screen.error = Some(format!("{error:#}"));
            // Retrying every update would repeat the failure; keep the request's pose unchanged.
            *previous = Some(current);
        }
    }
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

/// Draw one model at `pose` over a transparent 640 by 480 frame.
fn paint(
    artwork: &ExamineArtwork,
    model: &ExamineModel,
    pose: &ViewerPose,
    look: &ExamineLook,
) -> anyhow::Result<RgbaImage> {
    anyhow::ensure!(
        look.palette.len() == 256,
        "examine: the world palette is absent from this cache; re-export the world"
    );
    let matrix = orientation(pose);
    let origin = pose.position;
    let mut textures = Vec::new();
    let mut local = std::collections::BTreeMap::new();
    let mut faces = Vec::new();
    for face in &model.faces {
        let texture = face.texture.map(|index| {
            *local.entry(index).or_insert_with(|| {
                let source = &artwork.textures[index];
                let rgba = source
                    .indices
                    .iter()
                    .flat_map(|&entry| {
                        let [r, g, b] = look.palette[usize::from(entry)];
                        [r, g, b, if entry == 0 { 0 } else { 255 }]
                    })
                    .collect();
                textures.push(
                    RgbaImage::from_raw(u32::from(source.width), u32::from(source.height), rgba)
                        .expect("validated texture dimensions"),
                );
                textures.len() - 1
            })
        });
        let color = face.solid.map_or([0; 4], |index| {
            let [r, g, b] = look.palette[usize::from(index)];
            [r, g, b, 255]
        });
        let corners: Vec<([f32; 5], [f32; 3])> = face
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
                let scale = 320.0 / 0.788;
                (
                    [
                        320.0 + world[0] / world[2] * scale,
                        240.0 + world[1] / world[2] * scale,
                        world[2].recip(),
                        corner.uv[0],
                        corner.uv[1],
                    ],
                    level,
                )
            })
            .collect();
        for index in 1..corners.len().saturating_sub(1) {
            let mut triangle = [corners[0], corners[index], corners[index + 1]];
            // Back-face handling is unrecovered: both sides draw and the depth test resolves them.
            let [a, b, c] = triangle.map(|(vertex, _)| Vec2::new(vertex[0], vertex[1]));
            if (b - a).perp_dot(c - a) >= 0.0 {
                triangle.swap(1, 2);
            }
            faces.push(MenuFace {
                vertices: triangle.map(|(vertex, _)| vertex),
                texture,
                color,
                light: None,
                light_rgb: Some(triangle.map(|(_, level)| level)),
            });
        }
    }
    let mut image = RgbaImage::from_pixel(640, 480, Rgba([0, 0, 0, 0]));
    super::menu_raster::render_into(&mut image, &faces, &textures)?;
    Ok(image)
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
                    corner(1, 4.0, 0.0),
                    corner(2, 4.0, 4.0),
                    corner(3, 0.0, 4.0),
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

    #[test]
    fn a_textured_model_draws_through_the_world_palette_and_projects_about_the_centre() {
        let (artwork, model) = quad(0);
        let image = paint(&artwork, &model, &pose(), &look()).unwrap();
        // 100 units across at depth 400 span about 100 / 400 * 406 = 101 pixels.
        let centre = image.get_pixel(320, 240).0;
        assert_eq!(centre[3], 255);
        assert_eq!(image.get_pixel(5, 5).0[3], 0);
        assert_eq!(image.get_pixel(420, 240).0[3], 0);
        assert_eq!(image.get_pixel(270, 240).0[3], 255);
        // Palette entry 200 is (200, 0, 55). The normals face away from the lights, so only the
        // ambient level 64 (+1) over 256 reaches it.
        assert_eq!(centre[0], (200 * 65 / 256) as u8);
        assert_eq!(centre[2], (55 * 65 / 256) as u8);
        // The face brightness byte adds to the ambient level.
        let (artwork, model) = quad(100);
        let bright = paint(&artwork, &model, &pose(), &look()).unwrap();
        assert!(bright.get_pixel(320, 240).0[0] > centre[0]);
    }

    #[test]
    fn a_face_turned_towards_the_registered_lights_is_lit_by_them() {
        let (artwork, mut model) = quad(0);
        let away = paint(&artwork, &model, &pose(), &look()).unwrap();
        model.faces[0].normals = vec![[0.0, 0.0, -1.0]; 4];
        let towards = paint(&artwork, &model, &pose(), &look()).unwrap();
        assert!(towards.get_pixel(320, 240).0[0] > away.get_pixel(320, 240).0[0] + 50);
    }

    /// Every installed inventory model draws visibly at its resting pose with the installed
    /// extents, so a draw that culls or misplaces real geometry is caught.
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
            let image = paint(&artwork, &artwork.models[name], &pose, &look).unwrap();
            let visible = image.pixels().filter(|pixel| pixel.0[3] != 0).count();
            assert!(
                visible > 100,
                "item {id} model {name}: {visible} visible pixels"
            );
            drawn += 1;
        }
        assert!(drawn > 20, "only {drawn} models drawn");
    }

    #[test]
    fn a_missing_world_palette_is_a_contextual_error() {
        let (artwork, model) = quad(0);
        let mut look = look();
        look.palette.clear();
        let error = paint(&artwork, &model, &pose(), &look).unwrap_err();
        assert!(format!("{error:#}").contains("re-export the world"));
    }
}
