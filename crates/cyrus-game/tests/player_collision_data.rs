use cyrus_assets::collision::CollisionGeometry;
use cyrus_assets::collision::CollisionModel;
use cyrus_assets::collision::CollisionSubobject;

#[test]
fn authored_collision_preserves_subsets_and_rejects_invalid_references() {
    let mut model = CollisionModel {
        flags: 0,
        geometry: CollisionGeometry {
            version: Some(*b"v5.0"),
            textures: None,
            radius: 20.0,

            vertices: vec![[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [0.0, 10.0, 0.0]],
            normals: vec![[0.0, 0.0, 1.0]],
            faces: vec![vec![0, 1, 2]],
            subobjects: vec![CollisionSubobject {
                center: [0.0; 3],
                radius: 20.0,
                extents: None,
                faces: vec![0, 0],
            }],
        },
    };
    let value = serde_json::to_value(&model).unwrap();
    assert!(value.get("geometry").is_none());
    assert_eq!(value["flags"], 0);
    assert_eq!(value["radius"], 20.0);
    let bytes = serde_json::to_vec(&model).unwrap();
    let decoded: CollisionModel = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(decoded.geometry.subobjects[0].faces, [0, 0]);
    assert!(decoded.geometry.subobjects[0].extents.is_none());
    assert_eq!(
        cyrus_collision::geometry::CollisionFace::from_geometry(&decoded.geometry)
            .unwrap()
            .len(),
        1
    );
    model.geometry.subobjects[0].faces.push(1);
    assert!(cyrus_collision::geometry::CollisionFace::from_geometry(&model.geometry).is_err());
    model.geometry.subobjects[0].faces.pop();
    model.geometry.faces[0][2] = 3;
    assert!(cyrus_collision::geometry::CollisionFace::from_geometry(&model.geometry).is_err());
    model.geometry.faces[0][2] = 2;
    model.geometry.normals[0] = [0.0; 3];
    assert!(cyrus_collision::geometry::CollisionFace::from_geometry(&model.geometry).is_err());
}
