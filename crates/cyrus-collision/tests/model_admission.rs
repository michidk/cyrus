//! Camera boundary and child traversal contracts through the model-admission API.
use cyrus_assets::collision::{ModelVisibility, VisibilityNode};
use cyrus_collision::model_admission::ModelAdmission;

#[test]
fn camera_selection_keeps_strict_octants_boundaries_and_authored_order() {
    let mut root = VisibilityNode {
        children: [None; 8],
        objects: vec![4, 4, 2],
    };
    root.children[7] = Some(1);
    let mut child = VisibilityNode {
        children: [None; 8],
        objects: vec![9, 3],
    };
    child.children[0] = Some(2);
    let admission = ModelAdmission::new(ModelVisibility {
        center: [100, -200, 300],
        half_extent: 8,
        nodes: vec![
            root,
            child,
            VisibilityNode {
                children: [None; 8],
                objects: vec![7],
            },
        ],
    })
    .unwrap();
    assert_eq!(admission.select([100, -200, 300]), [4, 4, 2]);
    assert_eq!(admission.select([101, -199, 301]), [7]);
    assert_eq!(admission.select([104, -196, 304]), [7]);
    assert_eq!(admission.select([108, -192, 308]), [9, 3]);
    assert!(admission.select([109, -200, 300]).is_empty());
    assert!(admission.select([i32::MIN, i32::MAX, 300]).is_empty());
    assert!(
        ModelAdmission::new(ModelVisibility::default())
            .unwrap()
            .select([0; 3])
            .is_empty()
    );
}

#[test]
fn malformed_tree_cannot_enter_runtime_selection() {
    for children in [[Some(0); 8], [Some(3); 8]] {
        assert!(
            ModelAdmission::new(ModelVisibility {
                center: [0; 3],
                half_extent: 8,
                nodes: vec![VisibilityNode {
                    children,
                    objects: vec![]
                }],
            })
            .is_err()
        );
    }
}

#[test]
fn shared_child_records_are_valid_for_multiple_octants() {
    let mut root = VisibilityNode {
        children: [None; 8],
        objects: vec![],
    };
    root.children[0] = Some(1);
    root.children[7] = Some(1);
    let admission = ModelAdmission::new(ModelVisibility {
        center: [0; 3],
        half_extent: 8,
        nodes: vec![
            root,
            VisibilityNode {
                children: [None; 8],
                objects: vec![5],
            },
        ],
    })
    .unwrap();
    assert_eq!(admission.select([-1; 3]), [5]);
    assert_eq!(admission.select([1; 3]), [5]);
}
