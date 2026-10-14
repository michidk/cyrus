use bevy::math::Vec3;
use cyrus_assets::collision::CollisionGeometry;
use cyrus_assets::collision::CollisionModel;
use cyrus_assets::collision::CollisionSubobject;
use cyrus_collision::collision_mesh::CollisionMesh;

fn model() -> CollisionModel {
    CollisionModel {
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
                radius: 1.0,
                extents: Some([1.0, 10.0, 1.0]),
                faces: vec![0; 40],
            }],
        },
    }
}

#[test]
fn segment_uses_extent_scaled_admission_and_retains_first_32_duplicate_contacts() {
    let mut model = model();
    let start = Vec3::new(0.2, 5.0, -2.0);
    let end = Vec3::new(0.2, 5.0, 2.0);
    let contacts = CollisionMesh::new(&model.geometry)
        .unwrap()
        .segment_contacts(start, end, true)
        .unwrap();
    assert_eq!(contacts.len(), 32);
    for contact in contacts {
        assert_eq!(contact.contact.point, Vec3::new(0.2, 5.0, 0.0));
        assert_eq!(contact.contact.rank, 16_384);
    }
    model.geometry.subobjects[0].extents = Some([1.0; 3]);
    assert!(
        CollisionMesh::new(&model.geometry)
            .unwrap()
            .segment_contacts(start, end, true)
            .unwrap()
            .is_empty()
    );
    model.geometry.subobjects[0].extents = None;
    assert!(
        CollisionMesh::new(&model.geometry)
            .unwrap()
            .segment_contacts(start, end, true)
            .unwrap_err()
            .to_string()
            .contains("version-5")
    );
}

#[test]
fn model_radius_rejects_before_unsupported_subgroup_and_invalid_endpoints_fail() {
    let mut model = model();
    model.geometry.subobjects[0].extents = None;
    let mesh = CollisionMesh::new(&model.geometry).unwrap();
    assert!(
        mesh.segment_contacts(Vec3::splat(100.0), Vec3::splat(101.0), true)
            .unwrap()
            .is_empty()
    );
    assert!(mesh.segment_contacts(Vec3::NAN, Vec3::ZERO, true).is_err());
}

#[test]
fn body_contacts_retry_misses_but_suppress_successes_across_spheres_and_groups() {
    let mut source = model();
    source
        .geometry
        .subobjects
        .push(source.geometry.subobjects[0].clone());
    let mesh = CollisionMesh::new(&source.geometry).unwrap();
    let contacts = mesh
        .sphere_contacts(
            &[
                (Vec3::new(0.2, 5.0, 2.0), 1.0),
                (Vec3::new(0.2, 5.0, 0.5), 1.0),
                (Vec3::new(0.2, 5.0, 0.25), 1.0),
            ],
            true,
        )
        .unwrap();
    assert_eq!(contacts.len(), 1);
    assert_eq!(contacts[0].face, 0);
    assert_eq!(contacts[0].projection, Vec3::new(0.2, 5.0, 0.0));
    assert_eq!(contacts[0].normal, Vec3::Z);

    source.geometry.faces = vec![vec![0, 1, 2]; 33];
    source.geometry.normals = vec![[0.0, 0.0, 1.0]; 33];
    source.geometry.subobjects[0].faces = (0..33).collect();
    let saturated = CollisionMesh::new(&source.geometry)
        .unwrap()
        .sphere_contacts(&[(Vec3::new(0.2, 5.0, 0.5), 1.0)], true)
        .unwrap();
    assert_eq!(
        saturated
            .iter()
            .map(|contact| contact.face)
            .collect::<Vec<_>>(),
        (0..32).collect::<Vec<_>>()
    );
}

#[test]
fn version_four_authored_subobjects_are_not_runtime_collision() {
    let mut source = model();
    source.geometry.version = Some(*b"v4.0");
    source.geometry.subobjects[0].extents = None;
    let mesh = CollisionMesh::new(&source.geometry).unwrap();
    assert!(
        mesh.segment_contacts(Vec3::new(0.2, 5.0, -2.0), Vec3::new(0.2, 5.0, 2.0), true)
            .unwrap()
            .is_empty()
    );
    let spheres = [(Vec3::new(0.2, 5.0, 0.5), 1.0)];
    assert!(mesh.sphere_contacts(&spheres, true).unwrap().is_empty());
    assert!(!mesh.intersects_spheres(&spheres, true).unwrap());
    source.geometry.version = None;
    assert!(CollisionMesh::new(&source.geometry).is_err());
}

#[test]
fn world_segment_integer_adapter_rounds_ties_and_retains_rank_without_normalizing() {
    use cyrus_collision::collision_mesh::{IntegerSegmentContact, WorldSegmentContact};
    let contact = WorldSegmentContact {
        face: 7,
        rank: 0x8001,
        point: [0.5, -0.5, 1.5],
        normal: [0.5 / 256.0, -1.5 / 256.0, 2.5 / 256.0],
    };
    assert_eq!(
        contact.integer().unwrap(),
        IntegerSegmentContact {
            face: 7,
            rank: 0x8001,
            point: [0, 0, 2],
            normal: [0, -2, 2],
        }
    );
    assert!(
        WorldSegmentContact {
            point: [f32::NAN, 0.0, 0.0],
            ..contact
        }
        .integer()
        .is_err()
    );
}
