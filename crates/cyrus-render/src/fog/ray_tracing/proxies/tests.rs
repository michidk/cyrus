//! Stand-ins keep Solari's vertex layout, group faces by brightness and follow their sources.
use super::*;

/// The whole of `source` as Solari traces it.
fn standin_mesh(source: &Mesh) -> Option<Mesh> {
    Layout::of(source_faces(source)?, true).mesh(source)
}

/// Each part of `source` with its mesh, or `None` where it draws its traced mesh.
fn part_meshes(source: &Mesh) -> Option<Vec<(Part, Option<Mesh>)>> {
    Some(
        layouts(source)?
            .into_iter()
            .map(|(part, layout)| (part, layout.and_then(|layout| layout.mesh(source))))
            .collect(),
    )
}

#[test]
fn stand_in_mesh_has_exactly_the_layout_solari_accepts() {
    let source = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(
        Mesh::ATTRIBUTE_POSITION,
        vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; 3])
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, vec![[1.0; 4]; 3])
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_1, vec![[0.0; 2]; 3])
    .with_inserted_indices(Indices::U16(vec![0, 1, 2]));
    let stand_in = standin_mesh(&source).expect("triangle list with normals");
    let attributes: Vec<_> = stand_in
        .attributes()
        .map(|(attribute, _)| attribute.id)
        .collect();
    assert_eq!(
        attributes,
        [
            Mesh::ATTRIBUTE_POSITION.id,
            Mesh::ATTRIBUTE_NORMAL.id,
            Mesh::ATTRIBUTE_UV_0.id,
            Mesh::ATTRIBUTE_TANGENT.id,
        ]
    );
    assert!(matches!(stand_in.indices(), Some(Indices::U32(indices)) if indices == &[0, 1, 2]));
}

#[test]
fn meshes_without_normals_get_no_stand_in() {
    let source = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0; 3]; 3]);
    assert!(standin_mesh(&source).is_none());
}

/// Three triangles, each with one brightness for all three of its vertices.
fn faces(brightness: [f32; 3]) -> Mesh {
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0; 3]; 9])
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; 9])
    .with_inserted_attribute(
        Mesh::ATTRIBUTE_UV_1,
        brightness
            .iter()
            .flat_map(|b| [[*b, 1.0]; 3])
            .collect::<Vec<_>>(),
    )
}

/// Each part with the faces it holds, or `None` where it draws its traced mesh.
fn layout(mesh: &Mesh) -> Vec<(Kind, Option<Vec<usize>>)> {
    part_meshes(mesh)
        .expect("triangle list with normals")
        .into_iter()
        .map(|(part, mesh)| {
            let faces =
                mesh.map(|mesh| mesh.indices().map_or_else(Vec::new, |i| i.iter().collect()));
            (part.kind, faces)
        })
        .collect()
}

#[test]
fn faces_are_drawn_in_groups_of_one_brightness() {
    // Two dim brightnesses share the traced part, which gives off no light; each is drawn apart.
    assert_eq!(
        layout(&faces([60.0, 80.0, 60.0])),
        [
            (Kind::Traced(0), Some(vec![0, 1, 2, 3, 4, 5, 6, 7, 8])),
            (Kind::Drawn(60), Some(vec![0, 1, 2, 6, 7, 8])),
            (Kind::Drawn(80), Some(vec![3, 4, 5])),
        ]
    );
}

#[test]
fn glowing_faces_are_traced_apart_and_draw_their_traced_mesh() {
    assert_eq!(
        layout(&faces([60.0, 252.0, 60.0])),
        [
            (Kind::Traced(0), Some(vec![0, 1, 2, 6, 7, 8])),
            (Kind::Traced(252), Some(vec![3, 4, 5])),
            (Kind::Drawn(60), None),
            (Kind::Drawn(252), None),
        ]
    );
}

#[test]
fn meshes_without_face_brightness_draw_in_one_dark_group() {
    let source = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0; 3]; 3])
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; 3]);
    let parts = part_meshes(&source).expect("triangle list with normals");
    let drawn: Vec<_> = parts
        .iter()
        .filter_map(|(part, mesh)| match part.kind {
            Kind::Drawn(b) => Some((b, mesh.is_some())),
            Kind::Traced(_) => None,
        })
        .collect();
    assert_eq!(
        drawn,
        [(0, false)],
        "one group draws the traced mesh itself"
    );
}

#[test]
fn only_drawn_parts_give_off_their_face_brightness() {
    let base = StandardMaterial {
        base_color: Color::srgb(0.5, 0.5, 0.5),
        ..default()
    };
    let traced = standin_material(&base, Vec4::ZERO, Kind::Traced(0));
    assert_eq!(traced.emissive, LinearRgba::BLACK);
    assert_eq!(
        standin_material(&base, Vec4::ZERO, Kind::Traced(255))
            .emissive
            .to_vec3(),
        standin_material(&base, Vec4::ZERO, Kind::Drawn(255))
            .emissive
            .to_vec3()
            * TRACED_GLOW_GAIN,
        "glowing traced faces light the scene more brightly than they are drawn"
    );
    assert_eq!(
        standin_material(&base, Vec4::ZERO, Kind::Drawn(0)).emissive,
        LinearRgba::BLACK
    );
    let drawn = standin_material(&base, Vec4::ZERO, Kind::Drawn(255));
    // Full brightness adds the albedo once, as a full light factor does.
    let expected = LinearRgba::from(base.base_color) / lights::exposure();
    assert!((drawn.emissive.red - expected.red).abs() < 1e-3 * expected.red);
    assert_eq!(drawn.base_color, traced.base_color);
}

// Under the day/night cycle the walls' ambient face brightness fades with the sky, lit windows and
// lamps keep their drawn glow, and glowing traced faces light the scene the more the darker it is.
#[test]
fn face_brightness_follows_the_day_night_sky() {
    let mut standard = Assets::<StandardMaterial>::default();
    let base = StandardMaterial {
        base_color: Color::srgb(0.5, 0.5, 0.5),
        ..default()
    };
    let id = AssetId::<StandardMaterial>::default().untyped();
    let mut stand_ins = StandIns::default();
    let material = |stand_ins: &mut StandIns, standard: &mut Assets<StandardMaterial>, kind| {
        let handle = stand_ins.material(standard, id, kind, &base, Vec4::ZERO);
        standard.get(&handle).unwrap().emissive
    };
    let wall = material(&mut stand_ins, &mut standard, Kind::Drawn(72));
    let window = material(&mut stand_ins, &mut standard, Kind::Drawn(120));
    let lamp = material(&mut stand_ins, &mut standard, Kind::Traced(252));
    let night = Daylight(Some(crate::fog::SkyLight {
        tint: Vec3::splat(0.25),
        sun_rotation: Quat::IDENTITY,
        sun_disc: 0.0,
    }));
    assert_eq!(face_glow(None), 1.0);
    let glow = face_glow(Some(&night));
    assert!((glow - 0.25).abs() < 1e-4);
    stand_ins.follow_daylight(&mut standard, glow);
    assert_eq!(
        material(&mut stand_ins, &mut standard, Kind::Drawn(72)),
        scaled(wall, glow)
    );
    assert_eq!(
        material(&mut stand_ins, &mut standard, Kind::Drawn(120)),
        window,
        "a lit window keeps its glow at night"
    );
    // By day a glowing traced face gives off only its share, and a quarter-lit sky gives it more.
    let lit = material(&mut stand_ins, &mut standard, Kind::Traced(252));
    let ratio = lit.red / lamp.red;
    let expected = (DAY_GLOW_SHARE + (1.0 - DAY_GLOW_SHARE) * (1.0 - glow)) / DAY_GLOW_SHARE;
    assert!(
        (ratio - expected).abs() < 1e-3 * expected,
        "{ratio} vs {expected}"
    );
    // A material first made at night is dimmed too, and everything comes back in full by day.
    let late = material(&mut stand_ins, &mut standard, Kind::Drawn(60));
    stand_ins.follow_daylight(&mut standard, face_glow(None));
    let restored = material(&mut stand_ins, &mut standard, Kind::Drawn(60));
    assert!((restored.to_vec3() * glow).abs_diff_eq(late.to_vec3(), 1e-3));
    assert_eq!(
        material(&mut stand_ins, &mut standard, Kind::Drawn(72)),
        wall
    );
    assert_eq!(
        material(&mut stand_ins, &mut standard, Kind::Traced(252)),
        lamp
    );
}

fn terrain_material() -> TerrainMaterial {
    use super::super::super::{FogExtension, TerrainExtension};
    TerrainMaterial {
        base: StandardMaterial::default(),
        extension: TerrainExtension {
            fog: FogExtension {
                color: Vec4::ZERO,
                clipping: Vec4::ZERO,
                table: [Vec4::ZERO; 16],
                water: Vec4::ZERO,
                ambient: Vec4::ZERO,
                directional: Vec4::ZERO,
                direction: Vec4::ZERO,
                lighting: Vec4::ZERO,
            },
        },
    }
}

fn source_mesh(brightness: [f32; 2]) -> Mesh {
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0; 3]; 6])
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; 6])
    .with_inserted_attribute(
        Mesh::ATTRIBUTE_UV_1,
        [
            brightness[0],
            brightness[0],
            brightness[0],
            brightness[1],
            brightness[1],
            brightness[1],
        ]
        .map(|b| [b, 1.0])
        .to_vec(),
    )
}

fn parts(app: &mut App) -> Vec<(Entity, Part)> {
    let mut parts: Vec<_> = app
        .world_mut()
        .query_filtered::<(Entity, &Part), With<StandIn>>()
        .iter(app.world())
        .map(|(entity, part)| (entity, *part))
        .collect();
    parts.sort_by_key(|(_, part)| format!("{part:?}"));
    parts
}

#[test]
fn a_source_gets_its_parts_keeps_them_while_unchanged_and_loses_them_when_hidden() {
    let mut app = App::new();
    app.init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<StandardMaterial>>()
        .init_resource::<Assets<SceneMaterial>>()
        .init_resource::<Assets<TerrainMaterial>>()
        .add_message::<AssetEvent<Mesh>>()
        .add_message::<AssetEvent<SceneMaterial>>()
        .add_message::<AssetEvent<TerrainMaterial>>()
        .add_systems(Update, (sync_proxies, hide_draws.after(sync_proxies)));
    let mesh = app
        .world_mut()
        .resource_mut::<Assets<Mesh>>()
        .add(source_mesh([60.0, 252.0]));
    let material = app
        .world_mut()
        .resource_mut::<Assets<TerrainMaterial>>()
        .add(terrain_material());
    let source = app
        .world_mut()
        .spawn((
            Mesh3d(mesh),
            MeshMaterial3d(material),
            GlobalTransform::default(),
            InheritedVisibility::VISIBLE,
        ))
        .id();
    app.update();
    let first = parts(&mut app);
    assert_eq!(
        first.iter().map(|(_, part)| *part).collect::<Vec<_>>(),
        [
            Part::new(Kind::Drawn(252), 0),
            Part::new(Kind::Drawn(60), 0),
            Part::new(Kind::Traced(0), 0),
            Part::new(Kind::Traced(252), 0)
        ]
    );
    let traced = app.world().entity(source);
    assert!(traced.contains::<Traced>());
    assert_eq!(traced.get::<RenderLayers>(), Some(&RenderLayers::none()));

    app.update();
    assert_eq!(
        parts(&mut app),
        first,
        "an unchanged source keeps its parts"
    );

    // Posing swaps the source mesh for another asset; the parts follow it.
    let posed = app
        .world_mut()
        .resource_mut::<Assets<Mesh>>()
        .add(source_mesh([60.0, 60.0]));
    app.world_mut().entity_mut(source).insert(Mesh3d(posed));
    app.update();
    assert_eq!(
        parts(&mut app)
            .iter()
            .map(|(_, part)| *part)
            .collect::<Vec<_>>(),
        [Part::new(Kind::Drawn(60), 0), Part::new(Kind::Traced(0), 0)]
    );

    app.world_mut()
        .entity_mut(source)
        .insert(InheritedVisibility::HIDDEN);
    app.update();
    assert!(
        parts(&mut app).is_empty(),
        "a hidden source loses its parts"
    );
    let restored = app.world().entity(source);
    assert!(!restored.contains::<Traced>());
    assert!(!restored.contains::<RenderLayers>(), "its draw is restored");
}

/// A flat grid of `side` by `side` vertices, a quad per cell.
fn grid(side: u32, lift: Option<usize>) -> Mesh {
    let mut positions: Vec<[f32; 3]> = (0..side * side)
        .map(|index| [(index % side) as f32, 0.0, (index / side) as f32])
        .collect();
    if let Some(vertex) = lift {
        positions[vertex][1] = 1.0;
    }
    let count = positions.len();
    let indices = (0..side - 1)
        .flat_map(|z| (0..side - 1).map(move |x| z * side + x))
        .flat_map(|corner| {
            [
                corner,
                corner + 1,
                corner + side,
                corner + 1,
                corner + side + 1,
                corner + side,
            ]
        })
        .collect();
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; count])
    .with_inserted_indices(Indices::U32(indices))
}

#[test]
fn a_large_mesh_is_traced_in_tiles_and_a_local_edit_changes_one() {
    let source = grid(200, None);
    let traced: Vec<(u16, Layout, Mesh)> = layouts(&source)
        .expect("triangle list with normals")
        .into_iter()
        .filter_map(|(part, layout)| {
            let layout = layout.filter(|_| matches!(part.kind, Kind::Traced(_)))?;
            let mesh = layout.mesh(&source)?;
            Some((part.tile, layout, mesh))
        })
        .collect();
    assert_eq!(traced.len(), 4, "about TILE_VERTICES each");
    let faces: usize = traced
        .iter()
        .map(|(_, _, mesh)| mesh.indices().map_or(0, Indices::len))
        .sum();
    assert_eq!(faces, 199 * 199 * 6, "every face in exactly one tile");
    // A vertex near one corner, well inside its tile.
    let edited = grid(200, Some(10 * 200 + 10));
    let stale: Vec<_> = traced
        .iter()
        .filter(|(_, layout, mesh)| !layout.holds(&edited, mesh))
        .map(|(tile, _, _)| *tile)
        .collect();
    assert_eq!(stale, [0]);
}

#[test]
fn an_edit_in_place_rewrites_only_the_parts_it_changed() {
    let mut meshes = Assets::<Mesh>::default();
    let id = meshes.add(grid(200, None)).id();
    let source = meshes.get(id).unwrap().clone();
    let mut held = PartMeshes::default();
    rebuild(&mut meshes, &mut held, &source, layouts(&source).unwrap());
    let before: Vec<_> = held
        .parts
        .iter()
        .map(|(_, handle, _)| meshes.get(handle).unwrap().clone())
        .collect();
    let lifted = grid(200, Some(10 * 200 + 10));
    *meshes.get_mut(id).unwrap() = lifted;
    assert!(refresh(&mut meshes, &held, id), "the layouts still fit");
    let rewritten = held
        .parts
        .iter()
        .zip(&before)
        .filter(|((_, handle, _), old)| {
            meshes
                .get(handle)
                .unwrap()
                .attribute(Mesh::ATTRIBUTE_POSITION)
                .unwrap()
                .as_float3()
                != old.attribute(Mesh::ATTRIBUTE_POSITION).unwrap().as_float3()
        })
        .map(|((_, handle, _), _)| handle.id())
        .collect::<HashSet<_>>()
        .len();
    assert_eq!(rewritten, 1, "one tile, drawn from its traced mesh");
}
