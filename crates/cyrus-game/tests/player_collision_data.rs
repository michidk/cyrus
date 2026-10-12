use cyrus_assets::collision::CollisionModel;
use cyrus_assets::collision::CollisionSubobject;

#[test]
fn authored_collision_preserves_subsets_and_rejects_invalid_references() {
    let mut model = CollisionModel {
        version: Some(*b"v5.0"),
        textures: None,
        radius: 20.0,
        flags: 0,
        vertices: vec![[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [0.0, 10.0, 0.0]],
        normals: vec![[0.0, 0.0, 1.0]],
        faces: vec![vec![0, 1, 2]],
        subobjects: vec![CollisionSubobject {
            center: [0.0; 3],
            radius: 20.0,
            extents: None,
            faces: vec![0, 0],
        }],
    };
    let bytes = serde_json::to_vec(&model).unwrap();
    let decoded: CollisionModel = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(decoded.subobjects[0].faces, [0, 0]);
    assert!(decoded.subobjects[0].extents.is_none());
    assert_eq!(
        cyrus_collision::geometry::CollisionFace::from_model(&decoded)
            .unwrap()
            .len(),
        1
    );
    model.subobjects[0].faces.push(1);
    assert!(cyrus_collision::geometry::CollisionFace::from_model(&model).is_err());
    model.subobjects[0].faces.pop();
    model.faces[0][2] = 3;
    assert!(cyrus_collision::geometry::CollisionFace::from_model(&model).is_err());
    model.faces[0][2] = 2;
    model.normals[0] = [0.0; 3];
    assert!(cyrus_collision::geometry::CollisionFace::from_model(&model).is_err());
}
