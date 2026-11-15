//! Stand-in entities that give Solari the world meshes in the layout it accepts.
//!
//! Each traced source gets traced parts, which Solari traces and no camera draws, and one drawn part
//! per face brightness, which fills the deferred G-buffer. The original adds each face's brightness
//! byte to its ambient light, so the drawn parts give off their albedo scaled by that brightness
//! where they are seen. Most traced faces give off nothing: Solari samples every emissive mesh as a
//! light, uniformly, and thousands of faint ones would starve the sun of samples. Only faces bright
//! enough to read as lamps (`GLOW_FROM`) are traced apart and give off light into the scene, a
//! Modern choice; the original lights only the face itself. Under the day/night cycle the dimmer
//! face brightness fades with the sky, so only lamps glow at night. Meshes too large to rewrite
//! whole on every edit are split into tiles.
use super::super::{
    Daylight, SceneMaterial, TerrainMaterial, clipping::Draw, modern_lighting::lights,
};
use super::{
    StandIn,
    plugin::{Hidden, Traced, unhide},
};
use crate::flat::Flat;
use bevy::{
    asset::{RenderAssetUsages, UntypedAssetId},
    camera::visibility::RenderLayers,
    ecs::{query::QueryData, system::SystemParam},
    material::OpaqueRendererMethod,
    mesh::{Indices, PrimitiveTopology, VertexAttributeValues},
    prelude::*,
    solari::prelude::RaytracingMesh3d,
};
use std::collections::{BTreeMap, HashMap, HashSet, hash_map::Entry};

/// What a stand-in does for its source.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub(super) enum Kind {
    /// The faces as Solari traces them, giving off light of their face brightness when it is at
    /// least `GLOW_FROM`; 0 for faces that give off none.
    Traced(u8),
    /// The faces of one brightness byte, as the camera draws them.
    Drawn(u8),
}

/// Face brightness from which traced faces give off light into the scene, as lantern glass and lit
/// windows do; a Modern tuning choice. The original lights nothing with a face brightness: it only
/// brightens the face itself, as the drawn parts do. The walls' ambient brightening stays below it
/// (60 to 80 in ISLAND), and the lit windows and lamps sit above (96 and up). Solari samples every
/// emissive mesh as a light, uniformly, so the many dimly brightened faces stay dark to keep the sun
/// and fires sampled; ISLAND has about 220 glowing parts at this threshold, which leaves the
/// daylight shadows as clean as with 60.
const GLOW_FROM: u8 = 96;

/// How much brighter a glowing traced face gives off light into the scene than the camera draws
/// it, at night; a Modern tuning choice. Traced parts are not drawn, and a lit window at the
/// brightness it is drawn with is too small a light to show on the street around it.
const TRACED_GLOW_GAIN: f32 = 64.0;

/// The share of `TRACED_GLOW_GAIN` that glowing faces keep in full daylight, where the sun already
/// lights the street and a window should not outshine the wall it sits in; a Modern tuning choice.
const DAY_GLOW_SHARE: f32 = 0.125;

/// Luminance weights of linear sRGB, for the day/night cycle's sky tint.
const LUMINANCE: Vec3 = Vec3::new(0.2126, 0.7152, 0.0722);

impl Kind {
    /// The factor on this kind's emission under the day/night cycle, given `face_glow`: drawn faces
    /// below `GLOW_FROM` dim with the sky, and glowing traced faces light the scene the more the
    /// darker it is. Lamp glass and lit windows keep glowing as drawn at night.
    fn night_factor(self, glow: f32) -> f32 {
        match self {
            Self::Drawn(brightness) if brightness < GLOW_FROM => glow,
            Self::Traced(brightness) if brightness > 0 => {
                DAY_GLOW_SHARE + (1.0 - DAY_GLOW_SHARE) * (1.0 - glow).clamp(0.0, 1.0)
            }
            _ => 1.0,
        }
    }

    /// Whether this kind gives off anything that `night_factor` changes.
    fn follows_night(self) -> bool {
        matches!(self, Self::Drawn(1..GLOW_FROM) | Self::Traced(1..=u8::MAX))
    }
}

/// Strength of the dimming face brightness under the day/night cycle: 1 without it, else the
/// luminance of the cycle's sky tint. The original adds a face's brightness to its ambient light
/// and has no night, so at night it scales with the sky; a Modern tuning choice.
pub(super) fn face_glow(daylight: Option<&Daylight>) -> f32 {
    daylight
        .and_then(|daylight| daylight.0)
        .map_or(1.0, |sky| sky.tint.dot(LUMINANCE))
}

/// `color` with its red, green and blue channels scaled by `factor`.
fn scaled(color: LinearRgba, factor: f32) -> LinearRgba {
    LinearRgba {
        red: color.red * factor,
        green: color.green * factor,
        blue: color.blue * factor,
        ..color
    }
}

/// The traced kind that holds faces of this brightness.
fn traced_kind(face: u8) -> Kind {
    Kind::Traced(if face >= GLOW_FROM { face } else { 0 })
}

/// One stand-in of a source: its kind and the tile of the source faces it holds.
#[derive(Component, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(super) struct Part {
    pub(super) kind: Kind,
    pub(super) tile: u16,
}

impl Part {
    pub(super) const fn new(kind: Kind, tile: u16) -> Self {
        Self { kind, tile }
    }
}

/// Source meshes with more vertices than twice this are split into tiles of about this many.
/// Water animation rewrites the whole terrain mesh every frame, and each rewrite of a stand-in
/// mesh is uploaded and its acceleration structure rebuilt, so only the tiles whose faces moved are.
const TILE_VERTICES: usize = 16_384;

/// The stand-in meshes of one source mesh: each part with its mesh and, for a mesh of its own, its
/// layout, and the vertex and face counts the layouts were worked out for.
#[derive(Default)]
struct PartMeshes {
    vertices: usize,
    faces: usize,
    parts: Vec<(Part, Handle<Mesh>, Option<Layout>)>,
}

impl PartMeshes {
    fn get(&self, part: Part) -> Option<&Handle<Mesh>> {
        self.parts
            .iter()
            .find_map(|(held, handle, _)| (*held == part).then_some(handle))
    }

    fn parts(&self) -> impl Iterator<Item = Part> + use<'_> {
        self.parts.iter().map(|(part, _, _)| *part)
    }

    fn set(&mut self, part: Part, handle: Handle<Mesh>, layout: Option<Layout>) {
        self.parts.retain(|(held, _, _)| *held != part);
        self.parts.push((part, handle, layout));
    }
}

pub(super) struct StandIns {
    meshes: HashMap<AssetId<Mesh>, PartMeshes>,
    /// Each stand-in material, with its emissive colour before the day/night dimming.
    materials: HashMap<(UntypedAssetId, Kind), (Handle<StandardMaterial>, LinearRgba)>,
    /// The current `face_glow`.
    face_glow: f32,
}

impl Default for StandIns {
    fn default() -> Self {
        Self {
            meshes: HashMap::new(),
            materials: HashMap::new(),
            face_glow: 1.0,
        }
    }
}

/// The traceable material of a source: asset identity, base material and solid tint.
/// Meshes whose look depends on shader-side alpha or chroma keying are not traceable.
fn traceable<'a>(
    scene: Option<&MeshMaterial3d<SceneMaterial>>,
    terrain: Option<&MeshMaterial3d<TerrainMaterial>>,
    scene_materials: &'a Assets<SceneMaterial>,
    terrain_materials: &'a Assets<TerrainMaterial>,
) -> Option<(UntypedAssetId, &'a StandardMaterial, Vec4)> {
    let (id, base, solid, keyed) = match (scene, terrain) {
        (Some(handle), _) => {
            let material = scene_materials.get(&handle.0)?;
            (
                handle.0.id().untyped(),
                &material.base,
                material.extension.solid_color,
                material.extension.chroma_key != Vec4::ZERO,
            )
        }
        (None, Some(handle)) => {
            let material = terrain_materials.get(&handle.0)?;
            (handle.0.id().untyped(), &material.base, Vec4::ZERO, false)
        }
        (None, None) => return None,
    };
    (!keyed && matches!(base.alpha_mode, AlphaMode::Opaque)).then_some((id, base, solid))
}

/// Each vertex's face brightness byte. Scene preparation stores it in UV 1's first channel;
/// meshes without it, such as the terrain, have none.
fn face_brightness(source: &Mesh) -> Option<Vec<u8>> {
    match source.attribute(Mesh::ATTRIBUTE_UV_1)? {
        VertexAttributeValues::Float32x2(values) => Some(
            values
                .iter()
                .map(|[brightness, _]| brightness.clamp(0.0, 255.0) as u8)
                .collect(),
        ),
        _ => None,
    }
}

/// The faces of `source` as a triangle list over its vertices, if Solari can trace it: a triangle
/// list with positions and normals.
fn source_faces(source: &Mesh) -> Option<Vec<u32>> {
    if source.primitive_topology() != PrimitiveTopology::TriangleList
        || source
            .attribute(Mesh::ATTRIBUTE_POSITION)?
            .as_float3()
            .is_none()
        || source
            .attribute(Mesh::ATTRIBUTE_NORMAL)?
            .as_float3()
            .is_none()
    {
        return None;
    }
    Some(match source.indices() {
        Some(indices) => indices.iter().map(|index| index as u32).collect(),
        None => (0..source.count_vertices() as u32).collect(),
    })
}

/// The number of face indices of `source`, without collecting them.
fn face_count(source: &Mesh) -> usize {
    source
        .indices()
        .map_or_else(|| source.count_vertices(), Indices::len)
}

/// Which source vertices and faces a stand-in mesh holds. Edits in place, posing and water, change
/// vertex values but never which vertices and faces a part holds, so the layout is worked out once
/// per source mesh and an edit only compares and copies vertex values.
#[derive(Clone, PartialEq, Debug)]
struct Layout {
    /// The source vertex of each stand-in vertex; `None` for all of them, in order.
    vertices: Option<Vec<u32>>,
    /// The faces, over the stand-in vertices.
    indices: Vec<u32>,
}

impl Layout {
    /// The faces `indices` of a source mesh, keeping all its vertices where `whole`, else only
    /// the vertices they use.
    fn of(indices: Vec<u32>, whole: bool) -> Self {
        if whole {
            return Self {
                vertices: None,
                indices,
            };
        }
        let mut remap = HashMap::new();
        let mut used = Vec::new();
        let indices = indices
            .into_iter()
            .map(|index| {
                *remap.entry(index).or_insert_with(|| {
                    used.push(index);
                    (used.len() - 1) as u32
                })
            })
            .collect();
        Self {
            vertices: Some(used),
            indices,
        }
    }

    fn gather<T: Copy>(&self, values: &[T]) -> Vec<T> {
        match &self.vertices {
            Some(vertices) => vertices
                .iter()
                .map(|&vertex| values[vertex as usize])
                .collect(),
            None => values.to_vec(),
        }
    }

    /// The stand-in mesh, in exactly the layout Solari accepts: position, normal, UV 0 and
    /// tangent, as an indexed triangle list. Tangents are zero because no material here has a
    /// normal map.
    fn mesh(&self, source: &Mesh) -> Option<Mesh> {
        let positions = self.gather(source.attribute(Mesh::ATTRIBUTE_POSITION)?.as_float3()?);
        let normals = self.gather(source.attribute(Mesh::ATTRIBUTE_NORMAL)?.as_float3()?);
        let count = positions.len();
        let uvs = match source.attribute(Mesh::ATTRIBUTE_UV_0) {
            Some(VertexAttributeValues::Float32x2(uvs)) => self.gather(uvs),
            _ => vec![[0.0; 2]; count],
        };
        Some(
            Mesh::new(
                PrimitiveTopology::TriangleList,
                RenderAssetUsages::default(),
            )
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
            .with_inserted_attribute(Mesh::ATTRIBUTE_TANGENT, vec![[0.0; 4]; count])
            .with_inserted_indices(Indices::U32(self.indices.clone())),
        )
    }

    /// Whether `held` already has the vertex values of `source` this layout selects. Rewriting an
    /// unchanged stand-in would upload it and rebuild its acceleration structure for nothing.
    fn holds(&self, source: &Mesh, held: &Mesh) -> bool {
        fn matches<T: PartialEq>(layout: &Layout, source: &[T], held: &[T]) -> bool {
            match &layout.vertices {
                Some(vertices) => {
                    vertices.len() == held.len()
                        && vertices
                            .iter()
                            .zip(held)
                            .all(|(&vertex, held)| source.get(vertex as usize) == Some(held))
                }
                None => source == held,
            }
        }
        fn float3(mesh: &Mesh, attribute: bevy::mesh::MeshVertexAttribute) -> &[[f32; 3]] {
            mesh.attribute(attribute)
                .and_then(VertexAttributeValues::as_float3)
                .unwrap_or_default()
        }
        fn uvs(mesh: &Mesh) -> Option<&[[f32; 2]]> {
            match mesh.attribute(Mesh::ATTRIBUTE_UV_0) {
                Some(VertexAttributeValues::Float32x2(uvs)) => Some(uvs),
                _ => None,
            }
        }
        let held_uvs = uvs(held).unwrap_or_default();
        let uvs_match = match uvs(source) {
            Some(source) => matches(self, source, held_uvs),
            None => held_uvs.iter().all(|uv| *uv == [0.0; 2]),
        };
        // Positions first: they are what edits change.
        matches(
            self,
            float3(source, Mesh::ATTRIBUTE_POSITION),
            float3(held, Mesh::ATTRIBUTE_POSITION),
        ) && matches(
            self,
            float3(source, Mesh::ATTRIBUTE_NORMAL),
            float3(held, Mesh::ATTRIBUTE_NORMAL),
        ) && uvs_match
    }
}

/// The stand-in parts of `source` and the layout of each: per tile, the traced faces, glowing ones
/// apart, and the faces grouped by face brightness, one drawn part per brightness. A face's
/// brightness is that of its first vertex; all its vertices share it. A drawn brightness alone in
/// its traced part draws that part's mesh (`None`), so it uploads once per edit of its source.
fn layouts(source: &Mesh) -> Option<Vec<(Part, Option<Layout>)>> {
    let indices = source_faces(source)?;
    let positions = source.attribute(Mesh::ATTRIBUTE_POSITION)?.as_float3()?;
    let brightness = face_brightness(source);
    let tiles = Tiles::of(positions);
    // Tile, then brightness, then that group's triangles.
    let mut groups = BTreeMap::<u16, BTreeMap<u8, Vec<u32>>>::new();
    for triangle in indices.as_chunks::<3>().0 {
        let face = brightness
            .as_ref()
            .and_then(|brightness| brightness.get(triangle[0] as usize).copied())
            .unwrap_or(0);
        groups
            .entry(tiles.of_triangle(positions, triangle))
            .or_default()
            .entry(face)
            .or_default()
            .extend_from_slice(triangle);
    }
    let whole = tiles.count == 1;
    let mut parts = Vec::new();
    for (tile, faces) in groups {
        // Glowing faces are traced apart, one part per brightness, so they can give off light.
        let mut traced_faces = BTreeMap::<Kind, Vec<u8>>::new();
        for &face in faces.keys() {
            traced_faces
                .entry(traced_kind(face))
                .or_default()
                .push(face);
        }
        for (kind, members) in &traced_faces {
            let indices = if traced_faces.len() == 1 && whole {
                indices.clone()
            } else {
                members
                    .iter()
                    .flat_map(|face| faces[face].clone())
                    .collect()
            };
            parts.push((Part::new(*kind, tile), Some(Layout::of(indices, whole))));
        }
        for (face, indices) in faces {
            let alone = traced_faces[&traced_kind(face)].len() == 1;
            let layout = (!alone).then(|| Layout::of(indices, whole));
            parts.push((Part::new(Kind::Drawn(face), tile), layout));
        }
    }
    Some(parts)
}

/// A square grid over the horizontal extent of a mesh, fine enough for tiles of about
/// `TILE_VERTICES`; one tile for smaller meshes.
struct Tiles {
    count: u16,
    side: u16,
    min: Vec2,
    size: Vec2,
}

impl Tiles {
    fn of(positions: &[[f32; 3]]) -> Self {
        let side = if positions.len() > 2 * TILE_VERTICES {
            ((positions.len() as f32 / TILE_VERTICES as f32)
                .sqrt()
                .ceil() as u16)
                .max(1)
        } else {
            1
        };
        let (min, max) = positions.iter().fold(
            (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN)),
            |(min, max), [x, _, z]| (min.min(Vec2::new(*x, *z)), max.max(Vec2::new(*x, *z))),
        );
        Self {
            count: side * side,
            side,
            min,
            size: (max - min).max(Vec2::splat(f32::EPSILON)),
        }
    }

    /// The tile of a triangle, by its centroid. Water moves vertices only vertically, so a face
    /// keeps its tile while the terrain animates.
    fn of_triangle(&self, positions: &[[f32; 3]], triangle: &[u32; 3]) -> u16 {
        if self.count == 1 {
            return 0;
        }
        let centroid = triangle.iter().fold(Vec2::ZERO, |sum, &index| {
            let [x, _, z] = positions[index as usize];
            sum + Vec2::new(x, z)
        }) / 3.0;
        let last = f32::from(self.side) - 1.0;
        let cell = ((centroid - self.min) / self.size * f32::from(self.side))
            .clamp(Vec2::ZERO, Vec2::splat(last));
        cell.y as u16 * self.side + cell.x as u16
    }
}

/// Brings the held parts of one source to `built`, its fresh layouts: writes each part whose mesh
/// no longer holds its source values into its handle, adds new parts and drops vanished ones. A
/// drawn part without a layout shares the traced mesh of its tile. Returns whether parts were
/// added or dropped.
fn rebuild(
    meshes: &mut Assets<Mesh>,
    held: &mut PartMeshes,
    source: &Mesh,
    built: Vec<(Part, Option<Layout>)>,
) -> bool {
    held.vertices = source.count_vertices();
    held.faces = face_count(source);
    let wanted: HashSet<Part> = built.iter().map(|(part, _)| *part).collect();
    let before = held.parts.len();
    held.parts.retain(|(part, _, _)| wanted.contains(part));
    let mut regrouped = held.parts.len() != before;
    // Traced parts first, so drawn parts can share them.
    let (traced, drawn): (Vec<_>, Vec<_>) = built
        .into_iter()
        .partition(|(part, _)| matches!(part.kind, Kind::Traced(_)));
    for (part, layout) in traced.into_iter().chain(drawn) {
        let shared = match part.kind {
            Kind::Drawn(face) => held.get(Part::new(traced_kind(face), part.tile)).cloned(),
            Kind::Traced(_) => None,
        };
        let current = held.get(part).cloned();
        let handle = match (layout, current) {
            (None, current) if current == shared => continue,
            (None, _) => match shared {
                Some(shared) => (shared, None),
                None => continue,
            },
            (Some(layout), Some(current)) if Some(&current) != shared.as_ref() => {
                if !meshes
                    .get(&current)
                    .is_some_and(|mesh| layout.holds(source, mesh))
                    && let Some(mesh) = layout.mesh(source)
                    && let Err(error) = meshes.insert(current.id(), mesh)
                {
                    warn!("ray tracing stand-in mesh {}: {error}", current.id());
                }
                held.set(part, current, Some(layout));
                continue;
            }
            (Some(layout), _) => {
                let Some(mesh) = layout.mesh(source) else {
                    continue;
                };
                (meshes.add(mesh), Some(layout))
            }
        };
        held.set(part, handle.0, handle.1);
        regrouped = true;
    }
    regrouped
}

/// Rewrites the parts of one source edited in place whose meshes no longer hold its vertex values,
/// in their handles. False when the edit changed the vertex or face count, so the layouts no
/// longer fit and the source needs [`rebuild`].
fn refresh(meshes: &mut Assets<Mesh>, held: &PartMeshes, id: AssetId<Mesh>) -> bool {
    let Some(source) = meshes.get(id) else {
        return true;
    };
    if source.count_vertices() != held.vertices || face_count(source) != held.faces {
        return false;
    }
    let stale: Vec<_> = held
        .parts
        .iter()
        .filter_map(|(_, handle, layout)| {
            let layout = layout.as_ref()?;
            let fresh = meshes
                .get(handle)
                .is_some_and(|mesh| layout.holds(source, mesh));
            (!fresh).then(|| Some((handle.id(), layout.mesh(source)?)))?
        })
        .collect();
    for (handle, mesh) in stale {
        if let Err(error) = meshes.insert(handle, mesh) {
            warn!("ray tracing stand-in mesh {handle}: {error}");
        }
    }
    true
}

/// The stand-in material of a part of `kind`, before the day/night dimming. Every part shares
/// colour and texture, so traced bounces see the colours the camera shows. A drawn part gives off
/// its albedo times its face brightness over 255, the original's additive face brightness,
/// calibrated as the modern lighting's light factors are; a glowing traced part gives off
/// `TRACED_GLOW_GAIN` times that, into the scene.
fn standin_material(base: &StandardMaterial, solid: Vec4, kind: Kind) -> StandardMaterial {
    let base_color = if solid.w != 0.0 {
        Color::srgb(solid.x / 256.0, solid.y / 256.0, solid.z / 256.0)
    } else {
        base.base_color
    };
    let glow = match kind {
        Kind::Drawn(brightness) if brightness > 0 => f32::from(brightness) / 255.0,
        Kind::Traced(brightness) if brightness > 0 => {
            f32::from(brightness) / 255.0 * TRACED_GLOW_GAIN
        }
        _ => 0.0,
    } / lights::exposure();
    let albedo = LinearRgba::from(base_color);
    StandardMaterial {
        base_color,
        base_color_texture: base.base_color_texture.clone(),
        emissive: LinearRgba::rgb(albedo.red * glow, albedo.green * glow, albedo.blue * glow),
        emissive_texture: (glow > 0.0)
            .then(|| base.base_color_texture.clone())
            .flatten(),
        cull_mode: base.cull_mode,
        double_sided: base.double_sided,
        // The original has no specular term.
        perceptual_roughness: 1.0,
        reflectance: 0.0,
        // Solari shades the deferred G-buffer.
        opaque_render_method: OpaqueRendererMethod::Deferred,
        ..default()
    }
}

/// The asset stores a stand-in is built from, all four needed together, and the day/night tint
/// the drawn parts' face brightness follows.
#[derive(SystemParam)]
pub(super) struct StandInAssets<'w> {
    meshes: ResMut<'w, Assets<Mesh>>,
    standard: ResMut<'w, Assets<StandardMaterial>>,
    scene: Res<'w, Assets<SceneMaterial>>,
    terrain: Res<'w, Assets<TerrainMaterial>>,
    daylight: Option<Res<'w, Daylight>>,
}

impl StandIns {
    /// Animated and re-prepared source meshes are edited in place; rebuild their stand-ins into
    /// the same handles, writing only the parts whose faces changed. Returns the source meshes whose
    /// parts were added or dropped, whose sources need their stand-ins reconciled.
    fn rebuild_edited(
        &mut self,
        meshes: &mut Assets<Mesh>,
        events: &mut MessageReader<AssetEvent<Mesh>>,
    ) -> HashSet<AssetId<Mesh>> {
        let edited: HashSet<_> = events
            .read()
            .filter_map(|event| match event {
                AssetEvent::Modified { id } => Some(*id),
                _ => None,
            })
            .collect();
        let mut regrouped = HashSet::new();
        for id in edited {
            let Some(held) = self.meshes.get_mut(&id) else {
                continue;
            };
            if refresh(meshes, held, id) {
                continue;
            }
            // The vertex or face count changed: work the layouts out again.
            let Some(source) = meshes.get(id).cloned() else {
                continue;
            };
            let Some(built) = layouts(&source) else {
                continue;
            };
            if rebuild(meshes, held, &source, built) {
                regrouped.insert(id);
            }
        }
        self.meshes.retain(|id, _| meshes.contains(*id));
        regrouped
    }

    /// The stand-in meshes of a source mesh, built on first use. Posing swaps a source's mesh for
    /// another asset, which gets its own.
    fn meshes(&mut self, meshes: &mut Assets<Mesh>, source: &Handle<Mesh>) -> Option<&PartMeshes> {
        match self.meshes.entry(source.id()) {
            Entry::Occupied(entry) => Some(entry.into_mut()),
            Entry::Vacant(entry) => {
                let source = meshes.get(source)?.clone();
                let built = layouts(&source)?;
                let mut held = PartMeshes::default();
                rebuild(meshes, &mut held, &source, built);
                Some(entry.insert(held))
            }
        }
    }

    /// The stand-in material shared by every part of this kind using one source material.
    fn material(
        &mut self,
        standard: &mut Assets<StandardMaterial>,
        id: UntypedAssetId,
        kind: Kind,
        base: &StandardMaterial,
        solid: Vec4,
    ) -> Handle<StandardMaterial> {
        let dimmed = |mut material: StandardMaterial, glow: f32| {
            material.emissive = scaled(material.emissive, kind.night_factor(glow));
            material
        };
        let (handle, _) = self
            .materials
            .entry((id, kind))
            .or_insert_with(|| {
                let material = standin_material(base, solid, kind);
                let emissive = material.emissive;
                (standard.add(dimmed(material, self.face_glow)), emissive)
            })
            .clone();
        // Texture animation edits the source material's base texture in place.
        if standard
            .get(&handle)
            .is_some_and(|stand_in| stand_in.base_color_texture != base.base_color_texture)
            && let Some(mut stand_in) = standard.get_mut(&handle)
        {
            *stand_in = dimmed(standin_material(base, solid, kind), self.face_glow);
        }
        handle
    }

    /// Brings the emission of every part to the day/night cycle's `glow` (see `face_glow`).
    pub(super) fn follow_daylight(&mut self, standard: &mut Assets<StandardMaterial>, glow: f32) {
        if self.face_glow == glow {
            return;
        }
        self.face_glow = glow;
        for ((_, kind), (handle, emissive)) in &self.materials {
            if kind.follows_night()
                && let Some(mut material) = standard.get_mut(handle)
            {
                material.emissive = scaled(*emissive, kind.night_factor(glow));
            }
        }
    }
}

/// A mesh the stand-ins replace, with the change ticks of everything its parts follow.
#[derive(QueryData)]
pub(super) struct Source {
    entity: Entity,
    mesh: Ref<'static, Mesh3d>,
    global: Ref<'static, GlobalTransform>,
    inherited: Ref<'static, InheritedVisibility>,
    scene: Option<Ref<'static, MeshMaterial3d<SceneMaterial>>>,
    terrain: Option<Ref<'static, MeshMaterial3d<TerrainMaterial>>>,
    traced: Has<Traced>,
    hidden: Option<&'static Hidden>,
    layers: Option<Ref<'static, RenderLayers>>,
}

impl SourceItem<'_, '_> {
    /// The scene or terrain material the source draws with.
    fn material_id(&self) -> Option<UntypedAssetId> {
        self.scene
            .as_ref()
            .map(|handle| handle.0.id().untyped())
            .or_else(|| self.terrain.as_ref().map(|handle| handle.0.id().untyped()))
    }

    /// Whether any component the parts follow changed since the last run.
    fn changed(&self) -> bool {
        self.mesh.is_changed()
            || self.global.is_changed()
            || self.inherited.is_changed()
            || self.layers.as_ref().is_some_and(Ref::is_changed)
            || self.scene.as_ref().is_some_and(Ref::is_changed)
            || self.terrain.as_ref().is_some_and(Ref::is_changed)
    }
}
type SourceFilter = (
    Without<StandIn>,
    Without<Draw>,
    Without<Flat>,
    Or<(
        With<MeshMaterial3d<SceneMaterial>>,
        With<MeshMaterial3d<TerrainMaterial>>,
    )>,
);
type PartData = (
    &'static Part,
    &'static mut Transform,
    &'static mut GlobalTransform,
    Option<&'static mut Mesh3d>,
    Option<&'static mut RaytracingMesh3d>,
    &'static mut MeshMaterial3d<StandardMaterial>,
);

/// The parts a source should have: each part's mesh and material.
type Wanted = HashMap<Part, (Handle<Mesh>, Handle<StandardMaterial>)>;

/// The asset edits a stand-in follows: source meshes posed or re-prepared in place, and source
/// materials whose texture animates.
#[derive(SystemParam)]
pub(super) struct SourceEdits<'w, 's> {
    meshes: MessageReader<'w, 's, AssetEvent<Mesh>>,
    scene: MessageReader<'w, 's, AssetEvent<SceneMaterial>>,
    terrain: MessageReader<'w, 's, AssetEvent<TerrainMaterial>>,
}

impl SourceEdits<'_, '_> {
    /// The source materials edited since the last read.
    fn materials(&mut self) -> HashSet<UntypedAssetId> {
        fn modified<A: Asset>(event: &AssetEvent<A>) -> Option<UntypedAssetId> {
            match event {
                AssetEvent::Modified { id } => Some(id.untyped()),
                _ => None,
            }
        }
        self.scene
            .read()
            .filter_map(modified)
            .chain(self.terrain.read().filter_map(modified))
            .collect()
    }
}

/// Reconciles the stand-ins of every source that changed: its transform, mesh, visibility,
/// render layers or material, or its stand-in meshes. Unchanged sources cost a few flag checks.
pub(super) fn sync_proxies(
    mut commands: Commands,
    mut assets: StandInAssets,
    mut edits: SourceEdits,
    sources: Query<Source, SourceFilter>,
    mut parts: Query<PartData, With<StandIn>>,
    mut state: Local<(StandIns, HashMap<Entity, Vec<Entity>>)>,
) {
    let (stand_ins, index) = &mut *state;
    stand_ins.follow_daylight(&mut assets.standard, face_glow(assets.daylight.as_deref()));
    let regrouped = stand_ins.rebuild_edited(&mut assets.meshes, &mut edits.meshes);
    let edited_materials = edits.materials();
    // Sources that went away take their stand-ins with them.
    index.retain(|source, entities| {
        let live = sources.contains(*source);
        if !live {
            for entity in entities.iter() {
                commands.entity(*entity).try_despawn();
            }
        }
        live
    });
    for source in &sources {
        let entity = source.entity;
        let indexed = index
            .get(&entity)
            .is_some_and(|entities| entities.iter().all(|part| parts.contains(*part)));
        let dirty = !indexed
            || source.changed()
            || regrouped.contains(&source.mesh.0.id())
            || source
                .material_id()
                .is_some_and(|id| edited_materials.contains(&id));
        if !dirty {
            continue;
        }
        let wanted = source
            .inherited
            .get()
            .then(|| {
                wanted_parts(
                    stand_ins,
                    &mut assets,
                    &source.mesh.0,
                    source.scene.as_deref(),
                    source.terrain.as_deref(),
                )
            })
            .flatten();
        let Some(wanted) = wanted else {
            // An empty entry records that the source wants no parts, so it is not re-checked
            // every frame until one of its inputs changes.
            for part in index.insert(entity, Vec::new()).into_iter().flatten() {
                commands.entity(part).try_despawn();
            }
            if source.traced
                && let Some(hidden) = source.hidden
            {
                unhide(&mut commands, entity, hidden);
            }
            continue;
        };
        let entities = index.entry(entity).or_default();
        reconcile(&mut commands, &mut parts, *source.global, wanted, entities);
        // Hide the source's own draw, and hide it again when another module draws it, as the
        // clipping swap for modern lighting does, keeping the layers that module chose.
        let layers = source.layers.as_deref();
        if !source.traced || layers != Some(&RenderLayers::none()) {
            commands
                .entity(entity)
                .insert((Traced, Hidden(layers.cloned()), RenderLayers::none()));
        }
    }
}

/// The parts a visible source should have, or `None` when it is not traced.
fn wanted_parts(
    stand_ins: &mut StandIns,
    assets: &mut StandInAssets,
    source: &Handle<Mesh>,
    scene: Option<&MeshMaterial3d<SceneMaterial>>,
    terrain: Option<&MeshMaterial3d<TerrainMaterial>>,
) -> Option<Wanted> {
    let (id, base, solid) = traceable(scene, terrain, &assets.scene, &assets.terrain)?;
    let meshes: Vec<_> = {
        let meshes = stand_ins.meshes(&mut assets.meshes, source)?;
        meshes
            .parts()
            .filter_map(|part| Some((part, meshes.get(part)?.clone())))
            .collect()
    };
    Some(
        meshes
            .into_iter()
            .map(|(part, mesh)| {
                let material = stand_ins.material(&mut assets.standard, id, part.kind, base, solid);
                (part, (mesh, material))
            })
            .collect(),
    )
}

/// Brings one source's parts to `wanted`: follows its transform, swaps meshes and materials,
/// despawns parts no longer wanted and spawns the missing ones.
fn reconcile(
    commands: &mut Commands,
    parts: &mut Query<PartData, With<StandIn>>,
    source_global: GlobalTransform,
    mut wanted: Wanted,
    entities: &mut Vec<Entity>,
) {
    entities.retain(|&entity| {
        let Ok((part, mut transform, mut global, drawn, traced, mut material)) =
            parts.get_mut(entity)
        else {
            return false;
        };
        let Some((mesh, wanted_material)) = wanted.remove(part) else {
            commands.entity(entity).despawn();
            return false;
        };
        if *global != source_global {
            *global = source_global;
            *transform = source_global.compute_transform();
        }
        if let Some(mut drawn) = drawn
            && drawn.0 != mesh
        {
            drawn.0 = mesh.clone();
        }
        if let Some(mut traced) = traced
            && traced.0 != mesh
        {
            traced.0 = mesh;
        }
        if material.0 != wanted_material {
            material.0 = wanted_material;
        }
        true
    });
    for (part, (mesh, material)) in wanted {
        let mut entity = commands.spawn((
            StandIn,
            part,
            MeshMaterial3d(material),
            source_global.compute_transform(),
            source_global,
        ));
        match part.kind {
            Kind::Traced(_) => entity.insert(RaytracingMesh3d(mesh)),
            Kind::Drawn(_) => entity.insert(Mesh3d(mesh)),
        };
        entities.push(entity.id());
    }
}

/// Clipped model draws are children of their source; they stop drawing with it, and stay hidden if
/// another module draws them again.
#[allow(clippy::type_complexity)] // Bevy query filters.
pub(super) fn hide_draws(
    mut commands: Commands,
    draws: Query<(Entity, &ChildOf, Option<&RenderLayers>, Has<Hidden>), With<Draw>>,
    traced: Query<(), With<Traced>>,
) {
    for (draw, parent, layers, hidden) in &draws {
        let drawn = layers != Some(&RenderLayers::none());
        if traced.contains(parent.parent()) && (drawn || !hidden) {
            commands
                .entity(draw)
                .insert((Hidden(layers.cloned()), RenderLayers::none()));
        }
    }
}

#[cfg(test)]
mod tests;
