//! Contact, constraint and resource ownership contracts through the host API.
use super::*;
fn fixture() -> Physics {
    let mut host = Physics::default();
    host.simulation = Some(Simulation::default());
    host.owners.insert(
        "test".into(),
        Lease {
            instance: Entity::from_bits(42),
            revision: 1,
        },
    );
    host.models.insert(
        1,
        Model {
            node: Entity::from_bits(43),
            transform: Mat4::IDENTITY,
            vertices: vec![0., 0., 0.],
            reference: vec![0., 0., 0.],
            logical: cyrus_script::script::ScriptActorId {
                generation: 1,
                index: 0,
            },
            revision: 0,
            name: "TEST".into(),
            source: Handle::default(),
            frame: 0,
            attachments: vec![],
        },
    );
    host
}
fn assembly(bodies: usize, joints: bool) -> String {
    let bodies = (0..bodies).map(|i| serde_json::json!({
            "pose":[i as f32,3.,0.,0.,0.,0.,1.],"shape":{"Sphere":0.5},"mass":1.,
            "linear_damping":0.1,"angular_damping":0.2,"velocity":[0.,0.,0.],"acceleration":[0.,-9.81,0.],"impulse":[0.,0.,0.]
        })).collect::<Vec<_>>();
    serde_json::json!({"bodies":bodies,"joints":if joints {vec![serde_json::json!({
            "bodies":[0,1],"frame":[0.5,3.,0.,0.,0.,0.,1.],"limits":[[-0.2,0.2],[-0.2,0.2],[-0.2,0.2]]
        })]} else {vec![]}, "weights":[[[0,1.]]]}).to_string()
}
#[test]
fn bodies_settle_on_world_contact_and_release_is_owned() {
    let mut host = fixture();
    host.simulation.as_mut().unwrap().colliders.insert(
        rp::ColliderBuilder::cuboid(10., 0.5, 10.)
            .translation(vector([0., -0.5, 0.]))
            .build(),
    );
    assert_eq!(host.claim("test".into(), 1, assembly(1, false)), 0);
    for _ in 0..600 {
        host.simulation.as_mut().unwrap().advance(1. / 120.);
    }
    let transforms = host.transforms("test".into(), 1);
    assert_eq!(transforms.len(), 7);
    assert!(
        (transforms[1] - 0.5).abs() < 0.03,
        "resting height {}",
        transforms[1]
    );
    assert!(!host.release("other".into(), 1));
    assert!(!host.transforms("test".into(), 1).is_empty());
    assert!(host.release("test".into(), 1));
    assert!(host.transforms("test".into(), 1).is_empty());
}
#[test]
fn joints_preserve_anchors_and_limit_relative_rotation() {
    let mut host = fixture();
    assert_eq!(host.claim("test".into(), 1, assembly(2, true)), 0);
    let handles = host.claims[&1].bodies.clone();
    let sim = host.simulation.as_mut().unwrap();
    sim.bodies[handles[1]].apply_torque_impulse(vector([10., 5., 2.]), true);
    for _ in 0..300 {
        sim.advance(1. / 120.);
    }
    let a = sim.bodies[handles[0]].position();
    let b = sim.bodies[handles[1]].position();
    let pa = a.transform_point(vector([0.5, 0., 0.]));
    let pb = b.transform_point(vector([-0.5, 0., 0.]));
    assert!((pa - pb).length() < 0.05);
    let relative = a.rotation.inverse() * b.rotation;
    assert!(
        relative.to_scaled_axis().length() < 0.5,
        "relative rotation {:?}",
        relative.to_scaled_axis()
    );
}
#[test]
fn requests_reject_atomically_and_fixed_time_caps_catch_up() {
    let mut host = fixture();
    let mut invalid: serde_json::Value = serde_json::from_str(&assembly(2, true)).unwrap();
    invalid["joints"][0]["bodies"][1] = 32.into();
    assert_eq!(host.claim("test".into(), 1, invalid.to_string()), 1);
    assert!(host.claims.is_empty());
    assert!(host.simulation.as_ref().unwrap().bodies.is_empty());
    assert_eq!(host.claim("test".into(), 1, assembly(1, false)), 0);
    assert_eq!(host.claim("test".into(), 1, assembly(1, false)), 3);
    let before = host.transforms("test".into(), 1);
    host.simulation.as_mut().unwrap().advance(0.);
    assert_eq!(host.transforms("test".into(), 1), before);
    host.simulation.as_mut().unwrap().advance(10.);
    let after = host.transforms("test".into(), 1);
    assert!(after[1] > 2.9, "catch-up must be bounded: {}", after[1]);
}
#[test]
fn owner_budget_does_not_allocate_partial_assemblies() {
    let mut host = fixture();
    for actor in 1..=9 {
        host.models.insert(
            actor,
            Model {
                node: Entity::from_bits(actor + 100),
                transform: Mat4::IDENTITY,
                vertices: vec![0., 0., 0.],
                reference: vec![0., 0., 0.],
                logical: cyrus_script::script::ScriptActorId {
                    generation: 1,
                    index: 0,
                },
                revision: 0,
                name: "TEST".into(),
                source: Handle::default(),
                frame: 0,
                attachments: vec![],
            },
        );
        assert_eq!(
            host.claim("test".into(), actor, assembly(32, false)),
            if actor <= 8 { 0 } else { 4 }
        );
    }
    assert!(host.transforms("test".into(), 9).is_empty());
    assert_eq!(host.simulation.as_ref().unwrap().bodies.len(), OWNER_BODIES);
}

/// Runs the real bundled component through wasvy's host method dispatch.
#[test]
#[ignore = "requires CYRUS_RAGDOLL_WASM built for wasm32-wasip2"]
fn wasm_ragdoll_claims_an_articulated_assembly_through_the_typed_sdk() {
    #[derive(Resource)]
    struct ComponentPath(String);
    fn load(path: Res<ComponentPath>, mut mods: Mods) {
        let instance = mods.spawn(path.0.clone(), None);
        mods.enable_access(instance, ModAccess::World);
    }
    let path = std::env::var("CYRUS_RAGDOLL_WASM")
        .expect("set CYRUS_RAGDOLL_WASM to the actual bundled component");
    let path = std::path::Path::new(&path);
    assert!(path.is_file());
    let asset_root = path.parent().unwrap().to_string_lossy().into_owned();
    let component = path.file_name().unwrap().to_string_lossy().into_owned();
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        bevy::asset::AssetPlugin {
            file_path: asset_root,
            ..default()
        },
        ModLoaderPlugin::default().add_functionality(crate::add_components_to_linker),
    ))
    .insert_resource(ComponentPath(component))
    .add_systems(Startup, (crate::api::spawn_host, load));
    app.finish();
    app.cleanup();
    app.update();
    let entity = app
        .world_mut()
        .query_filtered::<Entity, With<Physics>>()
        .single(app.world())
        .unwrap();
    let mut physics = fixture();
    let lease = physics.owners.remove("test").unwrap();
    physics.owners.insert("ragdolls".into(), lease);
    let positions = (0..5)
        .flat_map(|x| {
            (0..5).flat_map(move |y| (0..3).flat_map(move |z| [x as f32, y as f32, z as f32]))
        })
        .collect::<Vec<_>>();
    let model = physics.models.get_mut(&1).unwrap();
    model.name = "CYRSA001".into();
    model.vertices = positions.clone();
    model.reference = positions;
    app.world_mut()
        .entity_mut(entity)
        .insert((physics, Actors::fixture_death(1)));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    loop {
        app.update();
        let host = app.world().get::<Physics>(entity).unwrap();
        if let Some(claim) = host.claims.get(&1) {
            assert!(claim.bodies.len() >= 8);
            assert_eq!(claim.weights.len(), 75);
            assert!(host.simulation.as_ref().unwrap().joints.len() >= 7);
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "actual Wasm did not claim the death fixture"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let mut host = app.world_mut().get_mut::<Physics>(entity).unwrap();
    assert!(host.release("ragdolls".into(), 1));
    assert!(host.simulation.as_ref().unwrap().bodies.is_empty());
}

#[test]
fn disable_reload_rebinding_and_revival_revision_retire_solver_and_render_ownership() {
    for exit in 0..4 {
        let mut host = fixture();
        assert_eq!(host.claim("test".into(), 1, assembly(2, true)), 0);
        assert!(host.retire_invalid().is_empty());
        match exit {
            0 => {
                host.owners.clear();
            }
            1 => host.owners.get_mut("test").unwrap().revision += 1,
            2 => host.models.get_mut(&1).unwrap().node = Entity::from_bits(999),
            3 => host.models.get_mut(&1).unwrap().revision += 1,
            _ => unreachable!(),
        }
        assert_eq!(host.retire_invalid().len(), 1);
        assert!(host.claims.is_empty());
        let sim = host.simulation.as_ref().unwrap();
        assert!(sim.bodies.is_empty());
        assert!(sim.colliders.is_empty());
        assert!(sim.joints.is_empty());
    }
}
